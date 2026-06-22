mod config;
mod core;
mod logger;
mod server;
fn main() {
    let _log_guard = logger::init_logger();
    rk_info!("Starting RoomKV server");
    server::async_server::run_async_tcp_server().unwrap();
    rk_info!("RoomKV server stopped");
}
