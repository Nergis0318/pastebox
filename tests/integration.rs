use std::process::{Child, Command};
use std::sync::Once;
use std::time::Duration;

static INIT: Once = Once::new();

fn setup_logging() {
    INIT.call_once(|| {
        tracing_subscriber::fmt()
            .with_env_filter("pastebox=debug")
            .try_init()
            .ok();
    });
}

struct TestServer {
    base_url: String,
    _child: Child,
}

impl TestServer {
    async fn start() -> anyhow::Result<Self> {
        let data_dir = tempfile::tempdir()?;
        let port = find_open_port()?;
        let base_url = format!("http://127.0.0.1:{port}");

        let bin = std::env::var("CARGO_BIN_EXE_pastebox")
            .unwrap_or_else(|_| {
                if cfg!(windows) {
                    "target/debug/pastebox.exe".into()
                } else {
                    "target/debug/pastebox".into()
                }
            });

        let child = Command::new(&bin)
            .env("PASTEBOX_LISTEN_ADDR", format!("127.0.0.1:{port}"))
            .env("PASTEBOX_DATA_DIR", data_dir.path().to_string_lossy().to_string())
            .env("PASTEBOX_EXPIRE_DAYS", "30")
            .spawn()?;

        // Wait for server to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        Ok(Self {
            base_url,
            _child: child,
        })
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self._child.kill();
        let _ = self._child.wait();
    }
}

fn find_open_port() -> std::io::Result<u16> {
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

#[tokio::test]
async fn test_upload_and_view_text() {
    setup_logging();
    let server = TestServer::start().await.unwrap();
    let client = reqwest::Client::new();

    // Upload
    let resp = client
        .post(&server.base_url)
        .body("Hello, Pastebox!")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    let paste_url = body.lines().next().unwrap().trim().to_string();
    assert!(paste_url.starts_with(&server.base_url));

    // View
    let resp = client.get(&paste_url).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let content = resp.text().await.unwrap();
    assert_eq!(content, "Hello, Pastebox!");
}

#[tokio::test]
async fn test_password_protected() {
    setup_logging();
    let server = TestServer::start().await.unwrap();
    let client = reqwest::Client::new();

    let resp = client
        .post(&server.base_url)
        .header("usepassword", "true")
        .body("secret")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();

    let paste_url = body.lines().next().unwrap().trim().to_string();
    let password = body
        .lines()
        .find(|l| l.starts_with("password: "))
        .unwrap()
        .strip_prefix("password: ")
        .unwrap()
        .trim();

    // Without password should fail
    let resp = client.get(&paste_url).send().await.unwrap();
    assert_eq!(resp.status(), 403);

    // With password should succeed (use header to avoid URL encoding issues)
    let resp = client
        .get(&paste_url)
        .header("paste-password", password)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_delete() {
    setup_logging();
    let server = TestServer::start().await.unwrap();
    let client = reqwest::Client::new();

    let resp = client
        .post(&server.base_url)
        .body("to be deleted")
        .send()
        .await
        .unwrap();

    let body = resp.text().await.unwrap();
    let delete_url = body
        .lines()
        .find(|l| l.starts_with("delete: "))
        .unwrap()
        .strip_prefix("delete: ")
        .unwrap()
        .trim()
        .to_string();

    // Delete
    let resp = client.get(&delete_url).send().await.unwrap();
    assert_eq!(resp.status(), 200);

    // Should no longer exist
    let paste_url = delete_url.split('?').next().unwrap();
    let resp = client.get(paste_url).send().await.unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_admin_flow() {
    setup_logging();
    let server = TestServer::start().await.unwrap();
    // Setup admin (server returns 303 redirect with Set-Cookie)
    let no_redirect = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let resp = no_redirect
        .post(&format!("{}/admin/setup", server.base_url))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("username=admin&password=admin123")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 303);
    assert!(resp.headers().get("set-cookie").is_some());
}

#[tokio::test]
async fn test_404() {
    setup_logging();
    let server = TestServer::start().await.unwrap();
    let client = reqwest::Client::new();

    let resp = client
        .get(&format!("{}/nonexistent", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}
