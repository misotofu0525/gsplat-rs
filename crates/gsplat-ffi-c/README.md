# gsplat-ffi-c

Small, stable C ABI over the `gsplat-rs` renderer and mobile Surface
presenters.

The public contract is the header at [`include/gsplat.h`](include/gsplat.h).
The crate builds as `staticlib`, `cdylib`, and `rlib`, and is the integration
boundary used by the Android JNI bridge and the iOS `GsplatKit` wrapper.

## Usage rules

- Use `gsplat_config_default()` and `gsplat_camera_default()` instead of
  hand-writing ABI defaults.
- Use `GSPLAT_RENDER_MODE_SORTED_ALPHA`; it is the only release-gated render
  mode in v0.1.
- Treat non-zero returns as `GsplatErrorCode` values and pass them to
  `gsplat_error_message()`; use `gsplat_last_error_message()` for the most
  recent operation detail.
- Native Surface handles are single-owner: serialize all access through one
  thread or queue.

## Keeping the ABI in sync

Any ABI change must update both `src/lib.rs` and `include/gsplat.h`, then pass
the FFI smoke path:

```bash
bash tests/ffi/run-ffi-smoke.sh
```

## License

MIT OR Apache-2.0, at your option.
