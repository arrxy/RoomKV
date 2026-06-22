use std::{io::Write, net::TcpStream};

use crate::core::{
    cmd::RedisCommand,
    resp::{Value, encode},
};

pub fn eval_and_respond(
    cmd: &RedisCommand,
    client_stream: &mut TcpStream,
) -> Result<(), std::io::Error> {
    match cmd.cmd.as_str() {
        "PING" => eval_ping(&cmd.args, client_stream),
        "ECHO" => Ok(()),
        "SET" => Ok(()),
        "GET" => Ok(()),
        "DEL" => Ok(()),
        "EXPIRE" => Ok(()),
        _ => eval_ping(&cmd.args, client_stream),
    }
}

fn eval_ping(args: &[String], client_stream: &mut TcpStream) -> Result<(), std::io::Error> {
    if args.len() >= 2 {
        let encoded = encode(&Value::Error(
            "ERR wrong number of arguments for 'ping' command".to_string(),
        ))?;
        client_stream.write_all(&encoded)?;
        return Ok(());
    }
    let response = if args.is_empty() {
        Value::SimpleString("PONG".to_string())
    } else {
        Value::BulkString(args[0].clone().into_bytes())
    };
    client_stream.write_all(&encode(&response)?)?;
    Ok(())
}
