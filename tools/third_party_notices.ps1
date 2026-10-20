# Writes THIRD-PARTY-NOTICES.txt: the license files of every library built into the DLL,
# as they ship in each library's package. Run it again after changing dependencies.
#   powershell -ExecutionPolicy Bypass -File tools\third_party_notices.ps1
# ASCII only: Windows PowerShell 5.1 reads this file as ANSI.
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$registries = @(Get-ChildItem (Join-Path $cargoHome 'registry\src') -Directory)

# Normal dependencies only: build scripts' dependencies and proc macros are not in the DLL.
$lines = cargo tree --manifest-path (Join-Path $root 'Cargo.toml') -e normal,no-proc-macro --prefix none -f '{p}|{l}'
if ($LASTEXITCODE) { throw 'cargo tree failed' }
$packages = $lines | ForEach-Object { $_ -replace ' \(\*\)$', '' } | Where-Object { $_ -notlike 'mierucover *' } | Sort-Object -Unique

$out = New-Object Text.StringBuilder
[void]$out.Append("Third-party software in mierucover.dll`n`n")
[void]$out.Append("mierucover.dll contains the following Rust libraries. Their license files follow,`n")
[void]$out.Append("as included in each library's package. Where a library offers a choice of licenses,`n")
[void]$out.Append("all of its license files are included.`n`n")
foreach ($p in $packages) { [void]$out.Append("  $($p.Replace('|', '  '))`n") }

$seen = @{}
$missing = @()
foreach ($p in $packages) {
    $id, $license = $p.Split([char]'|')
    $name, $version = $id.Split([char]' ')
    $version = $version.TrimStart([char]'v')
    $dir = $registries | ForEach-Object { Join-Path $_.FullName "$name-$version" } | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $dir) { throw "source of $name $version not found under $cargoHome" }
    $files = @(Get-ChildItem $dir -File | Where-Object { $_.Name -match '^(LICEN[CS]E|COPYING|NOTICE|UNLICENSE)' } | Sort-Object Name)
    if (-not $files) {
        # A few packages leave the file out; a copy from their repository is kept in licenses\.
        $files = @(Get-ChildItem (Join-Path $root 'licenses') -File -Filter "$name-LICENSE*")
    }
    [void]$out.Append("`n`n==================== $name $version ($license) ====================`n")
    if (-not $files) {
        $missing += "$name $version"
        [void]$out.Append("`nThe package has no license file. License: $license`n")
    }
    foreach ($f in $files) {
        $text = [IO.File]::ReadAllText($f.FullName).Replace("`r`n", "`n").Trim()
        # Most libraries ship the same Apache-2.0 text: print each text once.
        $key = $text -replace '\s+', ' '
        [void]$out.Append("`n--- $($f.Name) ---`n")
        if ($seen.ContainsKey($key)) {
            [void]$out.Append("(Same text as $($seen[$key]).)`n")
        } else {
            $seen[$key] = "$($f.Name) of $name $version"
            [void]$out.Append("$text`n")
        }
    }
}

$path = Join-Path $root 'THIRD-PARTY-NOTICES.txt'
[IO.File]::WriteAllText($path, $out.ToString(), (New-Object Text.UTF8Encoding $false))
"wrote $path ($($packages.Count) libraries)"
if ($missing) { "no license file in: $($missing -join ', ')" }
