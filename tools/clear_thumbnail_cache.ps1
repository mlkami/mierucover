# Deletes Windows' thumbnail cache so every thumbnail is made again (photos included).
# Explorer is closed for a moment (open Explorer windows close) and restarted.
#   powershell -ExecutionPolicy Bypass -File tools\clear_thumbnail_cache.ps1
# ASCII only: Windows PowerShell 5.1 reads this file as ANSI.
$ErrorActionPreference = 'Stop'
$cache = Join-Path $env:LOCALAPPDATA 'Microsoft\Windows\Explorer'
Stop-Process -Name explorer -Force
Start-Sleep -Seconds 2
$left = @()
foreach ($f in Get-ChildItem $cache -Filter 'thumbcache_*.db') {
    try { Remove-Item $f.FullName -Force -ErrorAction Stop } catch { $left += $f.Name }
}
if (-not (Get-Process explorer -ErrorAction SilentlyContinue)) { Start-Process explorer.exe }
if ($left) { "still in use (not deleted): $($left -join ', ')" } else { 'thumbnail cache cleared' }
