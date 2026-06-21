use crate::config::config::Config;
use crate::core::cmd::RedisCommand;
use crate::core::eval::eval_and_respond;
use crate::core::resp;
use crate::rk_info;
use std::io::{ErrorKind, prelude::*};
use std::net::{TcpListener, TcpStream};

fn read_client_command(client_stream: &mut TcpStream) -> Result<RedisCommand, std::io::Error> {
    let mut buffer: [u8; 1024] = [0; 1024];
    let n: usize = match client_stream.read(&mut buffer) {
        Ok(n) => n,
        Err(e) => return Err(e),
    };
    if n == 0 {
        return Err(std::io::Error::new(
            ErrorKind::UnexpectedEof,
            "Client closed connection",
        ));
    }
    let tokens = resp::decode_array_string(&buffer[..n])?;
    Ok(RedisCommand::new(
        tokens[0].clone().to_uppercase(),
        tokens[1..].to_vec(),
    ))
}

#[allow(dead_code)]
pub fn run_sync_tcp_server() {
    let config: Config = Config::new();
    let mut con_clients: u64 = 0;
    rk_info!(
        "Server running on {}:{}",
        config.get_host(),
        config.get_port()
    );
    let listener: TcpListener =
        TcpListener::bind(format!("{}:{}", config.get_host(), config.get_port()))
            .expect("Failed to bind to address");
    loop {
        let mut client_stream: TcpStream = match listener.accept() {
            Ok((stream, address)) => {
                con_clients += 1;
                rk_info!(
                    "New connection from {}:{}, concurrent connections: {}",
                    address.ip(),
                    address.port(),
                    con_clients
                );
                stream
            }
            Err(e) => {
                rk_info!("Error accepting connection: {}", e);
                break;
            }
        };
        loop {
            let cmd = match read_client_command(&mut client_stream) {
                Ok(cmd) => cmd,
                Err(e) => {
                    con_clients -= 1;
                    if let Err(e) = client_stream.shutdown(std::net::Shutdown::Both) {
                        rk_info!("Shutdown failed/ignored: {}", e);
                    }
                    rk_info!(
                        "Error reading from client: {}, concurrent connections: {}",
                        e,
                        con_clients
                    );
                    break;
                }
            };
            match respond_to_client(&cmd, &mut client_stream) {
                Ok(_) => {}
                Err(e) => {
                    rk_info!("Error responding to client: {}", e);
                    con_clients -= 1;
                    client_stream.shutdown(std::net::Shutdown::Both).unwrap();
                    rk_info!(
                        "Error responding to client: {}, closed connection, concurrent connections: {}",
                        e,
                        con_clients
                    );
                    rk_info!("Client closed connection");
                    break;
                }
            }
        }
    }
}

fn respond_to_client(
    cmd: &RedisCommand,
    client_stream: &mut TcpStream,
) -> Result<(), std::io::Error> {
    let response = match eval_and_respond(cmd, client_stream) {
        Ok(_) => {}
        Err(e) => {
            client_stream.write_all(e.to_string().as_bytes()).unwrap();
        }
    };
    Ok(response)
}
