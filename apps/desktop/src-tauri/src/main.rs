#![forbid(unsafe_code)]

fn main() {
    if let Err(error) = quisum_desktop::run() {
        eprintln!("Quisum desktop failed to start: {error}");
        std::process::exit(1);
    }
}
