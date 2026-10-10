; 蓝牙电量托盘 安装包脚本（Inno Setup 6.5+）
; 构建：先 cargo build --release，再运行 scripts\build-installer.ps1

#define AppExe "bt-battery-tray.exe"
#define AppExePath AddBackslash(SourcePath) + "..\target\release\" + AppExe
#define AppVersion GetStringFileInfo(AppExePath, "ProductVersion")
#define RunValueName "BtBatteryTray"

[Setup]
AppId={{6F1C2B7E-3D4A-4E8B-9C21-5A7D0E3F9B42}
AppName=蓝牙电量
AppVersion={#AppVersion}
AppVerName=蓝牙电量 {#AppVersion}
AppPublisher=bt-battery-tray
DefaultDirName={autopf}\BtBatteryTray
DefaultGroupName=蓝牙电量
DisableProgramGroupPage=yes
; 装在当前用户目录下，不需要管理员权限；如需给所有用户安装可在向导里切换
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\target\installer
OutputBaseFilename=BtBatteryTray-Setup-{#AppVersion}
SetupIconFile=..\assets\app.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName=蓝牙电量
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "chinesesimp"; MessagesFile: "ChineseSimplified.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
chinesesimp.AutoStart=开机自动启动
english.AutoStart=Start automatically when I sign in
chinesesimp.LaunchApp=立即启动蓝牙电量
english.LaunchApp=Launch Bluetooth Battery now

[Tasks]
Name: "autostart"; Description: "{cm:AutoStart}"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#AppExePath}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\蓝牙电量"; Filename: "{app}\{#AppExe}"
Name: "{group}\{cm:UninstallProgram,蓝牙电量}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\蓝牙电量"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
; 与程序内「开机自启」菜单使用同一个值（带引号的 exe 路径），两边状态一致
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "{#RunValueName}"; ValueData: """{app}\{#AppExe}"""; Flags: uninsdeletevalue; Tasks: autostart

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchApp}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM {#AppExe}"; Flags: runhidden; RunOnceId: "KillTray"

[Code]
procedure KillRunningApp();
var
  ResultCode: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM {#AppExe}', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

// 升级覆盖安装前先结束正在运行的托盘程序，避免文件被占用
function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  KillRunningApp();
  Result := '';
end;

// 自启项也可能是程序菜单里勾选写入的（安装时未勾选任务），卸载时一并清理
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', '{#RunValueName}');
end;
