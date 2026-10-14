fn main() {
    // Embed the app icon into the .exe (only when building *for* Windows).
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/iina-rs.ico");
        res.set("FileDescription", "IINA-RS media player");
        res.set("ProductName", "IINA-RS");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed Windows icon: {e}");
        }
    }
}
