# Builds dummy archives for testing into target\fixtures (pages are drawn, no real content).
# Needs 7-Zip (C:\Program Files\7-Zip\7z.exe). ASCII only: Windows PowerShell 5.1 reads this file as ANSI.
#   powershell -ExecutionPolicy Bypass -File tools\make_fixtures.ps1
param([string]$Out = (Join-Path $PSScriptRoot '..\target\fixtures'))
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.IO.Compression.FileSystem
$SevenZip = 'C:\Program Files\7-Zip\7z.exe'

$Out = [IO.Path]::GetFullPath($Out)
if (Test-Path $Out) { Remove-Item -Recurse -Force $Out }
$Src = Join-Path $Out 'src'
New-Item -ItemType Directory -Force $Src | Out-Null

$Hyoushi = "$([char]0x8868)$([char]0x7D19)"          # cover
$UraHyoushi = "$([char]0x88CF)$Hyoushi"               # back cover

function Draw-Busy($g, [int]$x, [int]$y, [int]$w, [int]$h, [int]$seed = 7) {
    $rnd = New-Object Random $seed
    for ($i = 0; $i -lt 400; $i++) {
        $brush = New-Object Drawing.SolidBrush ([Drawing.Color]::FromArgb($rnd.Next(256), $rnd.Next(256), $rnd.Next(256)))
        $g.FillEllipse($brush, $x + $rnd.Next($w), $y + $rnd.Next($h), $rnd.Next(10, [Math]::Max(11, [int]($w / 4))), $rnd.Next(10, [Math]::Max(11, [int]($h / 4))))
        $brush.Dispose()
    }
}

function Draw-Label($g, [string]$text, [int]$x, [int]$y, [int]$w, [int]$h) {
    $font = New-Object Drawing.Font 'Arial', ([Math]::Max(10, [int]([Math]::Min($w, $h) / 6))), ([Drawing.FontStyle]::Bold), ([Drawing.GraphicsUnit]::Pixel)
    $format = New-Object Drawing.StringFormat
    $format.Alignment = 'Center'; $format.LineAlignment = 'Center'
    $g.DrawString($text, $font, [Drawing.Brushes]::Black, (New-Object Drawing.RectangleF $x, $y, $w, $h), $format)
    $font.Dispose()
}

function Save-Image($bmp, [string]$path) {
    $format = if ($path -like '*.png') { [Drawing.Imaging.ImageFormat]::Png } else { [Drawing.Imaging.ImageFormat]::Jpeg }
    $bmp.Save($path, $format)
    $bmp.Dispose()
}

# A page: plain gray with a label, or 'busy' (colorful, like a front cover).
function New-Page([string]$path, [int]$w, [int]$h, [string]$label, [switch]$Busy) {
    $bmp = New-Object Drawing.Bitmap $w, $h
    $g = [Drawing.Graphics]::FromImage($bmp)
    $g.Clear([Drawing.Color]::Gainsboro)
    if ($Busy) { Draw-Busy $g 0 0 $w $h }
    Draw-Label $g $label 0 0 $w $h
    $g.Dispose()
    Save-Image $bmp $path
}

# A front+back spread; the front (busy, "FRONT") is on $front ('left' or 'right').
function New-Spread([string]$path, [int]$w, [int]$h, [string]$front) {
    $bmp = New-Object Drawing.Bitmap $w, $h
    $g = [Drawing.Graphics]::FromImage($bmp)
    $g.Clear([Drawing.Color]::Gainsboro)
    $half = [int]($w / 2)
    $fx = if ($front -eq 'left') { 0 } else { $half }
    $bx = if ($front -eq 'left') { $half } else { 0 }
    Draw-Busy $g $fx 0 $half $h
    Draw-Label $g 'FRONT' $fx 0 $half $h
    Draw-Label $g 'back' $bx 0 $half $h
    $g.Dispose()
    Save-Image $bmp $path
}

# A full jacket: flap | front | spine | back | flap, front on the left of the spine.
function New-Jacket([string]$path) {
    $w = 4400; $h = 2000
    $bmp = New-Object Drawing.Bitmap $w, $h
    $g = [Drawing.Graphics]::FromImage($bmp)
    $g.Clear([Drawing.Color]::Beige)
    Draw-Busy $g 760 0 1400 $h
    Draw-Label $g 'FRONT' 760 0 1400 $h
    $g.FillRectangle([Drawing.Brushes]::SteelBlue, 2160, 0, 80, $h)
    Draw-Label $g 'back' 2240 0 1400 $h
    Draw-Label $g 'flap' 0 0 760 $h
    Draw-Label $g 'flap' 3640 0 760 $h
    $g.Dispose()
    Save-Image $bmp $path
}

