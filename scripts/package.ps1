# Builds the release folder and ZIP from target/release and assets/.
# Run scripts/fetch-assets.ps1 and `cargo build --release` first.
#   dist/dpdf-voice-<version>-windows-x64.zip
#     dpdf-voice/  -> copy into EqualizerAPO\VSTPlugins\
#     licenses/    -> license files of everything shipped
param([string]$Out = (Join-Path (Split-Path $PSScriptRoot -Parent) 'dist'))
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$assets = Join-Path $root 'assets'

$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$name = "dpdf-voice-$version-windows-x64"
$stage = Join-Path $Out $name
$plugin = Join-Path $stage 'dpdf-voice'
$licenses = Join-Path $stage 'licenses'
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $plugin, $licenses | Out-Null

Copy-Item (Join-Path $root 'target\release\dpdf_voice.dll') $plugin
foreach ($f in 'onnxruntime.dll',
               'dpdfnet8_48khz_hr.onnx', 'dpdfnet8_48khz_hr.weights.f32',
               'dpdfnet2_48khz_hr.onnx', 'dpdfnet2_48khz_hr.weights.f32') {
    Copy-Item (Join-Path $assets $f) $plugin
}
foreach ($f in 'README.md', 'CHANGELOG.md', 'LICENSE-MIT', 'LICENSE-APACHE', 'THIRD_PARTY.md') {
    Copy-Item (Join-Path $root $f) $stage
}
foreach ($f in 'LICENSE-onnxruntime.txt', 'ThirdPartyNotices-onnxruntime.txt') {
    Copy-Item (Join-Path $assets $f) $licenses
}
python (Join-Path $PSScriptRoot 'collect-licenses.py') (Join-Path $licenses 'crates')
if ($LASTEXITCODE -ne 0) { throw 'collect-licenses.py failed' }

$zip = Join-Path $Out "$name.zip"
Remove-Item $zip -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
# LF line ending so `sha256sum -c` works on Linux and macOS.
[IO.File]::WriteAllText((Join-Path $Out "$name.zip.sha256"), "$hash  $name.zip`n")
Write-Host "$zip"
Write-Host "sha256 $hash"
