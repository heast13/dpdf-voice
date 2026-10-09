# Local changes to hushmic-denoiser v0.10.2

Source: https://github.com/Fovty/HushMic, tag v0.10.2, `crates/hushmic-denoiser`
(MIT OR Apache-2.0; the native runtime under `native/` is by Ceva, Apache-2.0).

## Native engine builds with MSVC

The `native` feature compiled the DPDFNet C runtime with GCC/Clang only.
Changes for the `x86_64-pc-windows-msvc` target, other targets unchanged:

- `build.rs`: MSVC gets `/std:c11` and `/arch:AVX2` for the AVX2/FP16 files
  instead of `-std=gnu11 -ffp-contract=off -fno-lto -mavx2 -mfma -mf16c`.
  MSVC does not contract to FMA under the default `/fp:precise`, which
  matches `-ffp-contract=off`. No `-lm`: the MSVC C runtime has the math
  functions.
- `native/runtime/dpdf_dprnn.c`: CPU detection via `__cpuid`/`_xgetbv`
  instead of `__builtin_cpu_supports` (checks AVX, FMA, F16C, OSXSAVE,
  XCR0 YMM state and AVX2, like the GCC builtin).
- `native/runtime/fp16.c`: scalar FP16 packing via `_mm_cvtps_ph` on a
  128-bit register instead of `_cvtss_sh` (not available in MSVC); same
  rounding mode, identical result.
- `native/runtime/internal.h`, `int8.c`: `DPDF_NOINLINE` macro instead of
  `__attribute__((noinline))`.
- `Cargo.toml`: workspace-inherited fields written out.
- Every modified C file starts with a modification notice (Apache-2.0 §4(b)).
- `native/README.md`: note about the modified files added at the top.
- `tests/` and `examples/` are upstream as-is; they need HushMic's asset
  script and fixtures and are not built or run in dpdf-voice.
