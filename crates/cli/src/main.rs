use std::io;

fn main() {
    let mut output = io::stdout().lock();
    if let Err(error) = gameforge_cli::run(std::env::args().skip(1), &mut output) {
        eprintln!("error: {error}");
        std::process::exit(2);
    }
}
