#[derive(Debug)]
pub struct RedisCommand {
    pub cmd: String,
    pub args: Vec<String>,
}

impl RedisCommand {
    pub fn new(cmd: String, args: Vec<String>) -> Self {
        Self { cmd, args }
    }
}
