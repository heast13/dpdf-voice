//! dpdf-voice: DPDFNet noise suppression as a mono VST2 effect.
//!
//! Expected next to the DLL: `onnxruntime.dll`, `dpdfnet8_48khz_hr.onnx`
//! (quality) and `dpdfnet2_48khz_hr.onnx` (light). With the matching
//! `<model>.weights.f32` files present, the native INT8 engine runs instead
//! of ONNX Runtime at about half the CPU.
//!
//! The plugin runs inside the host's audio thread (for Equalizer APO that is
//! the Windows audio service), so it never panics across the FFI boundary and
//! falls back to passing audio through whenever it cannot denoise: wrong
//! sample rate, too little stack, missing files or a failed model load.

mod paths;

use std::collections::VecDeque;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

use hushmic_denoiser::{init_runtime, Denoiser, Inference, Mode, StreamDenoiser, HOP, SAMPLE_RATE};
use vst::prelude::*;

const PARAM_MODEL: i32 = 0;
const PARAM_LIMIT: i32 = 1;
const PARAM_BYPASS: i32 = 2;
const PARAM_COUNT: i32 = 3;

/// Stack an audio block needs: ONNX Runtime inference measured at 64-80 KB,
/// plus headroom for the host's own frames.
const MIN_PROCESS_STACK: usize = 192 * 1024;

/// Stack for the loader thread. Creating the ONNX Runtime session needs more
/// than 128 KB, while hosts like the Windows audio service call into plugins
/// on threads with small stacks, so loading always runs on its own thread.
const LOADER_STACK: usize = 8 * 1024 * 1024;

/// Upper end of the attenuation limit; 100 dB means effectively unlimited,
/// which is HushMic's default.
const MAX_LIMIT_DB: f32 = 100.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Model {
    Quality,
    Light,
}

impl Model {
    fn from_value(v: f32) -> Model {
        if v < 0.5 {
            Model::Quality
        } else {
            Model::Light
        }
    }

    fn file_name(self) -> &'static str {
        match self {
            Model::Quality => "dpdfnet8_48khz_hr.onnx",
            Model::Light => "dpdfnet2_48khz_hr.onnx",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Model::Quality => "quality",
            Model::Light => "light",
        }
    }
}

/// Host-visible parameters, stored as normalized 0..1 values.
struct Params {
    model: AtomicF32,
    limit: AtomicF32,
    bypass: AtomicF32,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            model: AtomicF32::new(0.0),
            limit: AtomicF32::new(1.0),
            bypass: AtomicF32::new(0.0),
        }
    }
}

impl Params {
    fn slot(&self, index: i32) -> Option<&AtomicF32> {
        match index {
            PARAM_MODEL => Some(&self.model),
            PARAM_LIMIT => Some(&self.limit),
            PARAM_BYPASS => Some(&self.bypass),
            _ => None,
        }
    }

    fn limit_db(&self) -> f32 {
        self.limit.get() * MAX_LIMIT_DB
    }

    fn bypassed(&self) -> bool {
        self.bypass.get() >= 0.5
    }
}

impl PluginParameters for Params {
    fn get_parameter(&self, index: i32) -> f32 {
        self.slot(index).map_or(0.0, AtomicF32::get)
    }

    fn set_parameter(&self, index: i32, value: f32) {
        if let Some(slot) = self.slot(index) {
            slot.set(value.clamp(0.0, 1.0));
        }
    }

    fn get_parameter_name(&self, index: i32) -> String {
        match index {
            PARAM_MODEL => "Model",
            PARAM_LIMIT => "Limit",
            PARAM_BYPASS => "Bypass",
            _ => "",
        }
        .into()
    }

    fn get_parameter_text(&self, index: i32) -> String {
        match index {
            PARAM_MODEL => Model::from_value(self.model.get()).label().into(),
            PARAM_LIMIT => format!("{:.0}", self.limit_db()),
            PARAM_BYPASS => if self.bypassed() { "on" } else { "off" }.into(),
            _ => String::new(),
        }
    }