function New-Dir([string]$name) {
    $d = Join-Path $Src $name
    New-Item -ItemType Directory -Force $d | Out-Null
    $d
}

function New-Pages([string]$dir, [int]$count, [string]$prefix = '', [string]$ext = 'jpg', [int]$w = 1200, [int]$h = 1700) {
    for ($i = 1; $i -le $count; $i++) {
        $name = '{0}{1:D3}.{2}' -f $prefix, $i, $ext
        if ($i -eq 1) { New-Page (Join-Path $dir $name) $w $h 'COVER' -Busy } else { New-Page (Join-Path $dir $name) $w $h "P$i" }
    }
}

function Pack([string]$dir, [string]$archive, [string]$type = 'zip', [string]$level = '0') {
    Push-Location $dir
    & $SevenZip a "-t$type" "-mx$level" (Join-Path $Out $archive) * -r | Out-Null
    if ($LASTEXITCODE) { throw "7z failed: $archive" }
    Pop-Location
}

# 1-2. Plain books, stored and deflated.
$d = New-Dir 'basic'; New-Pages $d 8
Pack $d 'zip_stored.zip' 'zip' '0'
Pack $d 'zip_deflate.cbz' 'zip' '5'
Pack $d 'tar_basic.tar' 'tar'
Pack $d 'sevenz_solid.7z' '7z' '5'

# 3. Archive order 10, 2, 1: the cover is 1.
$d = New-Dir 'order'
New-Page (Join-Path $d '1.jpg') 1200 1700 'COVER' -Busy
New-Page (Join-Path $d '2.jpg') 1200 1700 'P2'
New-Page (Join-Path $d '10.jpg') 1200 1700 'P10'
Push-Location $d
foreach ($f in '10.jpg', '2.jpg', '1.jpg') { & $SevenZip a -tzip -mx0 (Join-Path $Out 'zip_order.zip') $f | Out-Null }
Pop-Location

# 4. Named cover sorting after the pages, plus a back cover; CP932 names without the UTF-8 flag.
$d = New-Dir 'covername'
New-Page (Join-Path $d '001.jpg') 1200 1700 'P1'
New-Page (Join-Path $d '002.jpg') 1200 1700 'P2'
New-Page (Join-Path $d "$Hyoushi.jpg") 1200 1700 'COVER' -Busy
New-Page (Join-Path $d "$UraHyoushi.jpg") 1200 1700 'BACK'
[IO.Compression.ZipFile]::CreateFromDirectory($d, (Join-Path $Out 'zip_covername_cp932.zip'), [IO.Compression.CompressionLevel]::NoCompression, $false, [Text.Encoding]::GetEncoding(932))

# 5. Store sample image (560x420) in front of the cover.
$d = New-Dir 'store'
New-Page (Join-Path $d '00.jpg') 560 420 'STORE'
New-Page (Join-Path $d '01.jpg') 1200 1700 'COVER' -Busy
New-Page (Join-Path $d '02.jpg') 1200 1700 'P2'
Pack $d 'zip_store_sample.zip'

# 6-7. Cover spreads, front on either side.
foreach ($side in 'left', 'right') {
    $d = New-Dir "spread_$side"
    New-Spread (Join-Path $d '000.jpg') 2400 1700 $side
    New-Page (Join-Path $d '001.jpg') 1200 1700 'P1'
    New-Page (Join-Path $d '002.jpg') 1200 1700 'P2'
    Pack $d "zip_spread_$side.zip"
}

# 8. A landscape CG set: wide first image followed by wide pages, not a spread.
$d = New-Dir 'cgset'
New-Spread (Join-Path $d '001.jpg') 1600 1200 'left'
New-Page (Join-Path $d '002.jpg') 1600 1200 'CG2'
New-Page (Join-Path $d '003.jpg') 1600 1200 'CG3'
Pack $d 'zip_cgset.zip'

# 9. Whole jacket with flaps.
$d = New-Dir 'jacket'
New-Jacket (Join-Path $d '000a.jpg')
New-Page (Join-Path $d '001.jpg') 1200 1700 'P1'
Pack $d 'zip_jacket.zip'

# 10. macOS metadata files in front.
$d = New-Dir 'macosx'
New-Pages $d 3
New-Item -ItemType Directory -Force (Join-Path $d '__MACOSX') | Out-Null
[IO.File]::WriteAllBytes((Join-Path $d '__MACOSX\._001.jpg'), [byte[]](0, 5, 22, 7, 0, 2, 0, 0))
[IO.File]::WriteAllBytes((Join-Path $d '._000.jpg'), [byte[]](0, 5, 22, 7, 0, 2, 0, 0))
Pack $d 'zip_macosx.zip'

