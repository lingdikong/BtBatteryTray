# bt-battery-tray

[English](README.md) | 简体中文

托盘 / 菜单栏小工具，显示所有已连接蓝牙设备的电量，支持 Windows、macOS、Linux。每台已连接设备各占一个托盘图标（仿 iPhone 电池小组件）：外圈圆环表示电量，圈内充电时显示闪电，否则显示设备类型（键盘 / 鼠标 / 耳机 / 手柄 / 手机 / 其他）；读不到电量时显示虚线圈。悬停显示设备名和百分比。

不依赖任何厂商驱动或后台程序，单个可执行文件直接运行。

## 电量来源

| 平台 | 系统电量 | 连接事件 |
|---|---|---|
| Windows | 设备属性 `DEVPKEY_Bluetooth_Battery`（BLE 电池服务、HFP 耳机）；iPhone 等双模设备从同一 ContainerId 的 BLE 节点取 | WinRT DeviceWatcher |
| Linux | BlueZ `org.bluez.Battery1`；没有时用内核 `/sys/class/power_supply`（Logitech HID++ 等，还能拿到充电状态） | BlueZ D-Bus 信号 |
| macOS | `system_profiler SPBluetoothDataType`（Apple 设备、AirPods、多数 BLE 设备） | IOHIDManager（只覆盖蓝牙 HID 设备，AirPods 靠定时刷新） |

荣源方案键盘（VID `0x3151`，如腹灵 NX87、Akko、MonsGeek、Epomaker…）系统读不到电量，通过厂商 HID 通道发送只读命令 `GET_BATTERY (0x83)` 获取，三个平台都支持。只有系统读不到电量的设备，才会去尝试厂商协议。

### 荣源协议

厂商集合：UsagePage `0xFF66`（经典蓝牙）/ `0xFF55`（BLE），Usage `0x0202`，Report ID 6，报告长度 66 字节。

```text
请求: 06 55 83 00 00 00 00 00 00 7C ...    最后一个有效字节是校验和 0xFF - sum(cmd..+6)
应答: 06 55 83 28 01 ...                   0x28 = 40%，01 = 充电中（0 未充电 / 2 已充满）
```

