fn main() {
    if let Err(err) = cargo_hoist::run() {
        eprintln!("Error: {err:?}");
        std::process::exit(1);
    }
}
