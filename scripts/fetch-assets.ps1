# Downloads the runtime assets into assets/ and verifies their SHA256.
#   - ONNX Runtime 1.27.0 (Microsoft), same version HushMic ships
#   - DPDFNet models and native-engine weights (Ceva, Apache-2.0) from the
#     HushMic v0.10.2 release
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$assets = Join-Path $root 'assets'
New-Item -ItemType Directory -Force $assets | Out-Null

$hushmic = 'https://github.com/Fovty/HushMic/releases/download/v0.10.2'
$files = @(
    @{ Name = 'dpdfnet8_48khz_hr.onnx'; Url = "$hushmic/dpdfnet8_48khz_hr.onnx"; Sha = '7b3afbb260a08fe9af3d16e3bda992971be1e7e951d1dee7c2d235f5c43f5631' }
    @{ Name = 'dpdfnet2_48khz_hr.onnx'; Url = "$hushmic/dpdfnet2_48khz_hr.onnx"; Sha = '7f0575a5cec0ba4ffd8f8bd657e06d007e4ccdd955d76faab922b9d3291dc14b' }
    # Packed weights for the native INT8 engine (same DPDFNet commit as the C runtime).
    @{ Name = 'dpdfnet8_48khz_hr.weights.f32'; Url = "$hushmic/dpdfnet8_48khz_hr.weights.f32"; Sha = '5a5bb67a8619090c54dc2793536fe813fe38123b236d445f3433aafe3075529d' }
    @{ Name = 'dpdfnet2_48khz_hr.weights.f32'; Url = "$hushmic/dpdfnet2_48khz_hr.weights.f32"; Sha = 'cb7248b8fccbff7b32f254ec2ed0061e604514b2ccd98d997cd081a876424e7b' }
    @{ Name = 'onnxruntime-win-x64-1.27.0.zip'; Url = 'https://github.com/microsoft/onnxruntime/releases/download/v1.27.0/onnxruntime-win-x64-1.27.0.zip'; Sha = 'c5c81710938e68079ff1a192b04897faabe4b43830d48f39f27ecd4e16138bfc' }
)

foreach ($f in $files) {
    $dest = Join-Path $assets $f.Name
    if (-not (Test-Path $dest) -or (Get-FileHash $dest -Algorithm SHA256).Hash -ne $f.Sha.ToUpper()) {
        Write-Host "Lade $($f.Name) ..."
        Invoke-WebRequest $f.Url -OutFile $dest
    }
    $hash = (Get-FileHash $dest -Algorithm SHA256).Hash
    if ($hash -ne $f.Sha.ToUpper()) {
        Remove-Item $dest
        throw "Pruefsumme falsch: $($f.Name)"
    }
    Write-Host "OK  $($f.Name)"
}

# Only onnxruntime.dll is needed from the ZIP.
$zip = Join-Path $assets 'onnxruntime-win-x64-1.27.0.zip'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [IO.Compression.ZipFile]::OpenRead($zip)
try {
    $entry = $archive.Entries | Where-Object { $_.FullName -like '*/lib/onnxruntime.dll' }
    [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, (Join-Path $assets 'onnxruntime.dll'), $true)
    # License and third-party notices must ship with onnxruntime.dll.
    foreach ($n in @(@('LICENSE', 'LICENSE-onnxruntime.txt'), @('ThirdPartyNotices.txt', 'ThirdPartyNotices-onnxruntime.txt'))) {
        $e = $archive.Entries | Where-Object { $_.FullName -like "*/$($n[0])" } | Select-Object -First 1
        if (-not $e) { throw "$($n[0]) fehlt im ONNX-Runtime-Archiv" }
        [IO.Compression.ZipFileExtensions]::ExtractToFile($e, (Join-Path $assets $n[1]), $true)
    }
} finally {
    $archive.Dispose()
}
Write-Host "OK  onnxruntime.dll"
