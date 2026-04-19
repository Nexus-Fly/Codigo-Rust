$vsBase = "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\MSVC"
$msvcVer = (Get-ChildItem $vsBase | Sort-Object Name -Descending | Select-Object -First 1).Name
$dumpbin = "$vsBase\$msvcVer\bin\HostX64\x64\dumpbin.exe"
$libExe  = "$vsBase\$msvcVer\bin\HostX64\x64\lib.exe"

# Detect build output dir dynamically (hash changes after cargo clean)
$buildBase = "C:\Hackatons\Nexus-Fly\codigo_rust\target\debug\build"
$outDir = Get-ChildItem $buildBase -Filter "tashi-vertex-*" |
    Select-Object -First 1 |
    ForEach-Object { Join-Path $_.FullName "out\lib" }
if (-not $outDir) { Write-Error "Run 'cargo build' first so tashi-vertex build output exists"; exit 1 }
$dll     = Join-Path $outDir "tashi-vertex.dll"
$defFile = Join-Path $outDir "tashi-vertex.def"
$libFile = Join-Path $outDir "tashi-vertex.lib"

Write-Host "Dumping exports from DLL..."
$raw = & $dumpbin /exports $dll

$inSection = $false
$exports = [System.Collections.Generic.List[string]]::new()
foreach ($line in $raw) {
    if ($line -match '^\s+ordinal\s+hint') { $inSection = $true; continue }
    # Stop at the Summary section (end of export table)
    if ($inSection -and $line -match '^\s+Summary') { break }
    if ($inSection) {
        # dumpbin format: "      7980 1F2B 000146A0 tv_base58_decode"
        # ordinal, hint (hex), RVA (hex), name
        if ($line -match '^\s+\d+\s+[0-9A-Fa-f]+\s+[0-9A-Fa-f]{8}\s+(\S+)') {
            $exports.Add($matches[1])
        }
    }
}
Write-Host "Found $($exports.Count) exports"

# Write DEF file
$defContent = @("LIBRARY `"tashi-vertex`"", "EXPORTS")
foreach ($exp in $exports) {
    $defContent += "    $exp"
}
$defContent | Set-Content -Path $defFile -Encoding ASCII
Write-Host "DEF file written: $defFile"

# Generate import LIB
Write-Host "Generating tashi-vertex.lib..."
& $libExe /DEF:$defFile /OUT:$libFile /MACHINE:X64
if (Test-Path $libFile) {
    Write-Host "SUCCESS: $libFile created ($('{0:N0}' -f (Get-Item $libFile).Length) bytes)"
} else {
    Write-Host "FAILED: lib file not created"
}
