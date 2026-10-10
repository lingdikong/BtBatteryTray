# bt-battery-tray

Windows 托盘小工具，显示所有已连接蓝牙设备的电量。每台能读到电量的设备各占一个托盘图标（仿 iPhone 电池小组件）：外圈圆环表示电量，圈内充电时显示闪电，否则显示设备类型（键盘 / 鼠标 / 耳机 / 手柄 / 其他）。悬停显示设备名和百分比。

不依赖任何厂商驱动或后台程序，单个 exe 直接运行。

## 电量来源

| 设备类型 | 方式 |
|---|---|
| BLE 设备（如 Logitech MX Anywhere 3）、HFP 耳机 | Windows 自带的 `DEVPKEY_Bluetooth_Battery` 设备属性 |
| 荣源方案键盘（VID `0x3151`，如腹灵 NX87、Akko、MonsGeek、Epomaker…） | 厂商 HID 通道发送只读命令 `GET_BATTERY (0x83)` |

只有系统读不到电量的设备，才会去尝试厂商协议。

### 荣源协议

厂商集合：UsagePage `0xFF66`（经典蓝牙）/ `0xFF55`（BLE），Usage `0x0202`，Report ID 6，报告长度 66 字节。

```text
请求: 06 55 83 00 00 00 00 00 00 7C ...    最后一个有效字节是校验和 0xFF - sum(cmd..+6)
应答: 06 55 83 28 01 ...                   0x28 = 40%，01 = 充电中（0 未充电 / 2 已充满）
```

参考：[monsgeek-akko-linux](https://github.com/echtzeit-solutions/monsgeek-akko-linux)、[rongyuan-kb-software](https://github.com/RieGan/rongyuan-kb-software)。

## 使用

```sh
cargo build --release
target/release/bt-battery-tray.exe            # 启动托盘
target/release/bt-battery-tray.exe --print | cat   # 打印一次电量后退出（诊断用）
```

左键单击图标立即刷新；右键菜单：设备列表、立即刷新、开机自启（写入 `HKCU\...\Run`）、退出。默认每 10 分钟自动刷新一次。

设备类型取自 `DEVPKEY_DeviceContainer_Category`（如 `Input.Keyboard`），取不到时退回蓝牙 Class of Device。图标颜色跟随任务栏深浅色主题。

## 打包安装程序

需要 [Inno Setup 6.5+](https://jrsoftware.org/isinfo.php)（`winget install JRSoftware.InnoSetup`）。

```powershell
scripts\build-installer.ps1      # 输出 target\installer\BtBatteryTray-Setup-<版本>.exe
```

安装包默认装到当前用户目录（无需管理员），可选开机自启和桌面快捷方式；升级或卸载时会先结束正在运行的托盘程序，卸载会清理自启项。

- `installer/setup.iss`：安装脚本；`installer/ChineseSimplified.isl`：简体中文语言文件（来自 Inno Setup 官方仓库）
- `assets/app.ico`：程序图标，由 `src/icon.rs` 绘制生成：
  `APP_ICON_DIR=<临时目录> cargo test app_icon` 后 `python scripts/make_ico.py <临时目录> assets/app.ico`

