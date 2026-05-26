use rand::Rng;

const PASSWORD_UPPER: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
const PASSWORD_LOWER: &[u8] = b"abcdefghjkmnpqrstuvwxyz";
const PASSWORD_DIGITS: &[u8] = b"23456789";
const PASSWORD_SPECIAL: &[u8] = b"!@#$%^&*";

const TOKEN_ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

pub fn looks_like_text(data: &[u8]) -> bool {
    if data.contains(&0) {
        return false;
    }
    let sample = if data.len() > 512 { &data[..512] } else { data };
    match std::str::from_utf8(sample) {
        Ok(s) => {
            let control_count = s.chars().filter(|c| c.is_control() && *c != '\n' && *c != '\r' && *c != '\t').count();
            control_count == 0
        }
        Err(_) => false,
    }
}

pub fn detect_content_type(data: &[u8], header: Option<&str>) -> String {
    if let Some(h) = header
        && let Some(ct) = h.split(';').next()
    {
        let ct = ct.trim();
        if !ct.is_empty() && ct != "application/octet-stream" {
            return ct.to_string();
        }
    }
    let mime_str = match infer::get(data) {
        Some(t) => t.mime_type().to_string(),
        None => "application/octet-stream".to_string(),
    };
    if mime_str == "text/plain" && looks_like_text(data) {
        "text/plain; charset=utf-8".to_string()
    } else {
        mime_str
    }
}

pub fn is_browser_request(user_agent: Option<&str>) -> bool {
    match user_agent {
        Some(ua) => {
            let ua_lower = ua.to_lowercase();
            !ua_lower.contains("curl")
                && !ua_lower.contains("wget")
                && !ua_lower.contains("httpie")
                && !ua_lower.contains("go-http-client")
        }
        None => false,
    }
}

pub fn request_base_url(
    scheme: Option<&str>,
    host: Option<&str>,
    forwarded_proto: Option<&str>,
    forwarded_host: Option<&str>,
) -> String {
    let proto = forwarded_proto.unwrap_or(scheme.unwrap_or("http"));
    let fwd_host = forwarded_host.or(host).unwrap_or("localhost");
    let host = fwd_host.split(',').next().unwrap_or("localhost").trim();
    format!("{proto}://{host}")
}

pub fn generate_password() -> String {
    let mut rng = rand::thread_rng();
    let mut chars: Vec<char> = vec![
        PASSWORD_UPPER[rng.gen_range(0..PASSWORD_UPPER.len())] as char,
        PASSWORD_LOWER[rng.gen_range(0..PASSWORD_LOWER.len())] as char,
        PASSWORD_DIGITS[rng.gen_range(0..PASSWORD_DIGITS.len())] as char,
        PASSWORD_SPECIAL[rng.gen_range(0..PASSWORD_SPECIAL.len())] as char,
    ];
    let all: Vec<u8> = [PASSWORD_UPPER, PASSWORD_LOWER, PASSWORD_DIGITS, PASSWORD_SPECIAL]
        .concat();
    for _ in 0..4 {
        chars.push(all[rng.gen_range(0..all.len())] as char);
    }
    fisher_yates_shuffle(&mut chars);
    chars.into_iter().collect()
}

pub fn random_token(len: usize) -> String {
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| {
            let idx = rng.gen_range(0..TOKEN_ALPHABET.len());
            TOKEN_ALPHABET[idx] as char
        })
        .collect()
}

fn fisher_yates_shuffle<T>(slice: &mut [T]) {
    let mut rng = rand::thread_rng();
    for i in (1..slice.len()).rev() {
        let j = rng.gen_range(0..=i);
        slice.swap(i, j);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_looks_like_text() {
        assert!(looks_like_text(b"hello world"));
        assert!(looks_like_text(b"hello\nworld"));
        assert!(looks_like_text(b"{\"key\": \"value\"}"));
        assert!(!looks_like_text(&[0, 1, 2, 3]));
        assert!(!looks_like_text(&[0xFF, 0xFE, 0x00, 0x00]));
    }

    #[test]
    fn test_is_browser_request() {
        assert!(!is_browser_request(Some("curl/7.68.0")));
        assert!(!is_browser_request(Some("Wget/1.20")));
        assert!(is_browser_request(Some("Mozilla/5.0 ...")));
        assert!(!is_browser_request(None));
    }

    #[test]
    fn test_generate_password_length() {
        let pw = generate_password();
        assert_eq!(pw.len(), 8);
        assert!(pw.chars().any(|c| c.is_uppercase()));
        assert!(pw.chars().any(|c| c.is_lowercase()));
        assert!(pw.chars().any(|c| c.is_ascii_digit()));
    }

    #[test]
    fn test_request_base_url() {
        let url = request_base_url(
            Some("https"),
            Some("example.com"),
            None,
            None,
        );
        assert_eq!(url, "https://example.com");

        let url2 = request_base_url(
            Some("http"),
            Some("localhost:8080"),
            Some("https"),
            Some("proxy.example.com"),
        );
        assert_eq!(url2, "https://proxy.example.com");
    }
}
