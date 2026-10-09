//! Offline tool: denoise a 48 kHz WAV file with DPDFNet.
//!
//! Usage: dpdf-voice-cli <model.onnx> <in.wav> <out.wav> [attn_limit_db]
//!
//! ONNX Runtime is loaded from `onnxruntime.dll` next to this executable,
//! or from the path in `DPDF_ORT_DLL`. Windows ships its own
//! `onnxruntime.dll` in System32, so the library is always loaded by full path.
//! `DPDF_ENGINE` picks the engine: auto (default), onnx, native-int8, native-fp32.

use std::path::PathBuf;
use std::time::Instant;

use hushmic_denoiser::{init_runtime, Denoiser, Inference, StreamDenoiser, SAMPLE_RATE};

type Result<T> = std::result::Result<T, String>;

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        return Err("usage: dpdf-voice-cli <model.onnx> <in.wav> <out.wav> [attn_limit_db]".into());
    }
    let attn_db: f32 = match args.get(4) {
        Some(s) => s.parse().map_err(|_| format!("invalid dB value: {s}"))?,
        None => 100.0,
    };

    let ort = match std::env::var_os("DPDF_ORT_DLL") {
        Some(p) => PathBuf::from(p),
        None => std::env::current_exe()
            .map_err(|e| e.to_string())?
            .with_file_name("onnxruntime.dll"),
    };
    init_runtime(&ort).map_err(|e| format!("ONNX Runtime ({}): {e:?}", ort.display()))?;

    let input = read_mono(&args[2])?;
    let engine = std::env::var("DPDF_ENGINE").unwrap_or_else(|_| "auto".into());
    let inference = Inference::parse(&engine).ok_or(format!("unknown engine: {engine}"))?;
    let mut denoiser =
        Denoiser::from_file_with(&args[1], inference).map_err(|e| format!("model: {e:?}"))?;
    match denoiser.fallback_reason() {
        Some(why) => println!("engine: {} (native not used: {why})", denoiser.engine()),
        None => println!("engine: {}", denoiser.engine()),
    }
    denoiser.set_attenuation_limit_db(attn_db);
    let latency = denoiser.latency_samples();
    let mut stream = StreamDenoiser::new(denoiser);

    // Pad the tail so the delayed output covers the whole input.
    let mut padded = input.clone();
    padded.extend(std::iter::repeat_n(0.0, latency + hushmic_denoiser::HOP));

    let start = Instant::now();
    let mut output = Vec::with_capacity(padded.len());
    for chunk in padded.chunks(512) {
        output.extend_from_slice(stream.process(chunk));
        if let Some(e) = stream.take_error() {
            return Err(format!("processing: {e:?}"));
        }
    }
    let elapsed = start.elapsed().as_secs_f64();

    // Drop the algorithmic delay so output lines up with input.
    let aligned: Vec<f32> = output.into_iter().skip(latency).take(input.len()).collect();
    write_mono(&args[3], &aligned)?;

    let audio_secs = input.len() as f64 / SAMPLE_RATE as f64;
    println!(
        "{:.1} s of audio in {:.2} s (real-time factor {:.3}, latency {} ms, limit {} dB)",
        audio_secs,
        elapsed,
        elapsed / audio_secs,
        latency * 1000 / SAMPLE_RATE as usize,
        attn_db
    );
    Ok(())
}

/// Reads a 48 kHz WAV and mixes all channels down to mono f32.
fn read_mono(path: &str) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path).map_err(|e| format!("{path}: {e}"))?;
    let spec = reader.spec();
    if spec.sample_rate != SAMPLE_RATE {
        return Err(format!("{path}: {} Hz, needs 48000 Hz", spec.sample_rate));
    }
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<std::result::Result<_, _>>(),
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 * scale))
                .collect::<std::result::Result<_, _>>()
        }
    }
    .map_err(|e| format!("{path}: {e}"))?;
    let ch = spec.channels as usize;
    Ok(samples.chunks(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect())
}

fn write_mono(path: &str, samples: &[f32]) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec).map_err(|e| format!("{path}: {e}"))?;
    for &s in samples {
        writer.write_sample(s).map_err(|e| e.to_string())?;
    }
    writer.finalize().map_err(|e| e.to_string())
}
