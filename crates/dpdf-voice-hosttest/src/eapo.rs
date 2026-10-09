//! Replays the exact VST2 call sequence of Equalizer APO 1.4
//! (VSTPluginInstance.cpp / VSTPluginFilter.cpp) against a plugin DLL,
//! including the host overwriting `AEffect::user`.

use std::ffi::{c_void, CStr, OsStr};
use std::os::windows::ffi::OsStrExt;

use vst::api::AEffect;

const EFF_OPEN: i32 = 0;
const EFF_CLOSE: i32 = 1;
const EFF_GET_PARAM_NAME: i32 = 8;
const EFF_SET_SAMPLE_RATE: i32 = 10;
const EFF_SET_BLOCK_SIZE: i32 = 11;
const EFF_MAINS_CHANGED: i32 = 12;
const EFF_START_PROCESS: i32 = 71;
const EFF_STOP_PROCESS: i32 = 72;
const AUDIO_MASTER_VERSION: i32 = 1;
const K_EFFECT_MAGIC: i32 = i32::from_be_bytes(*b"VstP");

/// Equalizer APO's capture block: 10 ms at 48 kHz.
const BLOCK: usize = 480;

type MainProc = extern "C" fn(callback: HostCallback) -> *mut AEffect;
type HostCallback = extern "C" fn(*mut AEffect, i32, i32, isize, *mut c_void, f32) -> isize;

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
}

extern "C" fn host_callback(_e: *mut AEffect, opcode: i32, _i: i32, _v: isize, _p: *mut c_void, _o: f32) -> isize {
    if opcode == AUDIO_MASTER_VERSION {
        2400
    } else {
        0
    }
}

/// Runs `input` through the plugin like Equalizer APO with the given
/// `Name value` parameters; returns the output.
pub fn run(dll: &str, input: &[f32], params: &[(&str, f32)]) -> Result<Vec<f32>, String> {
    let wide: Vec<u16> = OsStr::new(dll).encode_wide().chain(Some(0)).collect();
    let module = unsafe { LoadLibraryW(wide.as_ptr()) };
    if module.is_null() {
        return Err(format!("LoadLibrary failed: {dll}"));
    }
    let main = unsafe { GetProcAddress(module, c"VSTPluginMain".as_ptr() as *const u8) };
    if main.is_null() {
        return Err("VSTPluginMain missing".into());
    }
    let main: MainProc = unsafe { std::mem::transmute(main) };

    // VSTPluginInstance::initialize
    let effect = main(host_callback);
    if effect.is_null() {
        return Err("VSTPluginMain returned NULL".into());
    }
    let e = unsafe { &mut *effect };
    e.user = 0xDEAD_BEEF_usize as *mut c_void; // effect->user = this;
    if e.magic != K_EFFECT_MAGIC {
        return Err("wrong magic".into());
    }
    let dispatch = |op: i32, index: i32, value: isize, ptr: *mut c_void, opt: f32| unsafe {
        ((*effect).dispatcher)(effect, op, index, value, ptr, opt)
    };
    dispatch(EFF_OPEN, 0, 0, std::ptr::null_mut(), 0.0);
    println!("EAPO sequence: initialize ok ({} in, {} out, {} parameters)", e.numInputs, e.numOutputs, e.numParams);

    // VSTPluginInstance::prepareForProcessing
    dispatch(EFF_SET_SAMPLE_RATE, 0, 0, std::ptr::null_mut(), 48000.0);
    dispatch(EFF_SET_BLOCK_SIZE, 0, BLOCK as isize, std::ptr::null_mut(), 0.0);

    // VSTPluginInstance::writeToEffect (parameter path, no chunks)
    for i in 0..e.numParams {
        let mut buf = [0u8; 256];
        dispatch(EFF_GET_PARAM_NAME, i, 0, buf.as_mut_ptr() as *mut c_void, 0.0);
        buf[255] = 0;
        let name = CStr::from_bytes_until_nul(&buf).map_err(|e| e.to_string())?.to_string_lossy();
        if let Some(&(_, v)) = params.iter().find(|(n, _)| *n == name) {
            unsafe { ((*effect).setParameter)(effect, i, v) };
            println!("  {name} = {v}");
        }
    }

    // VSTPluginInstance::startProcessing
    dispatch(EFF_MAINS_CHANGED, 0, 1, std::ptr::null_mut(), 0.0);
    dispatch(EFF_START_PROCESS, 0, 0, std::ptr::null_mut(), 0.0);
    println!("EAPO sequence: prepareForProcessing ok");

    let mut output = Vec::with_capacity(input.len());
    let mut out_block = [0f32; BLOCK];
    for chunk in input.chunks(BLOCK) {
        let ins = [chunk.as_ptr()];
        let mut outs = [out_block.as_mut_ptr()];
        unsafe { ((*effect).processReplacing)(effect, ins.as_ptr(), outs.as_mut_ptr(), chunk.len() as i32) };
        output.extend_from_slice(&out_block[..chunk.len()]);
    }

    // VSTPluginInstance::stopProcessing and cleanup
    dispatch(EFF_STOP_PROCESS, 0, 0, std::ptr::null_mut(), 0.0);
    dispatch(EFF_MAINS_CHANGED, 0, 0, std::ptr::null_mut(), 0.0);
    dispatch(EFF_CLOSE, 0, 0, std::ptr::null_mut(), 0.0);
    println!("EAPO sequence: processing and close ok");
    Ok(output)
}
