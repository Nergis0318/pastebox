use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;

pub struct Config {
    pub listen_addr: SocketAddr,
    pub data_dir: PathBuf,
    pub expire_days: u32,
}

impl Config {
    pub fn from_env() -> Self {
        let addr = env::var("PASTEBOX_LISTEN_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:8080".into());
        let data_dir = env::var("PASTEBOX_DATA_DIR")
            .unwrap_or_else(|_| "/paste-data".into());
        let expire_days: u32 = env::var("PASTEBOX_EXPIRE_DAYS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);

        Self {
            listen_addr: addr.parse().expect("invalid LISTEN_ADDR"),
            data_dir: PathBuf::from(data_dir),
            expire_days,
        }
    }
}
