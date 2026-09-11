use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::time::Duration;
fn main() {
    for peer_shutdown in [false, true] {
        for configure_before in [true, false] {
            let (mut server, mut client) = UnixStream::pair().unwrap();
            if configure_before { client.set_read_timeout(Some(Duration::from_secs(3))).unwrap(); }
            server.write_all(b"partial failure frame").unwrap();
            if peer_shutdown { server.shutdown(Shutdown::Both).unwrap(); }
            drop(server);
            let timeout = if configure_before { Ok(()) } else { client.set_read_timeout(Some(Duration::from_secs(3))) };
            let mut data = Vec::new();
            let read = client.read_to_end(&mut data);
            println!("peer_shutdown={peer_shutdown} configure_before={configure_before} timeout={timeout:?} read={read:?} data={:?}", String::from_utf8_lossy(&data));
            assert_eq!(data, b"partial failure frame");
            assert_eq!(read.unwrap(), 21);
        }
    }
}
