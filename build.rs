// 把程序图标和版本信息嵌入 exe（资源管理器、安装包、任务管理器里显示）
fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/app.ico")
            .set("ProductName", "蓝牙电量")
            .set("FileDescription", "蓝牙设备电量托盘")
            .set("LegalCopyright", "");
        res.compile().expect("编译 Windows 资源失败");
    }
}
