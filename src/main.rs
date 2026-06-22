mod config;
mod core;
mod logger;
mod protocol;
mod server;

use core::processor::RespCommandProcessor;
use server::server::Server;

fn main() {
    let _log_guard = logger::init_logger();
    rk_info!("Starting RoomKV server");
    Server::new(RespCommandProcessor).run().unwrap();
    rk_info!("RoomKV server stopped");
}
