use crate::config::config::Config;
use crate::core::cmd::RedisCommand;
use crate::core::eval::eval_and_respond;
use crate::core::resp;
use crate::poller::{EventKind, EventObject, OsPoller, Poller};
use crate::rk_info;

use std::collections::HashMap;
use std::io::{ErrorKind, prelude::*};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::unix::io::{AsRawFd, RawFd};

fn read_client_command(client_stream: &mut TcpStream) -> Result<RedisCommand, std::io::Error> {
    let mut buffer: [u8; 1024] = [0; 1024];
    rk_info!("[READ] trying to read from client");
    let n: usize = match client_stream.read(&mut buffer) {
        Ok(n) => n,
        Err(e) => {
            rk_info!("[READ] read error: kind={:?}, err={}", e.kind(), e);
            return Err(e);
        }
    };

    rk_info!("[READ] read {} bytes", n);

    if n == 0 {
        rk_info!("[READ] client closed connection");
        return Err(std::io::Error::new(
            ErrorKind::UnexpectedEof,
            "Client closed connection",
        ));
    }

    rk_info!("[READ] raw bytes: {:?}", &buffer[..n]);
    rk_info!(
        "[READ] raw text:\n{}",
        String::from_utf8_lossy(&buffer[..n])
    );

    let tokens = match resp::decode_array_string(&buffer[..n]) {
        Ok(tokens) => {
            rk_info!("[DECODE] tokens = {:?}", tokens);
            tokens
        }
        Err(e) => {
            rk_info!("[DECODE] decode error: kind={:?}, err={}", e.kind(), e);
            return Err(e);
        }
    };

    if tokens.is_empty() {
        rk_info!("[DECODE] empty command");
        return Err(std::io::Error::new(
            ErrorKind::InvalidData,
            "Empty Redis command",
        ));
    }

    let cmd = RedisCommand::new(tokens[0].clone().to_uppercase(), tokens[1..].to_vec());

    rk_info!("[CMD] parsed command = {:?}", cmd);

    Ok(cmd)
}

fn respond_to_client(
    cmd: &RedisCommand,
    client_stream: &mut TcpStream,
) -> Result<(), std::io::Error> {
    rk_info!("[RESPOND] evaluating command: {:?}", cmd);

    match eval_and_respond(cmd, client_stream) {
        Ok(_) => {
            rk_info!("[RESPOND] response written successfully");
            Ok(())
        }

        Err(e) => {
            rk_info!("[RESPOND] eval error: {}", e);

            let err_response = e.to_string();

            rk_info!("[RESPOND] writing error response: {:?}", err_response);

            client_stream.write_all(err_response.as_bytes())?;
            client_stream.flush()?;

            rk_info!("[RESPOND] error response written");

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
    rk_info!("[CLOSE] closing client fd={}", fd);

    if let Err(e) = poller.delete(fd) {
        rk_info!("[CLOSE] failed to delete fd {} from poller: {}", fd, e);
    } else {
        rk_info!("[CLOSE] deleted fd {} from poller", fd);
    }

    if let Some(stream) = connections.remove(&fd) {
        rk_info!("[CLOSE] removed fd {} from connections map", fd);

        if let Err(e) = stream.shutdown(Shutdown::Both) {
            rk_info!("[CLOSE] shutdown failed/ignored for fd {}: {}", fd, e);
        } else {
            rk_info!("[CLOSE] shutdown successful for fd {}", fd);
        }
    } else {
        rk_info!("[CLOSE] fd {} not found in connections map", fd);
    }

    *con_clients = con_clients.saturating_sub(1);

    rk_info!(
        "[CLOSE] closed client fd={}, concurrent connections={}",
        fd,
        con_clients
    );
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

    rk_info!("[BOOT] server ready; waiting for events");

    loop {
        rk_info!("[WAIT] waiting for poller events");

        let events = match poller.wait(-1) {
            Ok(events) => {
                rk_info!("[WAIT] poller returned {} event(s)", events.len());
                events
            }

            Err(e) if e.kind() == ErrorKind::Interrupted => {
                rk_info!("[WAIT] interrupted, continuing");
                continue;
            }

            Err(e) => {
                rk_info!("[WAIT] poller wait error: kind={:?}, err={}", e.kind(), e);
                continue;
            }
        };

        for event in events {
            rk_info!(
                "[EVENT] received event: fd={}, kind={:?}",
                event.fd,
                event.kind
            );

            match event.kind {
                EventKind::Server => {
                    rk_info!("[ACCEPT] server socket readable; accepting clients");

                    loop {
                        match listener.accept() {
                            Ok((stream, address)) => {
                                rk_info!("[ACCEPT] accepted connection from {}", address);
                                stream.set_nonblocking(true)?;
                                rk_info!("[ACCEPT] client stream set to non-blocking");
                                let client_fd = stream.as_raw_fd();
                                rk_info!("[ACCEPT] client fd = {}", client_fd);
                                poller.add(EventObject::client(client_fd))?;
                                rk_info!("[ACCEPT] client fd {} registered with poller", client_fd);
                                connections.insert(client_fd, stream);
                                rk_info!(
                                    "[ACCEPT] client fd {} inserted into connections map",
                                    client_fd
                                );
                                con_clients += 1;
                                rk_info!("[ACCEPT] concurrent connections = {}", con_clients);
                            }

                            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                                rk_info!("[ACCEPT] no more clients to accept right now");
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
                    rk_info!("[CLIENT] client socket readable: fd={}", fd);
                    let Some(client_stream) = connections.get_mut(&fd) else {
                        rk_info!("[CLIENT] got event for unknown fd {}", fd);
                        continue;
                    };

                    rk_info!("[CLIENT] found fd {} in connections map", fd);

                    let cmd = match read_client_command(client_stream) {
                        Ok(cmd) => {
                            rk_info!("[CLIENT] command read successfully from fd {}", fd);
                            cmd
                        }

                        Err(e) if e.kind() == ErrorKind::WouldBlock => {
                            rk_info!("[CLIENT] fd {} would block; continuing", fd);
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

                    rk_info!("[CLIENT] responding to fd {}", fd);

                    if let Err(e) = respond_to_client(&cmd, client_stream) {
                        rk_info!(
                            "[CLIENT] error responding to fd {}: kind={:?}, err={}",
                            fd,
                            e.kind(),
                            e
                        );
                        close_client(&poller, &mut connections, fd, &mut con_clients);
                    } else {
                        rk_info!("[CLIENT] done handling fd {}", fd);
                    }
                }
            }
        }
    }
}
