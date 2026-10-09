# Changelog

## 0.1.0

First release.

- Mono VST2 plugin for Equalizer APO with DPDFNet noise suppression
  (models `dpdfnet8` quality and `dpdfnet2` light).
- Native INT8 engine (DPDFNet C runtime, built with MSVC), ONNX Runtime as
  fallback; the DLL links the C runtime statically.
- Parameters `Model`, `Limit`, `Bypass`.
- Safe in the Windows audio service: model loading on its own thread, stack
  guard, NaN/Inf filtering, passthrough instead of crashing.
- Tools: offline CLI, test host replaying Equalizer APO's VST2 call sequence,
  end-to-end CI check.
