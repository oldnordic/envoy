use std::net::SocketAddr;

use envoy::server;

#[tokio::main]
async fn main() {
    let db_path =
        std::env::var("ENVOY_DB").unwrap_or_else(|_| "/home/feanor/.envoy/server.db".to_string());

    // Ensure parent directory exists
    if let Some(parent) = std::path::Path::new(&db_path).parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let port: u16 = std::env::var("ENVOY_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(9876);

    let addr: SocketAddr = ([127, 0, 0, 1], port).into();

    if let Err(e) = server::run(&db_path, addr).await {
        eprintln!("envoy server error: {e}");
        std::process::exit(1);
    }
}
