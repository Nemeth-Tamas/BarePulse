mod platform;

fn main() {
    if let Err(error) = platform::windows::run() {
        eprintln!("BarePulse failed: {error}");
        std::process::exit(1);
    }
}
