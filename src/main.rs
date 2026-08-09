mod app;
mod config;
mod discovery;
mod platform;
mod transports;

fn main() {
    if let Err(error) = app::run() {
        eprintln!("BarePulse failed: {error}");
        std::process::exit(1);
    }
}
