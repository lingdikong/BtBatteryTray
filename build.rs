// Embed the app icon and version info into the exe (shown in Explorer, the installer and Task Manager).
// A version resource holds one language, so it is English; FileDescription is also the sender name of notifications.
fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    println!("cargo:rerun-if-changed=locales");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/app.ico")
            .set("ProductName", "Bluetooth Battery")
            .set("FileDescription", "Bluetooth Battery")
            .set("LegalCopyright", "");
        res.compile().expect("failed to compile Windows resources");
    }
}