# 11. Transparent PNG cover.
$d = New-Dir 'alpha'
$bmp = New-Object Drawing.Bitmap 1200, 1700, ([Drawing.Imaging.PixelFormat]::Format32bppArgb)
$g = [Drawing.Graphics]::FromImage($bmp); $g.Clear([Drawing.Color]::Transparent)
$g.FillEllipse([Drawing.Brushes]::OrangeRed, 200, 300, 800, 1100); Draw-Label $g 'ALPHA' 0 0 1200 1700; $g.Dispose()
Save-Image $bmp (Join-Path $d '001.png')
New-Page (Join-Path $d '002.png') 1200 1700 'P2'
Pack $d 'zip_alpha.zip'

# 12-13. EPUB 2 (meta cover) and EPUB 3 (cover-image), each with an image sorting before the cover.
function New-Epub([string]$name, [string]$opf, [hashtable]$images, [hashtable]$files = @{}) {
    $d = New-Dir $name
    New-Item -ItemType Directory -Force (Join-Path $d 'META-INF'), (Join-Path $d 'OEBPS\img') | Out-Null
    [IO.File]::WriteAllText((Join-Path $d 'mimetype'), 'application/epub+zip')
    [IO.File]::WriteAllText((Join-Path $d 'META-INF\container.xml'), '<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>')
    [IO.File]::WriteAllText((Join-Path $d 'OEBPS\content.opf'), $opf)
    foreach ($k in $images.Keys) { New-Page (Join-Path $d "OEBPS\img\$k") 1000 1500 $images[$k] -Busy:($images[$k] -eq 'COVER') }
    foreach ($k in $files.Keys) { [IO.File]::WriteAllText((Join-Path $d "OEBPS\$k"), $files[$k]) }
    Pack $d "$name.epub"
}
New-Epub 'epub2' '<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="2.0"><metadata><meta name="cover" content="cover-img"/></metadata><manifest><item id="a" href="img/a001.jpg" media-type="image/jpeg"/><item id="cover-img" href="img/front%20page.jpg" media-type="image/jpeg"/></manifest><spine><itemref idref="a"/></spine></package>' @{ 'a001.jpg' = 'P1'; 'front page.jpg' = 'COVER' }
New-Epub 'epub3' '<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><manifest><item id="x" href="img/0001.jpg" media-type="image/jpeg"/><item id="y" href="img/main.jpg" media-type="image/jpeg" properties="cover-image"/></manifest><spine><itemref idref="x"/></spine></package>' @{ '0001.jpg' = 'P1'; 'main.jpg' = 'COVER' }

# EPUB whose cover-image is an SVG page wrapping the real image (WIC cannot read SVG).
New-Epub 'epub_svg' '<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><manifest><item id="s" href="cover.svg" media-type="image/svg+xml" properties="cover-image"/><item id="x" href="img/0001.jpg" media-type="image/jpeg"/><item id="c" href="img/c.jpg" media-type="image/jpeg"/></manifest><spine><itemref idref="x"/></spine></package>' @{ '0001.jpg' = 'P1'; 'c.jpg' = 'COVER' } @{ 'cover.svg' = '<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 1000 1500"><image width="1000" height="1500" xlink:href="img/c.jpg"/></svg>' }

# 14. A zip holding EPUBs (stored) and one holding deflated zips.
$d = New-Dir 'epubs'
Copy-Item (Join-Path $Out 'epub2.epub'), (Join-Path $Out 'epub3.epub') $d
Pack $d 'zip_of_epubs.zip' 'zip' '0'
$d = New-Dir 'volumes'
Copy-Item (Join-Path $Out 'zip_stored.zip') (Join-Path $d 'vol01.zip')
Copy-Item (Join-Path $Out 'zip_spread_left.zip') (Join-Path $d 'vol02.zip')
Pack $d 'zip_of_zips.zip' 'zip' '9'

# 15. Encrypted zip: no cover can be read.
Push-Location (Join-Path $Src 'basic')
& $SevenZip a -tzip -mx0 -psecret (Join-Path $Out 'zip_encrypted.zip') * | Out-Null
Pop-Location

# 16. A download cut short: no central directory, only the first entries.
$bytes = [IO.File]::ReadAllBytes((Join-Path $Out 'zip_stored.zip'))
$part = New-Object byte[] ([int]($bytes.Length * 0.6))
[Array]::Copy($bytes, $part, $part.Length)
[IO.File]::WriteAllBytes((Join-Path $Out 'zip_truncated.zip'), $part)

Get-ChildItem $Out -File | Select-Object Name, Length | Format-Table -AutoSize
