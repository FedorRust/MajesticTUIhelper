fn main() {
    if let Err(err) = mj::ui::run() {
        eprintln!("mj: {err}");
        std::process::exit(1);
    }
}
