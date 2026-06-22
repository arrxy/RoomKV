use crate::config::config::Config;
use crate::core::cmd::RedisCommand;
use crate::core::eval::eval_and_respond;
use crate::core::resp;
use pollio::{EventKind, EventObject, OsPoller, Poller};
use crate::rk_info;

use std::collections::HashMap;
use std::io::{ErrorKind, prelude::*};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::unix::io::{AsRawFd, RawFd};

fn read_client_command(client_stream: &mut TcpStream) -> Result<RedisCommand, std::io::Error> {
    let mut buffer: [u8; 16384] = [0; 16384];
    let n: usize = client_stream.read(&mut buffer)?;

    if n == 0 {
        return Err(std::io::Error::new(
            ErrorKind::UnexpectedEof,
            "Client closed connection",
        ));
    }

    let tokens = resp::decode_array_string(&buffer[..n])?;

    if tokens.is_empty() {
        return Err(std::io::Error::new(
            ErrorKind::InvalidData,
            "Empty Redis command",
        ));
    }

    let mut tokens = tokens.into_iter();
    let mut cmd_name = tokens.next().expect("checked non-empty above");
    cmd_name.make_ascii_uppercase();
    let args: Vec<String> = tokens.collect();
    let cmd = RedisCommand::new(cmd_name, args);

    Ok(cmd)
}

fn respond_to_client(
    cmd: &RedisCommand,
    client_stream: &mut TcpStream,
) -> Result<(), std::io::Error> {

    match eval_and_respond(cmd, client_stream) {
        Ok(_) => Ok(()),

        Err(e) => {
            rk_info!("[RESPOND] eval error: {}", e);
            client_stream.write_all(e.to_string().as_bytes())?;
            client_stream.flush()?;
            Ok(())
        }
    }
}

fn close_client(
    poller: &OsPoller,
    connections: &mut HashMap<RawFd, TcpStream>,
    fd: RawFd,
    con_clients: &mut u64,
) {
    if let Err(e) = poller.delete(fd) {
        rk_info!("[CLOSE] failed to delete fd {} from poller: {}", fd, e);
    }

    if let Some(stream) = connections.remove(&fd) {
        let _ = stream.shutdown(Shutdown::Both);
    }

    *con_clients = con_clients.saturating_sub(1);
}

pub fn run_async_tcp_server() -> Result<(), std::io::Error> {
    let config: Config = Config::new();
    let address = format!("{}:{}", config.get_host(), config.get_port());

    rk_info!("[BOOT] server starting");
    rk_info!("[BOOT] configured address = {}", address);

    let listener: TcpListener = TcpListener::bind(&address)?;

    rk_info!("[BOOT] listener bound successfully");
    rk_info!("[BOOT] actual local addr = {}", listener.local_addr()?);

    listener.set_nonblocking(true)?;

    rk_info!("[BOOT] listener set to non-blocking");

    let listener_fd = listener.as_raw_fd();

    rk_info!("[BOOT] listener fd = {}", listener_fd);

    let mut poller = OsPoller::new()?;

    rk_info!("[BOOT] poller created");

    poller.add(EventObject::server(listener_fd))?;

    rk_info!("[BOOT] listener fd registered with poller");

    let mut connections: HashMap<RawFd, TcpStream> = HashMap::new();
    let mut con_clients: u64 = 0;
    let mut events_buf: Vec<EventObject> = Vec::new();

    rk_info!("[BOOT] server ready; waiting for events");

    loop {
        match poller.wait(-1) {
            Ok(events) => {
                events_buf.clear();
                events_buf.extend_from_slice(events);
            }

            Err(e) if e.kind() == ErrorKind::Interrupted => {
                continue;
            }

            Err(_e) => {
                continue;
            }
        };

        for event in &events_buf {
            match event.kind {
                EventKind::Server => {
                    loop {
                        match listener.accept() {
                            Ok((stream, _address)) => {
                                stream.set_nonblocking(true)?;
                                stream.set_nodelay(true)?;
                                let client_fd = stream.as_raw_fd();
                                poller.add(EventObject::client(client_fd))?;
                                connections.insert(client_fd, stream);
                                con_clients += 1;
                            }

                            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                                break;
                            }

                            Err(e) => {
                                rk_info!(
                                    "[ACCEPT] error accepting connection: kind={:?}, err={}",
                                    e.kind(),
                                    e
                                );
                                break;
                            }
                        }
                    }
                }

                EventKind::Client => {
                    let fd = event.fd;
                    let Some(client_stream) = connections.get_mut(&fd) else {
                        continue;
                    };

                    let cmd = match read_client_command(client_stream) {
                        Ok(cmd) => {
                            cmd
                        }

                        Err(e) if e.kind() == ErrorKind::WouldBlock => {
                            continue;
                        }

                        Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
                            close_client(&poller, &mut connections, fd, &mut con_clients);
                            continue;
                        }

                        Err(e) => {
                            rk_info!(
                                "[CLIENT] error reading from fd {}: kind={:?}, err={}",
                                fd,
                                e.kind(),
                                e
                            );
                            close_client(&poller, &mut connections, fd, &mut con_clients);
                            continue;
                        }
                    };

                    if let Err(_e) = respond_to_client(&cmd, client_stream) {
                        close_client(&poller, &mut connections, fd, &mut con_clients);
                    }
                }
            }
        }
    }
}
