use std::net::SocketAddr;
use std::time::Duration;

use envoy::server;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Resolve the envoy database path: `--db` flag > `ENVOY_DB` env > default
/// `$XDG_DATA_HOME/envoy/server.db` (or `~/.local/share/envoy/server.db`).
fn resolve_db_path(flag: Option<&str>) -> String {
    if let Some(p) = flag {
        return p.to_string();
    }
    if let Ok(p) = std::env::var("ENVOY_DB") {
        return p;
    }
    let base = std::env::var("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .ok()
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| std::path::PathBuf::from(h).join(".local").join("share"))
        })
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    base.join("envoy")
        .join("server.db")
        .to_string_lossy()
        .into_owned()
}

fn default_http_port() -> u16 {
    std::env::var("ENVOY_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(9876)
}

fn default_atheneum() -> Option<String> {
    std::env::var("ATHENEUM_DB").ok()
}

fn print_usage() {
    eprintln!(
        "envoy v{VERSION} — message/coordination server for AI coding agents\n\
\n\
USAGE:\n    envoy <SUBCOMMAND> [OPTIONS]\n\
\n\
SUBCOMMANDS:\n    \
local    Run as a local daemon over a Unix domain socket (no TCP port).\n            \
Default local-dev transport. Same business logic as HTTP.\n    \
serve    Run the HTTP server on a TCP port. Network access + universal\n            \
curl fallback. Explicit opt-in.\n    \
status   Introspection: report daemon liveness and config. Binds nothing,\n            \
starts no server.\n\
\n\
OPTIONS (local):\n    \
--socket <PATH>    Socket path (default: $XDG_RUNTIME_DIR/envoy.sock)\n    \
--db <PATH>        Database path (default: $ENVOY_DB or ~/.local/share/envoy/server.db)\n    \
--atheneum <PATH>  Atheneum database path (default: $ATHENEUM_DB)\n\
\n\
OPTIONS (serve):\n    \
--port <PORT>      TCP port (default: $ENVOY_PORT or 9876)\n    \
--bind <ADDR>      Bind address (default: 127.0.0.1)\n    \
--db <PATH>        Database path\n    \
--atheneum <PATH>  Atheneum database path\n\
\n\
LEGACY: `envoy --port 9876` (bare flags) is treated as `envoy serve ...`."
    );
}

/// Parse `--flag value` pairs out of an args slice into a lookup helper.
struct Flags<'a> {
    port: Option<&'a str>,
    bind: Option<&'a str>,
    socket: Option<&'a str>,
    db: Option<&'a str>,
    atheneum: Option<&'a str>,
}

fn parse_flags(args: &[String]) -> Flags<'_> {
    let mut flags = Flags {
        port: None,
        bind: None,
        socket: None,
        db: None,
        atheneum: None,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--port" if i + 1 < args.len() => {
                flags.port = Some(&args[i + 1]);
                i += 2;
            }
            "--bind" if i + 1 < args.len() => {
                flags.bind = Some(&args[i + 1]);
                i += 2;
            }
            "--socket" if i + 1 < args.len() => {
                flags.socket = Some(&args[i + 1]);
                i += 2;
            }
            "--db" if i + 1 < args.len() => {
                flags.db = Some(&args[i + 1]);
                i += 2;
            }
            "--atheneum" if i + 1 < args.len() => {
                flags.atheneum = Some(&args[i + 1]);
                i += 2;
            }
            _ => i += 1,
        }
    }
    flags
}

