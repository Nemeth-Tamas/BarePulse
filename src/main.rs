mod app;
mod config;
mod platform;

fn main() {
    if let Err(error) = app::run() {
        eprintln!("BarePulse failed: {error}");
        std::process::exit(1);
    }
}
