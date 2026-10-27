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
    // Test builds only: IINA_RS_DGPU_TEST=1 exports NvOptimusEnablement /
    // AmdPowerXpressRequestHighPerformance so hybrid laptops run it on the
    // discrete GPU (to test drivers other than the integrated one).
    println!("cargo:rerun-if-env-changed=IINA_RS_DGPU_TEST");
    if std::env::var_os("IINA_RS_DGPU_TEST").is_some() && std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-cfg=dgpu_test");
        println!("cargo:rustc-link-arg-bins=/EXPORT:NvOptimusEnablement");
        println!("cargo:rustc-link-arg-bins=/EXPORT:AmdPowerXpressRequestHighPerformance");
    }
    println!("cargo:rustc-check-cfg=cfg(dgpu_test)");
}
