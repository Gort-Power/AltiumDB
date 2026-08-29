# Builds release + Inno Setup installer (packaging\dist\AltiumDB-Setup-*.exe)
param(
    [string]$Iscc = "C:\Program Files (x86)\Inno Setup 6\ISCC.exe"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

Write-Host "==> cargo build --release"
Push-Location $root
cmd /c '"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul && set "PATH=%USERPROFILE%\.cargo\bin;%PATH%" && cargo build --release'
Pop-Location

if (-not (Test-Path "$root\target\release\AltiumDB.exe")) {
    throw "Release build failed"
}

Write-Host "==> ISCC"
& $Iscc "$root\packaging\AltiumDB.iss"

Write-Host "==> Done:"
Get-ChildItem "$root\packaging\dist" | Select-Object Name, Length
