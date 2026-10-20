# Installs (or updates) mierucover for the current user. No administrator rights needed.
#   powershell -ExecutionPolicy Bypass -File install.ps1 [-NoBuild]
# From the release zip it installs the mierucover.dll next to this file; from the source
# it builds first (skipped with -NoBuild).
# ASCII only: Windows PowerShell 5.1 reads this file as ANSI.
param([switch]$NoBuild)
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
. (Join-Path $root 'tools\common.ps1')

$dll = Join-Path $root 'mierucover.dll'
if (-not (Test-Path $dll)) {
    if (-not $NoBuild) {
        cargo build --release --manifest-path (Join-Path $root 'Cargo.toml')
        if ($LASTEXITCODE) { throw 'build failed' }
    }
    $dll = Join-Path $root 'target\release\mierucover.dll'
}

New-Item -ItemType Directory -Force $InstallDir | Out-Null
# A loaded DLL cannot be overwritten, but it can be renamed: move the old one aside.
Get-ChildItem $InstallDir -Filter 'mierucover.dll.old*' | ForEach-Object { try { Remove-Item $_.FullName -Force -ErrorAction Stop } catch {} }
if (Test-Path $InstalledDll) { Rename-Item $InstalledDll ('mierucover.dll.old' + [DateTime]::Now.Ticks) }
Copy-Item $dll $InstalledDll
Copy-Item (Join-Path $root 'licenses\*') $InstallDir -Force

$p = Start-Process regsvr32.exe -ArgumentList '/s', "`"$InstalledDll`"" -Wait -PassThru
if ($p.ExitCode) { throw "regsvr32 failed: $($p.ExitCode)" }
Stop-ThumbnailHosts
"installed: $InstalledDll"
