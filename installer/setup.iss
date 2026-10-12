; Bluetooth Battery tray installer script (Inno Setup 6.5+)
; Build: cargo build --release, then run scripts\build-installer.ps1

#define AppExe "bt-battery-tray.exe"
#define AppExePath AddBackslash(SourcePath) + "..\target\release\" + AppExe
#define AppVersion GetStringFileInfo(AppExePath, "ProductVersion")
#define RunValueName "BtBatteryTray"

[Setup]
AppId={{6F1C2B7E-3D4A-4E8B-9C21-5A7D0E3F9B42}
AppName={cm:AppName}
AppVersion={#AppVersion}
AppVerName={cm:AppName} {#AppVersion}
AppPublisher=bt-battery-tray
DefaultDirName={autopf}\BtBatteryTray
DefaultGroupName={cm:AppName}
DisableProgramGroupPage=yes
; Installs per user without admin rights; the wizard lets the user switch to an all-users install
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\target\installer
OutputBaseFilename=BtBatteryTray-Setup-{#AppVersion}
SetupIconFile=..\assets\app.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={cm:AppName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; Pick the language from the Windows UI language without asking; unmatched languages get the first entry (English)
ShowLanguageDialog=no
LanguageDetectionMethod=uilanguage

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "chinesesimp"; MessagesFile: "ChineseSimplified.isl"

[CustomMessages]
english.AppName=Bluetooth Battery
chinesesimp.AppName=蓝牙电量
english.AutoStart=Start automatically when I sign in
chinesesimp.AutoStart=开机自动启动
english.LaunchApp=Launch Bluetooth Battery now
chinesesimp.LaunchApp=立即启动蓝牙电量

[Tasks]
Name: "autostart"; Description: "{cm:AutoStart}"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#AppExePath}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{cm:AppName}"; Filename: "{app}\{#AppExe}"
Name: "{group}\{cm:UninstallProgram,{cm:AppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{cm:AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
; Same value as the in-app "Launch at login" menu item (quoted exe path), so both stay in sync
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

// Stop the running tray app before an upgrade so its exe is not locked
function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  KillRunningApp();
  Result := '';
end;

// The autostart value may also have been written from the app menu (task unchecked at install), so always remove it
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', '{#RunValueName}');
end;
