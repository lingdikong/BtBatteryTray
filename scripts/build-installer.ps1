# Build the release exe and package the Windows installer into target\installer\
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

cargo build --release --manifest-path "$root\Cargo.toml"
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

$iscc = @(
    (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe"
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $iscc) { throw 'Inno Setup 6 not found; install it with: winget install JRSoftware.InnoSetup' }

& $iscc /Q "$root\installer\setup.iss"
if ($LASTEXITCODE -ne 0) { throw 'ISCC packaging failed' }
Get-ChildItem "$root\target\installer\*.exe" | Sort-Object LastWriteTime | Select-Object -Last 1 | ForEach-Object { "Installer: $($_.FullName)" }
