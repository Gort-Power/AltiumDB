fn main() {
    if std::env::var("CARGO_CFG_WINDOWS").is_ok() {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("icon.ico");
        match res.compile() {
            Ok(()) => {}
            Err(e) => {
                println!("cargo:warning=Failed to embed icon: {e}");
            }
        }
    }
}
