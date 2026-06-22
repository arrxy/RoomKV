use crate::config::config::Config;
use crate::protocol::CommandProcessor;
use pollio::{EventKind, EventObject, OsPoller, Poller};
use crate::rk_info;

use std::collections::HashMap;
use std::io::{ErrorKind, prelude::*};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::unix::io::{AsRawFd, RawFd};

pub struct Server<P: CommandProcessor> {
    listener: TcpListener,
    poller: OsPoller,
    connections: HashMap<RawFd, TcpStream>,
    con_clients: u64,
    events_buf: Vec<EventObject>,
    #[allow(dead_code)]
    config: Config,
    processor: P,
}

impl<P: CommandProcessor> Server<P> {

    pub fn new(processor: P) -> Self {
        let (listener, poller) = Self::boot_up_server().unwrap();
        Self {
            listener,
            poller,
            connections: HashMap::new(),
            con_clients: 0,
            events_buf: Vec::new(),
            config: Config::new(),
            processor,
        }
    }

    pub fn run(&mut self) -> Result<(), std::io::Error> {
        loop {
            match self.poller.wait(-1) {
                Ok(events) => {
                    self.events_buf.clear();
                    self.events_buf.extend_from_slice(events);
                }

                Err(e) if e.kind() == ErrorKind::Interrupted => {
                    continue;
                }

                Err(_e) => {
                    continue;
                }
            }

            for i in 0..self.events_buf.len() {
                let event = self.events_buf[i];
                match event.kind {
                    EventKind::Server => self.handle_server_events()?,
                    EventKind::Client => self.handle_client_events(&event)?,
                }
            }
        }
    }

    fn close_client(&mut self, fd: RawFd) {
        if let Err(e) = self.poller.delete(fd) {
            rk_info!("[CLOSE] failed to delete fd {} from poller: {}", fd, e);
        }

        if let Some(stream) = self.connections.remove(&fd) {
            let _ = stream.shutdown(Shutdown::Both);
        }

        self.con_clients = self.con_clients.saturating_sub(1);
    }

    fn boot_up_server() -> Result<(TcpListener, OsPoller), std::io::Error> {
        let config: Config = Config::new();
        let address = format!("{}:{}", config.get_host(), config.get_port());

        rk_info!("[BOOT] server starting \n address = {}", address);
        let listener: TcpListener = TcpListener::bind(&address)?;
        rk_info!(
            "[BOOT] listener bound successfully local addr = {}",
            listener.local_addr()?
        );

        listener.set_nonblocking(true)?;
        let listener_fd = listener.as_raw_fd();
        rk_info!("[BOOT] listener fd = {}", listener_fd);
        let poller = OsPoller::new()?;
        poller.add(EventObject::server(listener_fd))?;

        Ok((listener, poller))
    }

    fn handle_server_events(&mut self) -> Result<(), std::io::Error> {
        loop {
            match self.listener.accept() {
                Ok((stream, _address)) => {
                    stream.set_nonblocking(true)?;
                    stream.set_nodelay(true)?;
                    let client_fd = stream.as_raw_fd();
                    self.poller.add(EventObject::client(client_fd))?;
                    self.connections.insert(client_fd, stream);
                    self.con_clients += 1;
                }

                // If the listener is blocked, break the loop. exit #1: accept queue is empty
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    break;
                }

                // exit #2: a real accept error
                Err(e) => {
                    rk_info!(
                        "[SERVER] error accepting connection: kind={:?}, err={}",
                        e.kind(),
                        e
                    );
                    break;
                }
            }
        }
        Ok(())
    }

    fn handle_client_events(&mut self, event: &EventObject) -> Result<(), std::io::Error> {
        let fd = event.fd;

        let should_close = {
            let Some(client_stream) = self.connections.get_mut(&fd) else {
                return Ok(());
            };

            let mut buffer: [u8; 16384] = [0; 16384];
            match client_stream.read(&mut buffer) {
                Ok(0) => true,

                Ok(n) => self.processor.process(&buffer[..n], client_stream).is_err(),

                Err(e) if e.kind() == ErrorKind::WouldBlock => return Ok(()),

                Err(e) => {
                    rk_info!(
                        "[CLIENT] error reading from fd {}: kind={:?}, err={}",
                        fd,
                        e.kind(),
                        e
                    );
                    true
                }
            }
        };

        if should_close {
            self.close_client(fd);
        }
        Ok(())
    }
}
