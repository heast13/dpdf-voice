# Local changes to vst 0.4.0

Source: https://crates.io/crates/vst/0.4.0 (MIT, archived upstream at
https://github.com/RustAudio/vst-rs).

## Plugin cache moved out of `AEffect::user`

vst-rs stored its `PluginCache` (info, parameter object, editor) in
`AEffect::user`. The VST 2.4 SDK reserves that field for the host, and
Equalizer APO writes its own pointer there right after `VSTPluginMain`
(`effect->user = this`). The next dispatcher call that needs the cache, for
example `effGetParamName`, then dereferences the host's object and crashes.

The plugin and its cache now share one allocation behind `AEffect::object`
(`PluginObject` in `src/lib.rs`, accessors in `src/api.rs`). `user` is left
untouched for the host. `main` now requires `T: Plugin + 'static`, which the
boxed trait object needs.

## Lint allowances

`src/lib.rs` allows `useless_ptr_null_checks` and
`mismatched_lifetime_syntaxes`, which newer compilers raise in unchanged
upstream code.
