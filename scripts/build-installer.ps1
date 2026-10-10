# 编译 release 版并打包安装程序，输出到 target\installer\
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

cargo build --release --manifest-path "$root\Cargo.toml"
if ($LASTEXITCODE -ne 0) { throw 'cargo build 失败' }

$iscc = @(
    (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe"
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $iscc) { throw '未找到 Inno Setup 6，请先安装：winget install JRSoftware.InnoSetup' }

& $iscc /Q "$root\installer\setup.iss"
if ($LASTEXITCODE -ne 0) { throw 'ISCC 打包失败' }
Get-ChildItem "$root\target\installer\*.exe" | Sort-Object LastWriteTime | Select-Object -Last 1 | ForEach-Object { "安装包: $($_.FullName)" }
