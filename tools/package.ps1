# Builds the zip for a GitHub release: target\mierucover-<version>.zip, holding the DLL,
# the install scripts and the license files. Unzipped, install.ps1 installs that DLL.
#   powershell -ExecutionPolicy Bypass -File tools\package.ps1
# ASCII only: Windows PowerShell 5.1 reads this file as ANSI.
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem

cargo build --release --lib --manifest-path (Join-Path $root 'Cargo.toml')
if ($LASTEXITCODE) { throw 'build failed' }
$version = ([regex]'(?m)^version = "([^"]+)"').Match([IO.File]::ReadAllText((Join-Path $root 'Cargo.toml'))).Groups[1].Value

$name = "mierucover-$version"
$stage = Join-Path $root "target\package\$name"
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force (Join-Path $stage 'tools'), (Join-Path $stage 'licenses') | Out-Null
Copy-Item (Join-Path $root 'target\release\mierucover.dll') $stage
foreach ($f in 'install.ps1', 'uninstall.ps1', 'README.md', 'LICENSE', 'THIRD-PARTY-NOTICES.txt') { Copy-Item (Join-Path $root $f) $stage }
foreach ($f in 'common.ps1', 'clear_thumbnail_cache.ps1') { Copy-Item (Join-Path $root "tools\$f") (Join-Path $stage 'tools') }
Copy-Item (Join-Path $root 'licenses\*') (Join-Path $stage 'licenses')

$zip = Join-Path $root "target\$name.zip"
if (Test-Path $zip) { Remove-Item $zip }
# Entries are added one by one: ZipFile.CreateFromDirectory in Windows PowerShell writes
# '\' into the paths, which the zip format does not allow.
$file = [IO.File]::Open($zip, [IO.FileMode]::CreateNew)
$archive = New-Object IO.Compression.ZipArchive($file, [IO.Compression.ZipArchiveMode]::Create)
try {
    foreach ($f in Get-ChildItem $stage -Recurse -File) {
        $entry = "$name/" + $f.FullName.Substring($stage.Length + 1).Replace('\', '/')
        [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, $f.FullName, $entry, [IO.Compression.CompressionLevel]::Optimal)
    }
} finally {
    $archive.Dispose()
    $file.Dispose()
}
"wrote $zip"
