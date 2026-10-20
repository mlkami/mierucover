# Shared by install.ps1 and uninstall.ps1. ASCII only.
$InstallDir = Join-Path $env:LOCALAPPDATA 'Programs\mierucover'
$InstalledDll = Join-Path $InstallDir 'mierucover.dll'

# Explorer runs thumbnail providers in dllhost.exe surrogates that keep the DLL loaded
# until they go idle. Ending the ones holding mierucover makes Explorer load the new DLL.
function Stop-ThumbnailHosts {
    Get-Process dllhost -ErrorAction SilentlyContinue | Where-Object {
        try { @($_.Modules | Where-Object { $_.FileName -like '*\mierucover.dll*' }).Count -gt 0 } catch { $false }
    } | ForEach-Object { try { Stop-Process -Id $_.Id -Force -ErrorAction Stop } catch {} }
}
