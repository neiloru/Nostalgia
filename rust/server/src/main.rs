#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("nostalgia-server {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let file = nostalgia_server::config::resolve_settings_file(&args);
    if let Err(e) = nostalgia_server::run(&file).await {
        nostalgia_server::log::error(format!("{e:#}"));
        std::process::exit(1);
    }
}
