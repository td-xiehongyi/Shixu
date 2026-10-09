#[cfg(windows)]
fn main() {
    shixu_desktop::runtime::run();
}
#[cfg(not(windows))]
fn main() {
    eprintln!("UNSUPPORTED: Windows desktop runtime required");
    std::process::exit(1);
}
