fn main() {
    // Compile the UI, with the Windows 11 (Fluent) dark look for the Settings window
    let config = slint_build::CompilerConfiguration::new().with_style("fluent-dark".into());
    slint_build::compile_with_config("ui/app.slint", config).expect("UI failed to compile");

    // Give WinOverview.exe its icon (skipped quietly if the Windows SDK's rc.exe isn't found)
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/app.ico");
        if let Err(e) = res.compile() {
            println!("cargo:warning=No exe icon (rc.exe not found): {e}");
        }
    }
}
