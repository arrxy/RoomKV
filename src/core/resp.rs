use std::fmt;
use std::io::{Error, ErrorKind, Write};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    SimpleString(String),
    Error(String),
    Integer(i64),
    BulkString(Vec<u8>),
    Array(Vec<Value>),
    Null,
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::SimpleString(s) => write!(f, "{}", s),
            Value::Error(s) => write!(f, "{}", s),
            Value::Integer(i) => write!(f, "{}", i),
            Value::BulkString(bytes) => write!(f, "{}", String::from_utf8_lossy(bytes)),
            Value::Array(values) => write!(f, "{:?}", values),
            Value::Null => write!(f, "null"),
        }
    }
}

impl Default for Value {
    fn default() -> Self {
        Value::Null
    }
}

pub fn decode(data: &[u8]) -> Result<Value, Error> {
    if data.is_empty() {
        return Err(Error::new(ErrorKind::InvalidData, "Empty data"));
    }
    decode_one(data)
        .map(|(value, _)| value)
        .map_err(|e| Error::new(ErrorKind::InvalidData, e))
}

fn decode_one(data: &[u8]) -> Result<(Value, usize), Error> {
    if data.is_empty() {
        return Err(Error::new(ErrorKind::InvalidData, "Empty data"));
    }
    let (value, bytes_read) = match data[0] {
        b'+' => decode_simple_string(data),
        b'-' => decode_error(data),
        b':' => decode_integer(data),
        b'$' => decode_bulk_string(data),
        b'*' => decode_array(data),
        _ => return Err(Error::new(ErrorKind::InvalidData, "Invalid data")),
    }?;
    Ok((value, bytes_read))
}

fn decode_simple_string(data: &[u8]) -> Result<(Value, usize), Error> {
    let mut pos = 1;
    while pos < data.len() && data[pos] != b'\r' {
        pos += 1;
    }
    if pos >= data.len() {
        return Err(Error::new(ErrorKind::InvalidData, "Invalid data"));
    }
    Ok((
        Value::SimpleString(String::from_utf8_lossy(&data[1..pos]).to_string()),
        pos + 2,
    ))
}

fn decode_error(data: &[u8]) -> Result<(Value, usize), Error> {
    let (value, bytes_read) = decode_simple_string(data)?;
    Ok((Value::Error(value.to_string()), bytes_read))
}

fn decode_integer(data: &[u8]) -> Result<(Value, usize), Error> {
    let mut pos: usize = 1;
    while pos < data.len() && data[pos] != b'\r' {
        pos += 1;
    }
    if pos >= data.len() {
        return Err(Error::new(ErrorKind::InvalidData, "Invalid data"));
    }
    let value: i64 = String::from_utf8_lossy(&data[1..pos]).parse().unwrap();
    Ok((Value::Integer(value), pos + 2))
}

fn decode_bulk_string(data: &[u8]) -> Result<(Value, usize), Error> {
    let mut pos: usize = 1;
    let (len, delta) = read_len(&data[1..])?;
    pos += delta;
    if pos + len + 2 > data.len() {
        return Err(Error::new(ErrorKind::InvalidData, "Invalid data"));
    }
    let value = Value::BulkString(data[pos..pos + len].to_vec());
    Ok((value, pos + len + 2))
}

fn decode_array(data: &[u8]) -> Result<(Value, usize), Error> {
    let mut pos: usize = 1;
    let (len, delta) = read_len(&data[1..])?;
    pos += delta;
    let mut values: Vec<Value> = Vec::with_capacity(len);
    for _ in 0..len {
        let (value, delta) = decode_one(&data[pos..])?;
        values.push(value);
        pos += delta;
    }
    Ok((Value::Array(values), pos))
}

fn value_into_string(value: Value) -> String {
    match value {
        Value::BulkString(bytes) => String::from_utf8(bytes)
            .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()),
        other => other.to_string(),
    }
}

