fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime must start");
    if let Err(error) = runtime.block_on(nodes_svc::app::bootstrap::run()) {
        eprintln!("fatal: {error}");
        std::process::exit(1);
    }
}