#[tokio::main]
async fn main() {
    // Use args_os() rather than args(): identical for UTF-8 input, but it does
    // not panic on non-UTF-8 argv (lossy conversion instead), which is the
    // robust choice for a daemon entry point.
    //
    // nosemgrep is required because the p/rust registry ships blanket rules
    // (`args.args`, `args-os.args-os`) that flag *any* read of process argv,
    // on the theory that args[0] may be spoofed and must not be a trust
    // anchor. That does not apply here: we use argv purely for CLI dispatch
    // (subcommand + flags) and never treat args[0] as a path, identity, or
    // security boundary.
    // nosemgrep: rust.lang.security.args-os.args-os
    let args: Vec<String> = std::env::args_os()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();

    // No subcommand at all → usage, bind nothing. (Previously every
    // invocation — even `--help` — bound the TCP port.)
    if args.len() < 2 {
        print_usage();
        std::process::exit(0);
    }

    let sub = args[1].as_str();

    // help / version
    if matches!(sub, "-h" | "--help" | "help") {
        print_usage();
        return;
    }
    if matches!(sub, "-V" | "--version") {
        println!("envoy {VERSION}");
        return;
    }

    // Help/version flags appearing AFTER a subcommand (e.g. `envoy local
    // --help`, `envoy serve --version`) must print and exit — never start a
    // server. Without this, `parse_flags` silently ignores the unknown flag and
    // the daemon binds the socket/port forever.
    let sub_args = &args[2..];
    if sub_args
        .iter()
        .any(|a| matches!(a.as_str(), "-h" | "--help"))
    {
        print_usage();
        return;
    }
    if sub_args
        .iter()
        .any(|a| matches!(a.as_str(), "-V" | "--version"))
    {
        println!("envoy {VERSION}");
        return;
    }

    let result = match sub {
        // -----------------------------------------------------------------
        // local — Unix domain socket, no TCP port
        // -----------------------------------------------------------------
        "local" => {
            let flags = parse_flags(&args[2..]);
            let db_path = resolve_db_path(flags.db);
            ensure_db_parent(&db_path);
            let socket_path = flags
                .socket
                .map(std::path::PathBuf::from)
                .unwrap_or_else(server::default_socket_path);
            server::run_local(&db_path, socket_path, flags.atheneum.map(str::to_string)).await
        }

        // -----------------------------------------------------------------
        // serve — HTTP over TCP (network + universal curl fallback)
        // -----------------------------------------------------------------
        "serve" => {
            let flags = parse_flags(&args[2..]);
            let db_path = resolve_db_path(flags.db);
            ensure_db_parent(&db_path);
            let port: u16 = flags
                .port
                .map(|p| p.parse().unwrap_or(default_http_port()))
                .unwrap_or(default_http_port());
            let bind_host = flags.bind.unwrap_or("127.0.0.1");
            let addr: SocketAddr = format!("{bind_host}:{port}").parse().unwrap_or_else(|_| {
                eprintln!("invalid bind address: {bind_host}:{port}");
                std::process::exit(1);
            });
            server::run_with_atheneum(&db_path, addr, flags.atheneum.map(str::to_string)).await
        }

        // -----------------------------------------------------------------
        // status — introspection, binds nothing, starts no server
        // -----------------------------------------------------------------
        "status" => {
            run_status(&args[2..]).await;
            return;
        }

        // -----------------------------------------------------------------
        // Legacy: bare flag form (`envoy --port 9876`) → treat as `serve`.
        // -----------------------------------------------------------------
        other if other.starts_with("--") => {
            let flags = parse_flags(&args[1..]);
            let db_path = resolve_db_path(flags.db);
            ensure_db_parent(&db_path);
            let port: u16 = flags
                .port
                .map(|p| p.parse().unwrap_or(default_http_port()))
                .unwrap_or(default_http_port());
            let bind_host = flags.bind.unwrap_or("127.0.0.1");
            let addr: SocketAddr = format!("{bind_host}:{port}").parse().unwrap_or_else(|_| {
                eprintln!("invalid bind address: {bind_host}:{port}");
                std::process::exit(1);
            });
            server::run_with_atheneum(&db_path, addr, flags.atheneum.map(str::to_string)).await
        }

        other => {
            eprintln!("unknown subcommand: {other}\n");
            print_usage();
            std::process::exit(2);
        }
    };

    if let Err(e) = result {
        eprintln!("envoy server error: {e}");
        std::process::exit(1);
    }
}

fn ensure_db_parent(db_path: &str) {
    if let Some(parent) = std::path::Path::new(db_path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
}

/// Introspection: report daemon liveness (Unix socket + HTTP) and config.
/// Pure client-side probe — binds nothing and starts no server.
async fn run_status(args: &[String]) {
    let flags = parse_flags(args);
    let db_path = resolve_db_path(flags.db);
    let socket_path = flags
        .socket
        .map(std::path::PathBuf::from)
        .unwrap_or_else(server::default_socket_path);
    let port: u16 = flags
        .port
        .map(|p| p.parse().unwrap_or(default_http_port()))
        .unwrap_or(default_http_port());
    let atheneum = flags.atheneum.map(str::to_string).or_else(default_atheneum);

    println!("envoy {VERSION}");
    println!("  db:       {db_path}");
    println!("  socket:   {}", socket_path.display());
    println!("  http:     127.0.0.1:{port}");
    if let Some(a) = &atheneum {
        println!("  atheneum: {a}");
    }

    // Probe the Unix socket (local-RPC daemon).
    let socket_up = tokio::net::UnixStream::connect(&socket_path).await.is_ok();
    println!(
        "\n  local daemon: {}",
        if socket_up {
            "UP (socket reachable)"
        } else {
            "DOWN"
        }
    );

    // Probe the HTTP port.
    let http_addr = format!("127.0.0.1:{port}");
    let http_up = tokio::time::timeout(
        Duration::from_millis(300),
        tokio::net::TcpStream::connect(&http_addr),
    )
    .await
    .map(|r| r.is_ok())
    .unwrap_or(false);
    println!(
        "  http daemon:  {}",
        if http_up {
            "UP (port reachable)"
        } else {
            "DOWN (use `envoy serve` to start)"
        }
    );
}
