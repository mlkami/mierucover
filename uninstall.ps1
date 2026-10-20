# Removes mierucover: its registration (only the keys it owns) and its files.
#   powershell -ExecutionPolicy Bypass -File uninstall.ps1
# ASCII only: Windows PowerShell 5.1 reads this file as ANSI.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'tools\common.ps1')

if (Test-Path $InstalledDll) {
    $p = Start-Process regsvr32.exe -ArgumentList '/u', '/s', "`"$InstalledDll`"" -Wait -PassThru
    if ($p.ExitCode) { throw "regsvr32 /u failed: $($p.ExitCode)" }
}
Stop-ThumbnailHosts
try { Remove-Item -Recurse -Force $InstallDir -ErrorAction Stop } catch { "could not remove $InstallDir now (in use); delete it after signing out" }
'uninstalled'