参考：[monsgeek-akko-linux](https://github.com/echtzeit-solutions/monsgeek-akko-linux)、[rongyuan-kb-software](https://github.com/RieGan/rongyuan-kb-software)。

## 使用

```sh
target/release/bt-battery-tray                # 启动托盘（编译方法见下文“构建”）
target/release/bt-battery-tray --print        # 打印一次电量后退出（诊断用；Windows 上需重定向，如 `| cat`）
target/release/bt-battery-tray --watch        # 打印 30 秒内的蓝牙连接事件（诊断用）
target/release/bt-battery-tray --notify-test  # 弹一条示例低电量提醒，检查系统通知是否被屏蔽
```

左键单击图标立即刷新；右键菜单：设备列表、立即刷新、开机自启、退出。蓝牙设备连上或断开时立即刷新；此外每 10 分钟自动刷新一次，读不到电量时按 1 秒起的退避间隔重试；键盘休眠后一有键鼠输入就重试（Windows、macOS）。图标颜色跟随任务栏 / 菜单栏深浅色。

**低电量提醒**：设备电量降到 20% 及以下（且没在充电）时圆环变红，并弹一次系统通知；充上电或电量回到 25% 以上后才会再次提醒。Windows 用托盘气泡（Windows 10/11 显示为系统通知），Linux 用 freedesktop 通知服务，macOS 用 `osascript`（通知显示为来自“脚本编辑器”）。

**开机自启**：Windows 写 `HKCU\...\Run`，macOS 写 `~/Library/LaunchAgents/com.lingdikong.btbatterytray.plist`，Linux 写 `~/.config/autostart/bt-battery-tray.desktop`。

### 界面语言

界面跟随系统语言：内置英文和简体中文，其他语言回退到英文。可以用环境变量 `BT_BATTERY_LANG` 指定，如 `BT_BATTERY_LANG=zh-CN`。

翻译在 `locales/*.yml`，`en.yml` 是源语言。新增语言时把 `en.yml` 复制成如 `locales/ja.yml` 再翻译各项的值；`cargo test` 会检查每种语言的键和 `%{占位符}` 与 `en.yml` 完全一致。安装包和应用名单独本地化：`installer/setup.iss`（`[CustomMessages]`）、`packaging/macos/*.lproj/InfoPlist.strings`、`packaging/linux/bt-battery-tray.desktop`。

### 平台注意事项

- **Windows**：新托盘图标默认收在折叠区，拖到任务栏一次即可，之后按设备记住。
- **macOS**：读荣源键盘电量需要在“系统设置 → 隐私与安全性 → 输入监控”里允许本程序。没有开发者证书签名时，首次打开需右键 → 打开。iPhone 电量系统不对外提供。
- **Linux**：托盘使用 StatusNotifierItem（KDE、Xfce 等原生支持；GNOME 需要 AppIndicator 扩展）。读荣源键盘需要安装 udev 规则 `packaging/linux/70-bt-battery-tray.rules`（`install.sh --udev`）。Wayland 下拿不到输入空闲时间，没有“有输入就重试”。

## 代码结构

- `src/main.rs`：托盘界面、刷新线程、电量汇总（跨平台）
- `src/icon.rs`：图标绘制；`src/rongyuan.rs`：荣源协议；`src/bluetooth.rs`：设备类型等公共定义；`src/i18n.rs`：界面语言选择
- `src/platform/{windows,linux,macos}/`：各平台的蓝牙枚举（`bluetooth`）、连接事件（`devwatch`）、HID 收发（`hid`）、开机自启（`autostart`）、事件循环和通知
- `locales/`：界面翻译

## 构建

### 环境要求

| 平台 | 需要 |
|---|---|
| 全部 | Rust 1.88 或更新的稳定版，用 [rustup](https://rustup.rs) 安装 |
| Windows | Visual Studio 生成工具，勾选“使用 C++ 的桌面开发”（提供 MSVC 链接器和 Windows SDK）；rustup 安装程序会提示安装 |
| macOS | Xcode 命令行工具：`xcode-select --install` |
| Linux | 链接用的 C 工具链（Debian/Ubuntu 装 `build-essential`，Arch 装 `base-devel`），不需要 GTK 等系统库。运行时需要 BlueZ 和 D-Bus 会话 |

### 编译和运行

```sh
git clone https://github.com/lingdikong/BtBatteryTray.git
cd BtBatteryTray
cargo build --release          # 产物：target/release/bt-battery-tray（Windows 上是 .exe）
cargo run                      # 调试版，启动托盘
cargo run -- --print           # 调试版，打印一次电量
BT_BATTERY_LANG=en cargo run   # 指定界面语言（PowerShell：$env:BT_BATTERY_LANG="en"; cargo run）
```

### 测试和检查

```sh
cargo test                                  # 单元测试，包括翻译文件的键 / 占位符检查
cargo clippy --all-targets -- -D warnings   # 代码检查，和 CI 一致
```

在一台机器上就能对其他系统的平台代码做类型检查（只编译不链接）：

```sh
rustup target add x86_64-unknown-linux-gnu aarch64-apple-darwin x86_64-apple-darwin
cargo clippy --target x86_64-unknown-linux-gnu --all-targets -- -D warnings
cargo clippy --target aarch64-apple-darwin --all-targets -- -D warnings
```

### 重新生成程序图标

`assets/app.ico`、`app.icns`、`app.png` 由 `src/icon.rs` 绘制并提交在仓库里。改了绘制代码后重新生成（需要 Python 3，不需要额外的包）：

```sh
APP_ICON_DIR=/tmp/appicon cargo test app_icon        # PowerShell：$env:APP_ICON_DIR="$env:TEMP\appicon"; cargo test app_icon
python scripts/make_ico.py /tmp/appicon assets/app.ico assets/app.icns assets/app.png
```

## 打包

版本号取自 `Cargo.toml` 的 `version`，发布前先改它。每个平台的安装包要在对应系统上打。

### Windows 安装包

1. 安装 [Inno Setup 6.5+](https://jrsoftware.org/isinfo.php)：`winget install JRSoftware.InnoSetup`
2. 打包：`powershell -ExecutionPolicy Bypass -File scripts\build-installer.ps1`（在 PowerShell 里也可以直接运行 `scripts\build-installer.ps1`）
3. 输出：`target\installer\BtBatteryTray-Setup-<版本>.exe`

脚本会编译 release 版 exe 并编译 `installer/setup.iss`。安装包默认装到当前用户目录（无需管理员），可选开机自启和桌面快捷方式，按 Windows 界面语言显示中文或英文；升级或卸载时会先结束正在运行的托盘程序，卸载会清理自启项。

### macOS（.dmg）

1. 添加两种架构：`rustup target add aarch64-apple-darwin x86_64-apple-darwin`
2. 打包：`sh scripts/build-macos.sh`
3. 输出：`target/dist/BtBatteryTray-<版本>-macos.dmg`，里面是通用（Apple Silicon + Intel）的 `BtBatteryTray.app` 和“应用程序”快捷方式

签名：默认用临时签名（ad-hoc），用户首次打开需要右键 → 打开。有 Apple 开发者账号时，用证书签名后再公证：

```sh
CODESIGN_IDENTITY="Developer ID Application: Your Name (TEAMID)" sh scripts/build-macos.sh
xcrun notarytool submit target/dist/BtBatteryTray-<版本>-macos.dmg --keychain-profile <配置名> --wait
xcrun stapler staple target/dist/BtBatteryTray-<版本>-macos.dmg
```

### Linux（.tar.gz）

1. 打包：`sh scripts/build-linux.sh`（按当前机器的架构编译）
2. 输出：`target/dist/BtBatteryTray-<版本>-linux-<架构>.tar.gz`，包含程序、图标、`.desktop` 文件、udev 规则和 `install.sh`
3. 在目标机器上安装：

   ```sh
   tar xzf BtBatteryTray-<版本>-linux-x86_64.tar.gz
   cd BtBatteryTray-<版本>-linux-x86_64
   ./install.sh            # 装到 ~/.local；设置 PREFIX=... 可改位置
   ./install.sh --udev     # 另外用 sudo 安装 udev 规则（荣源方案键盘需要）
   ```

   卸载时删除 `~/.local/bin/bt-battery-tray`、`~/.local/share/applications/bt-battery-tray.desktop`、`~/.local/share/icons/hicolor/256x256/apps/bt-battery-tray.png`，装过 udev 规则的再删 `/etc/udev/rules.d/70-bt-battery-tray.rules`。

### CI 自动打包

`.github/workflows/build.yml` 在推送到 `main`、推送 `v*` 标签、提交 PR 以及手动触发时运行：在 Windows、macOS、Linux 上跑 clippy 和测试，再按上面的方式打包，安装包挂在这次运行的 Artifacts 里。发布流程：改 `Cargo.toml` 的 `version` 并提交，然后 `git tag v<版本> && git push origin v<版本>`，到这次运行里下载 Artifacts（工作流不会自动创建 GitHub Release）。

### 打包相关文件

- `installer/setup.iss`：Windows 安装脚本；`installer/ChineseSimplified.isl`：简体中文语言文件（来自 Inno Setup 官方仓库）
- `packaging/macos/`：`Info.plist` 模板和各语言应用名（`*.lproj/InfoPlist.strings`）
- `packaging/linux/`：`.desktop` 文件、udev 规则和 `install.sh`
- `scripts/`：`build-installer.ps1`、`build-macos.sh`、`build-linux.sh`、`make_ico.py`
