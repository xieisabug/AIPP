//! Helpers for tests that stand up a one-shot HTTP mock server on a raw TCP socket.

use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

/// Read one complete HTTP request: headers plus a `Content-Length` body.
///
/// Mock servers must drain the body before replying with `Connection: close`. Closing a socket
/// that still holds unread request bytes makes Windows send RST, so the client intermittently
/// fails while reading a perfectly valid response.
pub(crate) async fn read_request(socket: &mut TcpStream) -> (String, Vec<u8>) {
    let mut data = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
        let n = socket.read(&mut chunk).await.expect("read request head");
        assert!(n > 0, "client closed before sending the request head");
        data.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&data[..header_end - 4]).to_string();
    let length = head
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while data.len() < header_end + length {
        let n = socket.read(&mut chunk).await.expect("read request body");
        assert!(n > 0, "client closed before sending the full request body");
        data.extend_from_slice(&chunk[..n]);
    }
    (head, data[header_end..header_end + length].to_vec())
}