pub fn decode_array_string(data: &[u8]) -> Result<Vec<String>, Error> {
    let value = decode(data)?;
    match value {
        Value::Array(values) => Ok(values.into_iter().map(value_into_string).collect()),
        _ => Err(Error::new(ErrorKind::InvalidData, "Invalid data")),
    }
}

fn read_len(data: &[u8]) -> Result<(usize, usize), Error> {
    let mut pos: usize = 0;
    let mut len: usize = 0;
    while pos < data.len() {
        let b = data[pos];
        if !(b >= b'0' && b <= b'9') {
            return Ok((len, pos + 2));
        }
        len = len * 10 + (b - b'0') as usize;
        pos += 1;
    }
    Ok((0, 0))
}

pub fn encode(value: &Value) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    encode_into(value, &mut out);
    Ok(out)
}

fn encode_into(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::SimpleString(s) => {
            out.push(b'+');
            out.extend_from_slice(s.as_bytes());
            out.extend_from_slice(b"\r\n");
        }
        Value::Error(s) => {
            out.push(b'-');
            out.extend_from_slice(s.as_bytes());
            out.extend_from_slice(b"\r\n");
        }
        Value::Integer(i) => {
            out.push(b':');
            write!(out, "{}", i).unwrap();
            out.extend_from_slice(b"\r\n");
        }
        Value::BulkString(bytes) => {
            out.push(b'$');
            write!(out, "{}", bytes.len()).unwrap();
            out.extend_from_slice(b"\r\n");
            out.extend_from_slice(bytes);
            out.extend_from_slice(b"\r\n");
        }
        Value::Array(values) => {
            out.push(b'*');
            write!(out, "{}", values.len()).unwrap();
            out.extend_from_slice(b"\r\n");
            for v in values {
                encode_into(v, out);
            }
        }
        Value::Null => out.extend_from_slice(b"$-1\r\n"),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn test_decode_simple_string() {
        println!("test_decode_simple_string");
        let mut map: HashMap<&str, Value> = HashMap::new();
        map.insert("+OK\r\n", Value::SimpleString("OK".to_string()));
        map.insert("+Hello\r\n", Value::SimpleString("Hello".to_string()));
        map.insert("+World\r\n", Value::SimpleString("World".to_string()));
        map.insert(
            "+Hello World\r\n",
            Value::SimpleString("Hello World".to_string()),
        );
        map.insert(
            "+Hello World\r\n",
            Value::SimpleString("Hello World".to_string()),
        );

        for (data, expected_value) in map {
            let (value, bytes_read) = decode_simple_string(data.as_bytes()).unwrap();
            println!("value: {:?}, bytes_read: {}", value, bytes_read);
            assert_eq!(value, expected_value);
        }
    }

    #[test]
    fn test_decode_error() {
        println!("test_decode_error");
        let mut map: HashMap<&str, Value> = HashMap::new();
        map.insert(
            "-ERR error message\r\n",
            Value::Error("ERR error message".to_string()),
        );
        map.insert("-Error\r\n", Value::Error("Error".to_string()));
        map.insert("-World\r\n", Value::Error("World".to_string()));
        map.insert("-Hello World\r\n", Value::Error("Hello World".to_string()));
        map.insert("-Hello World\r\n", Value::Error("Hello World".to_string()));

        for (data, expected_value) in map {
            let (value, bytes_read) = decode_error(data.as_bytes()).unwrap();
            println!("value: {:?}, bytes_read: {}", value, bytes_read);
            assert_eq!(value, expected_value);
        }
    }

    #[test]
    fn test_decode_integer() {
        println!("test_decode_integer");
        let mut map: HashMap<&str, Value> = HashMap::new();
        map.insert(":123\r\n", Value::Integer(123));
        map.insert(":1234567890\r\n", Value::Integer(1234567890));
        map.insert(":0\r\n", Value::Integer(0));
        map.insert(":-123\r\n", Value::Integer(-123));
        map.insert(":-1234567890\r\n", Value::Integer(-1234567890));
        map.insert(":-0\r\n", Value::Integer(0));
        map.insert(":-1234567890\r\n", Value::Integer(-1234567890));

        for (data, expected_value) in map {
            let (value, bytes_read) = decode_integer(data.as_bytes()).unwrap();
            println!("value: {:?}, bytes_read: {}", value, bytes_read);
            assert_eq!(value, expected_value);
        }
    }

    #[test]
    fn test_decode_bulk_string() {
        println!("test_decode_bulk_string");
        let mut map: HashMap<&str, Value> = HashMap::new();
        map.insert(
            "$5\r\nHello\r\n",
            Value::BulkString("Hello".as_bytes().to_vec()),
        );
        map.insert(
            "$5\r\nWorld\r\n",
            Value::BulkString("World".as_bytes().to_vec()),
        );
        map.insert(
            "$11\r\nHello World\r\n",
            Value::BulkString("Hello World".as_bytes().to_vec()),
        );
        map.insert("$0\r\n\r\n", Value::BulkString(Vec::new()));
        for (data, expected_value) in map {
            let (value, bytes_read) = decode_bulk_string(data.as_bytes()).unwrap();
            println!("value: {:?}, bytes_read: {}", value, bytes_read);
            assert_eq!(value, expected_value);
        }
    }

    #[test]
    fn test_decode_array() {
        println!("test_decode_array");
        let mut map: HashMap<&str, Value> = HashMap::new();
        map.insert("*0\r\n", Value::Array(Vec::new()));
        map.insert(
            "*1\r\n$5\r\nHello\r\n",
            Value::Array(vec![Value::BulkString("Hello".as_bytes().to_vec())]),
        );
        map.insert(
            "*2\r\n$5\r\nHello\r\n$5\r\nWorld\r\n",
            Value::Array(vec![
                Value::BulkString("Hello".as_bytes().to_vec()),
                Value::BulkString("World".as_bytes().to_vec()),
            ]),
        );
        map.insert(
            "*3\r\n$5\r\nHello\r\n$5\r\nWorld\r\n$11\r\nHello World\r\n",
            Value::Array(vec![
                Value::BulkString("Hello".as_bytes().to_vec()),
                Value::BulkString("World".as_bytes().to_vec()),
                Value::BulkString("Hello World".as_bytes().to_vec()),
            ]),
        );

        for (data, expected_value) in map {
            let (value, bytes_read) = decode_array(data.as_bytes()).unwrap();
            println!("value: {:?}, bytes_read: {}", value, bytes_read);
            assert_eq!(value, expected_value);
        }
    }

    #[test]
    fn test_decode() {
        println!("test_decode");
        let mut map: HashMap<&str, Value> = HashMap::new();
        map.insert("+OK\r\n", Value::SimpleString("OK".to_string()));
        map.insert(
            "-ERR error message\r\n",
            Value::Error("ERR error message".to_string()),
        );
        map.insert(":123\r\n", Value::Integer(123));
        map.insert(
            "$5\r\nHello\r\n",
            Value::BulkString("Hello".as_bytes().to_vec()),
        );
        map.insert("*0\r\n", Value::Array(Vec::new()));
        map.insert(
            "*1\r\n$5\r\nHello\r\n",
            Value::Array(vec![Value::BulkString("Hello".as_bytes().to_vec())]),
        );
        map.insert(
            "*2\r\n$5\r\nHello\r\n$5\r\nWorld\r\n",
            Value::Array(vec![
                Value::BulkString("Hello".as_bytes().to_vec()),
                Value::BulkString("World".as_bytes().to_vec()),
            ]),
        );
        map.insert(
            "*3\r\n$5\r\nHello\r\n$5\r\nWorld\r\n$11\r\nHello World\r\n",
            Value::Array(vec![
                Value::BulkString("Hello".as_bytes().to_vec()),
                Value::BulkString("World".as_bytes().to_vec()),
                Value::BulkString("Hello World".as_bytes().to_vec()),
            ]),
        );

        for (data, expected_value) in map {
            let value = decode(data.as_bytes()).unwrap();
            println!("value: {:?}", value);
            assert_eq!(value, expected_value);
        }
    }
}
