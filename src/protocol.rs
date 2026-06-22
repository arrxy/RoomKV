use std::io;
use std::net::TcpStream;

pub trait CommandProcessor {
    fn process(&self, data: &[u8], client_stream: &mut TcpStream) -> io::Result<()>;
}
