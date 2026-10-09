# dpdf-voice

Real-time microphone noise suppression for Windows as a VST2 plugin, built for
[Equalizer APO](https://sourceforge.net/projects/equalizerapo/). It runs
[DPDFNet](https://github.com/ceva-ip/DPDFNet), a neural speech enhancement
model, through the engine of [HushMic](https://github.com/Fovty/HushMic), and
works system-wide: Discord, Teams, OBS and every other app get the cleaned
microphone.

Why another denoiser: RNNoise and DeepFilterNet tend to swallow soft speech
onsets such as "v" or "f" on some microphones. DPDFNet keeps them.

- Runs on the CPU, no GPU needed, no network access
- Native INT8 engine: about 11 % of one CPU core for the quality model
  (Ryzen 7 9800X3D), ONNX Runtime as fallback on CPUs without AVX2
- Latency: 60 ms (50 ms model, 10 ms block alignment)
- Mono, 48 kHz

## Requirements

- Windows 10/11, x64
- Equalizer APO 1.4 or newer, installed on the microphone
- Microphone running at 48 kHz (Sound settings → microphone → Properties →
  Advanced). At other rates the plugin passes audio through unchanged.

## Installation

1. Download `dpdf-voice-<version>-windows-x64.zip` from the
   [releases](https://github.com/heast13/dpdf-voice/releases).
2. Copy the `dpdf-voice` folder from the ZIP to
   `C:\Program Files\EqualizerAPO\VSTPlugins\` (needs administrator rights).
3. Add this line to `C:\Program Files\EqualizerAPO\config\config.txt`, below
   the `Device:` line of your microphone and before other effects such as a
   compressor:

   ```
   VSTPlugin: Library dpdf-voice\dpdf_voice.dll Model 0 Limit 1 Bypass 0
   ```

   Or use the Configuration Editor: add a "VST plugin" filter and pick
   `dpdf_voice.dll`.

The filter is active as soon as an app opens the microphone.

## Parameters

Values are normalized 0..1, as Equalizer APO writes them.

| Name | Values | Meaning |
|---|---|---|
| `Model` | `0` quality, `1` light | quality: `dpdfnet8` (best), light: `dpdfnet2` (about half the CPU) |
| `Limit` | `0`…`1` = 0…100 dB | maximum attenuation; `1` removes noise completely, lower values keep some room sound |
| `Bypass` | `0` off, `1` on | passes audio through, latency-aligned |

## Troubleshooting

**Nothing changes.** Make sure audio enhancements are enabled for the
microphone: Control Panel → Sound → Recording → microphone → Properties →
Advanced → "Enable audio enhancements". Windows may switch them off after an
effect misbehaved; Equalizer APO is skipped entirely then.

**Log file.** The plugin writes load results and errors to
`C:\Windows\ServiceProfiles\LocalService\AppData\Local\Temp\dpdf-voice.log`
(readable as administrator). A healthy start looks like:

```
model quality loaded, engine native int8
processing: 480 samples per block, 508 KB stack free
```

**Updating the plugin.** The Windows audio service keeps the DLL loaded.
Close the Equalizer APO editor, rename the old `dpdf_voice.dll`, copy the new
one, then restart the "Windows Audio" service or reboot.

## Building from source

Needs Rust (stable, MSVC toolchain) and the Visual Studio Build Tools with the
C++ workload.

```powershell
./scripts/fetch-assets.ps1      # models, native weights, ONNX Runtime (checksummed)
cargo build --release
./scripts/package.ps1           # dist/dpdf-voice-<version>-windows-x64.zip
```

Tools in the workspace:

- `dpdf-voice-cli`: denoises a WAV file, `DPDF_ENGINE` selects the engine
  (`auto`, `onnx`, `native-int8`, `native-fp32`).
- `dpdf-voice-hosttest`: loads the DLL like a VST host; `--eapo` replays
  Equalizer APO's exact call sequence, including the host writing to
  `AEffect::user`.
- `scripts/ci-check.py`: end-to-end check used by CI.

`vendor/` holds two patched dependencies, each with a `PATCHES.md`:
vst-rs (plugin data moved out of `AEffect::user`, which Equalizer APO
overwrites) and hushmic-denoiser (native engine builds with MSVC).

## License

MIT OR Apache-2.0, at your option. Third-party components and their licenses
are listed in [THIRD_PARTY.md](THIRD_PARTY.md). The DPDFNet models are by
Ceva, the engine is from HushMic by Fovty. dpdf-voice is not affiliated with
either project.