    fn get_parameter_label(&self, index: i32) -> String {
        if index == PARAM_LIMIT { "dB" } else { "" }.into()
    }
}

struct DpdfVoice {
    params: Arc<Params>,
    sample_rate: f32,
    stream: Option<StreamDenoiser>,
    loaded_model: Option<Model>,
    /// Set after a failed load so the audio thread does not retry every block.
    load_failed: Option<Model>,
    /// Denoised samples waiting to be handed out. Primed with one hop of
    /// silence so every block can be filled although the engine only emits
    /// whole hops.
    out_fifo: VecDeque<f32>,
    /// Result of the last stack check, so a change is logged only once.
    stack_ok: Option<bool>,
    first_block_logged: bool,
}

impl DpdfVoice {
    fn wanted_model(&self) -> Model {
        Model::from_value(self.params.model.get())
    }

    /// Loads the requested model unless it is already active. Runs from
    /// `resume`, outside the audio thread, and only as a fallback from
    /// `process`.
    fn ensure_model(&mut self) {
        let want = self.wanted_model();
        if self.loaded_model == Some(want) || self.load_failed == Some(want) {
            return;
        }
        match load_denoiser(want) {
            Ok(denoiser) => {
                paths::log(&format!("model {} loaded, engine {}", want.label(), denoiser.engine()));
                if let Some(why) = denoiser.fallback_reason() {
                    paths::log(&format!("native engine not used: {why}"));
                }
                self.stream = Some(StreamDenoiser::new(denoiser));
                self.loaded_model = Some(want);
                self.load_failed = None;
                self.reset_fifo();
            }
            Err(e) => {
                paths::log(&format!("model {} failed to load, passing audio through: {e}", want.label()));
                self.load_failed = Some(want);
            }
        }
    }

    fn reset_fifo(&mut self) {
        self.out_fifo.clear();
        self.out_fifo.extend(std::iter::repeat(0.0).take(HOP));
        if let Some(stream) = self.stream.as_mut() {
            stream.reset();
        }
    }

    /// Whether the calling thread has enough stack for inference, logging
    /// whenever the answer changes.
    fn stack_sufficient(&mut self) -> bool {
        let free = paths::stack_remaining();
        let ok = free >= MIN_PROCESS_STACK;
        if self.stack_ok != Some(ok) {
            self.stack_ok = Some(ok);
            if !ok {
                paths::log(&format!(
                    "audio thread has only {} KB stack left (needs {} KB), passing audio through",
                    free / 1024,
                    MIN_PROCESS_STACK / 1024
                ));
            }
        }
        ok
    }

    fn process_mono(&mut self, input: &[f32], output: &mut [f32]) {
        if self.sample_rate as u32 != SAMPLE_RATE || !self.stack_sufficient() {
            output.copy_from_slice(input);
            return;
        }
        if self.stream.is_none() {
            self.ensure_model();
        }
        let params = Arc::clone(&self.params);
        let Some(stream) = self.stream.as_mut() else {
            output.copy_from_slice(input);
            return;
        };

        let denoiser = stream.denoiser_mut();
        denoiser.set_mode(if params.bypassed() { Mode::Bypass } else { Mode::Process });
        let limit = params.limit_db();
        if denoiser.attenuation_limit_db() != limit {
            denoiser.set_attenuation_limit_db(limit);
        }

        self.out_fifo.extend(stream.process(input));
        if let Some(e) = stream.take_error() {
            paths::log(&format!("inference error: {e:?}"));
        }
        for (dst, src) in output.iter_mut().zip(self.out_fifo.drain(..input.len())) {
            *dst = src;
        }
    }
}

impl Plugin for DpdfVoice {
    fn new(_host: HostCallback) -> Self {
        DpdfVoice {
            params: Arc::new(Params::default()),
            sample_rate: SAMPLE_RATE as f32,
            stream: None,
            loaded_model: None,
            load_failed: None,
            out_fifo: VecDeque::with_capacity(HOP + 8192),
            stack_ok: None,
            first_block_logged: false,
        }
    }

