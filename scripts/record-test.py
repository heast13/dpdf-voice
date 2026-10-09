"""Record a 48 kHz mono 16-bit WAV from a microphone for offline tests.

Usage: python record-test.py <seconds> <out.wav> [device name part]

Uses the Windows waveIn API through ctypes, so no extra packages are needed.
Note: the capture goes through Windows, so Equalizer APO filters on this
microphone are applied to the recording.
"""
import ctypes
import ctypes.wintypes as wt
import sys
import time
import wave

RATE, CHANNELS, BITS = 48000, 1, 16
WAVE_MAPPER = 0xFFFFFFFF
WHDR_DONE = 0x1

winmm = ctypes.WinDLL("winmm")


class WAVEFORMATEX(ctypes.Structure):
    _fields_ = [("wFormatTag", wt.WORD), ("nChannels", wt.WORD), ("nSamplesPerSec", wt.DWORD),
                ("nAvgBytesPerSec", wt.DWORD), ("nBlockAlign", wt.WORD), ("wBitsPerSample", wt.WORD),
                ("cbSize", wt.WORD)]


class WAVEHDR(ctypes.Structure):
    _fields_ = [("lpData", ctypes.c_void_p), ("dwBufferLength", wt.DWORD), ("dwBytesRecorded", wt.DWORD),
                ("dwUser", ctypes.c_size_t), ("dwFlags", wt.DWORD), ("dwLoops", wt.DWORD),
                ("lpNext", ctypes.c_void_p), ("reserved", ctypes.c_size_t)]


class WAVEINCAPSW(ctypes.Structure):
    _fields_ = [("wMid", wt.WORD), ("wPid", wt.WORD), ("vDriverVersion", wt.UINT),
                ("szPname", ctypes.c_wchar * 32), ("dwFormats", wt.DWORD), ("wChannels", wt.WORD),
                ("wReserved1", wt.WORD)]


def find_device(part):
    for i in range(winmm.waveInGetNumDevs()):
        caps = WAVEINCAPSW()
        winmm.waveInGetDevCapsW(i, ctypes.byref(caps), ctypes.sizeof(caps))
        if part.lower() in caps.szPname.lower():
            return i, caps.szPname
    raise SystemExit(f"Kein Aufnahmegeraet mit '{part}' gefunden")


def check(res, what):
    if res != 0:
        raise SystemExit(f"{what} fehlgeschlagen (MMRESULT {res})")


def main():
    if len(sys.argv) < 3:
        raise SystemExit(__doc__)
    seconds, out = float(sys.argv[1]), sys.argv[2]
    dev, name = find_device(sys.argv[3]) if len(sys.argv) > 3 else (WAVE_MAPPER, "Standardgeraet")

    fmt = WAVEFORMATEX(1, CHANNELS, RATE, RATE * CHANNELS * BITS // 8, CHANNELS * BITS // 8, BITS, 0)
    handle = wt.HANDLE()
    check(winmm.waveInOpen(ctypes.byref(handle), dev, ctypes.byref(fmt), 0, 0, 0), "waveInOpen")

    size = int(seconds * fmt.nAvgBytesPerSec)
    buf = ctypes.create_string_buffer(size)
    hdr = WAVEHDR(ctypes.cast(buf, ctypes.c_void_p), size, 0, 0, 0, 0, None, 0)
    check(winmm.waveInPrepareHeader(handle, ctypes.byref(hdr), ctypes.sizeof(hdr)), "waveInPrepareHeader")
    check(winmm.waveInAddBuffer(handle, ctypes.byref(hdr), ctypes.sizeof(hdr)), "waveInAddBuffer")

    print(f"Aufnahme von '{name}' fuer {seconds:.0f} s ... jetzt sprechen")
    check(winmm.waveInStart(handle), "waveInStart")
    while not hdr.dwFlags & WHDR_DONE:
        time.sleep(0.1)
    winmm.waveInStop(handle)
    winmm.waveInUnprepareHeader(handle, ctypes.byref(hdr), ctypes.sizeof(hdr))
    winmm.waveInClose(handle)

    with wave.open(out, "wb") as w:
        w.setnchannels(CHANNELS)
        w.setsampwidth(BITS // 8)
        w.setframerate(RATE)
        w.writeframes(buf.raw[:hdr.dwBytesRecorded])
    print(f"Gespeichert: {out}")


if __name__ == "__main__":
    main()
