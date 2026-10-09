//! Loads the dpdf-voice VST2 DLL the way a host does and runs a 48 kHz WAV
//! through it with changing block sizes.
//!
//! Usage: dpdf-voice-hosttest <plugin.dll> <in.wav> <out.wav> [Model Limit Bypass]
//! Parameter values are normalized 0..1, as in an Equalizer APO config line.

mod eapo;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use vst::host::{Host, HostBuffer, PluginLoader};
use vst::plugin::Plugin;

struct TestHost;

impl Host for TestHost {
    fn automate(&self, _index: i32, _value: f32) {}
}

/// Block sizes cycled through, including ones that are not hop multiples.
const BLOCKS: [usize; 6] = [480, 441, 512, 128, 1024, 333];

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

/// Runs `f` on a thread with a stack of `DPDF_STACK_KB` kilobytes if set, to
/// mimic hosts such as the Windows audio service that call plugins on
/// threads with small stacks. Without the variable `f` runs inline.
fn on_host_thread<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    match std::env::var("DPDF_STACK_KB").ok().and_then(|v| v.parse::<usize>().ok()) {
        Some(kb) => std::thread::scope(|s| {
            std::thread::Builder::new()
                .stack_size(kb * 1024)
                .spawn_scoped(s, f)
                .expect("host thread")
                .join()
                .expect("host thread panicked")
        }),
        None => f(),
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        return Err("usage: dpdf-voice-hosttest [--eapo] <plugin.dll> <in.wav> <out.wav> [Model Limit Bypass]".into());
    }
    if args[1] == "--eapo" {
        let input = read_mono(&args[3])?;
        let output = eapo::run(&args[2], &input, &[("Model", 0.0), ("Limit", 1.0), ("Bypass", 0.0)])?;
        return write_mono(&args[4], &output);
    }

    let host = Arc::new(Mutex::new(TestHost));
    let mut loader =
        PluginLoader::load(Path::new(&args[1]), host).map_err(|e| format!("load: {e:?}"))?;
    let mut plugin = loader.instance().map_err(|e| format!("instance: {e:?}"))?;
    let info = plugin.get_info();
    println!(
        "plugin: {} | in/out: {}/{} | parameters: {} | delay: {} samples",
        info.name, info.inputs, info.outputs, info.parameters, info.initial_delay
    );

    plugin.init();
    let params = plugin.get_parameter_object();
    for (i, v) in args.iter().skip(4).take(3).enumerate() {
        params.set_parameter(i as i32, v.parse().map_err(|_| format!("invalid: {v}"))?);
    }
    for i in 0..info.parameters {
        println!("  {} = {} {}", params.get_parameter_name(i), params.get_parameter_text(i), params.get_parameter_label(i));
    }
    plugin.set_sample_rate(48000.0);
    plugin.set_block_size(*BLOCKS.iter().max().unwrap() as i64);
    let input = read_mono(&args[2])?;
    if let Ok(kb) = std::env::var("DPDF_STACK_KB") {
        println!("resume and process on a thread with {kb} KB stack");
    }
    // DPDF_RESUME_INLINE=1 keeps resume on the main thread, so only process
    // runs on the small stack.
    let resume_inline = std::env::var_os("DPDF_RESUME_INLINE").is_some();
    if resume_inline {
        plugin.resume();
    }
    let (output, elapsed, worst_ms) = on_host_thread(|| {
        // DPDF_MXCSR=<hex> sets the SSE control register of the processing
        // thread, to mimic audio hosts that change rounding or denormal modes.
        if let Some(csr) = std::env::var("DPDF_MXCSR").ok().and_then(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok()) {
            #[allow(deprecated)]
            unsafe {
                std::arch::x86_64::_mm_setcsr(csr)
            };
            println!("MXCSR = {csr:#06x}");
        }
        if !resume_inline {
            let t = Instant::now();
            plugin.resume();
            println!("resume (model load): {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
        }

        let mut output = Vec::with_capacity(input.len());
        let mut host_buffer: HostBuffer<f32> = HostBuffer::new(1, 1);
        let mut out_block = vec![0.0f32; *BLOCKS.iter().max().unwrap()];
        let mut worst_ms = 0.0f64;

        let start = Instant::now();
        let mut pos = 0;
        for &n in BLOCKS.iter().cycle() {
            if pos >= input.len() {
                break;
            }
            let n = n.min(input.len() - pos);
            let ins = [&input[pos..pos + n]];
            let mut outs = [&mut out_block[..n]];
            let mut buffer = host_buffer.bind(&ins, &mut outs);
            let t = Instant::now();
            plugin.process(&mut buffer);
            worst_ms = worst_ms.max(t.elapsed().as_secs_f64() * 1000.0);
            output.extend_from_slice(&out_block[..n]);
            pos += n;
        }
        let elapsed = start.elapsed().as_secs_f64();
        plugin.suspend();
        (output, elapsed, worst_ms)
    });

    write_mono(&args[3], &output)?;
    println!(
        "{:.1} s of audio, real-time factor {:.3}, slowest block {:.2} ms",
        input.len() as f64 / 48000.0,
        elapsed / (input.len() as f64 / 48000.0),
        worst_ms
    );
    Ok(())
}

fn read_mono(path: &str) -> Result<Vec<f32>, String> {
    let mut reader = hound::WavReader::open(path).map_err(|e| format!("{path}: {e}"))?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>(),
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().map(|s| s.map(|v| v as f32 * scale)).collect::<Result<_, _>>()
        }
    }
    .map_err(|e| format!("{path}: {e}"))?;
    let ch = spec.channels as usize;
    Ok(samples.chunks(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect())
}

fn write_mono(path: &str, samples: &[f32]) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec).map_err(|e| format!("{path}: {e}"))?;
    for &s in samples {
        writer.write_sample(s).map_err(|e| e.to_string())?;
    }
    writer.finalize().map_err(|e| e.to_string())
}
