"""End-to-end check of the built plugin, standard library only.

1. Generates a synthetic 48 kHz test signal: noise only, then noise plus a
   harmonic tone (a stand-in for voice; no recordings live in the repo).
2. Denoises it with dpdf-voice-cli (reference) and runs it through the
   plugin DLL with Equalizer APO's exact VST2 call sequence
   (dpdf-voice-hosttest --eapo).
3. Checks: the plugin output equals the reference (after the plugin's
   2880-sample delay), the noise drops by at least 20 dB, and the tone
   level stays within 3 dB.

Usage: python scripts/ci-check.py <plugin folder> <tool folder>
"""
import math
import os
import random
import struct
import subprocess
import sys
import tempfile
import wave

RATE = 48000
PLUGIN_DELAY = 2880  # LATENCY_SAMPLES + HOP, see the plugin's initial_delay


def write_test_signal(path):
    rnd = random.Random(1)
    frames = []
    for i in range(RATE * 4):
        noise = rnd.gauss(0.0, 0.02)
        t = i / RATE
        tone = 0.0
        if i >= RATE * 2:
            tone = sum(0.12 / k * math.sin(2 * math.pi * 180 * k * t) for k in range(1, 6))
        frames.append(struct.pack("<h", max(-32768, min(32767, int((noise + tone) * 32767)))))
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(b"".join(frames))


def read_wav(path):
    data = open(path, "rb").read()
    fmt = struct.unpack("<H", data[20:22])[0]
    if fmt == 0xFFFE:
        fmt = struct.unpack("<H", data[44:46])[0]
    i = data.find(b"data")
    n = struct.unpack("<I", data[i + 4:i + 8])[0]
    body = data[i + 8:i + 8 + n]
    if fmt == 3:
        return list(struct.unpack("<%df" % (len(body) // 4), body))
    return [v / 32768 for v in struct.unpack("<%dh" % (len(body) // 2), body)]


def level_db(x):
    return 10 * math.log10(sum(v * v for v in x) / len(x) + 1e-20)


def run(*cmd):
    print("$", " ".join(cmd), flush=True)
    subprocess.run(cmd, check=True)


def main():
    plugin_dir, tool_dir = sys.argv[1], sys.argv[2]
    dll = os.path.join(plugin_dir, "dpdf_voice.dll")
    model = os.path.join(plugin_dir, "dpdfnet8_48khz_hr.onnx")
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        src = os.path.join(tmp, "in.wav")
        ref = os.path.join(tmp, "ref.wav")
        out = os.path.join(tmp, "eapo.wav")
        write_test_signal(src)
        env_dll = dict(os.environ, DPDF_ORT_DLL=os.path.join(plugin_dir, "onnxruntime.dll"))
        print("$ dpdf-voice-cli", flush=True)
        subprocess.run([os.path.join(tool_dir, "dpdf-voice-cli.exe"), model, src, ref], check=True, env=env_dll)
        run(os.path.join(tool_dir, "dpdf-voice-hosttest.exe"), "--eapo", dll, src, out)

        x, r, y = read_wav(src), read_wav(ref), read_wav(out)
        n = min(len(r), len(y) - PLUGIN_DELAY)
        diff = max(abs(y[i + PLUGIN_DELAY] - r[i]) for i in range(n))
        print(f"plugin vs reference: max difference {diff:.2e}")
        if diff > 1e-6:
            failures.append("plugin output differs from the reference")

        # Skip the first half second of each segment (model warm-up, tone onset).
        noise = slice(RATE // 2, RATE * 2)
        tone = slice(RATE * 2 + RATE // 2, RATE * 4)
        noise_drop = level_db(x[noise]) - level_db(r[noise])
        tone_change = level_db(r[tone]) - level_db(x[tone])
        print(f"noise reduced by {noise_drop:.1f} dB, tone level changed by {tone_change:+.1f} dB")
        if noise_drop < 20:
            failures.append(f"noise only reduced by {noise_drop:.1f} dB (< 20)")
        if abs(tone_change) > 3:
            failures.append(f"tone level changed by {tone_change:+.1f} dB (> 3)")

    if failures:
        print("FAILED: " + "; ".join(failures))
        sys.exit(1)
    print("OK")


if __name__ == "__main__":
    main()
