#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod devices;
mod discovery;
mod platform;
mod protocols;
mod transports;

fn main() {
    if let Err(error) = app::run() {
        eprintln!("BarePulse failed: {error}");
        std::process::exit(1);
    }
}