    fn get_info(&self) -> Info {
        Info {
            name: "dpdf-voice".into(),
            vendor: "heast13".into(),
            unique_id: i32::from_be_bytes(*b"DpdV"),
            version: 10,
            inputs: 1,
            outputs: 1,
            parameters: PARAM_COUNT,
            category: Category::Effect,
            initial_delay: (hushmic_denoiser::LATENCY_SAMPLES + HOP) as i32,
            ..Default::default()
        }
    }

    fn set_sample_rate(&mut self, rate: f32) {
        self.sample_rate = rate;
        if rate as u32 != SAMPLE_RATE {
            paths::log(&format!("{rate} Hz, needs 48000 Hz, passing audio through"));
        }
    }

    fn set_block_size(&mut self, size: i64) {
        let need = HOP + size.max(0) as usize;
        if self.out_fifo.capacity() < need {
            self.out_fifo.reserve(need - self.out_fifo.len());
        }
    }

    fn resume(&mut self) {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            self.ensure_model();
            self.reset_fifo();
        }));
    }

    fn process(&mut self, buffer: &mut AudioBuffer<f32>) {
        let (inputs, mut outputs) = buffer.split();
        if inputs.len() == 0 || outputs.len() == 0 {
            return;
        }
        let input = inputs.get(0);
        let output = outputs.get_mut(0);
        let n = input.len().min(output.len());
        if !self.first_block_logged {
            self.first_block_logged = true;
            paths::log(&format!(
                "processing: {n} samples per block, {} KB stack free",
                paths::stack_remaining() / 1024
            ));
        }
        let result = catch_unwind(AssertUnwindSafe(|| self.process_mono(&input[..n], &mut output[..n])));
        if let Err(payload) = result {
            // Engine state is unknown after a panic: drop it and pass audio through.
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_default();
            paths::log(&format!("panic in audio thread, passing audio through: {msg}"));
            self.stream = None;
            self.load_failed = self.loaded_model.take();
            output[..n].copy_from_slice(&input[..n]);
        }
    }

    fn get_parameter_object(&mut self) -> Arc<dyn PluginParameters> {
        Arc::clone(&self.params) as Arc<dyn PluginParameters>
    }
}

/// ONNX Runtime is process-global: initialize it once from the DLL folder.
/// Windows ships its own `onnxruntime.dll` in System32, so the bundled copy
/// is always loaded by full path.
fn ensure_runtime() -> Result<(), String> {
    static RUNTIME: OnceLock<Result<(), String>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            let dir = paths::plugin_dir().ok_or("plugin folder not found")?;
            let dll = dir.join("onnxruntime.dll");
            init_runtime(&dll).map(|_| ()).map_err(|e| format!("{}: {e:?}", dll.display()))
        })
        .clone()
}

fn load_denoiser(model: Model) -> Result<Denoiser, String> {
    std::thread::Builder::new()
        .name("dpdf-voice-loader".into())
        .stack_size(LOADER_STACK)
        .spawn(move || {
            ensure_runtime()?;
            let dir = paths::plugin_dir().ok_or("plugin folder not found")?;
            let path = dir.join(model.file_name());
            // Native INT8 engine when the CPU has AVX2/FMA and the model's
            // .weights.f32 file is present and intact, ONNX Runtime otherwise.
            Denoiser::from_file_with(&path, Inference::Auto).map_err(|e| format!("{}: {e:?}", path.display()))
        })
        .map_err(|e| format!("loader thread: {e}"))?
        .join()
        .map_err(|_| "loader thread panicked".to_string())?
}

/// Lock-free f32 cell for parameters shared between host and audio thread.
struct AtomicF32(AtomicU32);

impl AtomicF32 {
    fn new(v: f32) -> Self {
        AtomicF32(AtomicU32::new(v.to_bits()))
    }

    fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed);
    }
}

vst::plugin_main!(DpdfVoice);
