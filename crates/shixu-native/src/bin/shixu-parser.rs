//! Reserved worker executable. A command line must never grant parser authority.
fn main() {
    eprintln!("UNSUPPORTED");
    std::process::exit(78);
}
