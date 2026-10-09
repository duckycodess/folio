//! The loopback inference client must never route through a configured
//! proxy. This runs in its own test binary because it sets process-wide
//! proxy environment variables.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn serve_once(listener: TcpListener, hit: Arc<AtomicBool>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            hit.store(true, Ordering::SeqCst);
            let mut buffer = [0_u8; 2048];
            let _ = stream.read(&mut buffer);
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        }
    })
}

#[test]
fn loopback_client_ignores_proxy_environment() {
    let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy_address = proxy.local_addr().unwrap();
    proxy.set_nonblocking(true).unwrap();
    for name in ["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"] {
        std::env::set_var(name, format!("http://{proxy_address}"));
    }
    for name in ["NO_PROXY", "no_proxy"] {
        std::env::remove_var(name);
    }

    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = server.local_addr().unwrap().port();
    let server_hit = Arc::new(AtomicBool::new(false));
    let handle = serve_once(server, server_hit.clone());

    let client = folio_core::generation::loopback_client(Duration::from_secs(5)).unwrap();
    let response = client
        .get(format!("http://127.0.0.1:{port}/health"))
        .bearer_auth("test-key")
        .send()
        .expect("loopback request succeeds");
    assert!(response.status().is_success());
    handle.join().unwrap();

    assert!(
        server_hit.load(Ordering::SeqCst),
        "loopback server was not reached"
    );
    assert!(
        proxy.accept().is_err(),
        "the request was sent to the configured proxy"
    );
}
