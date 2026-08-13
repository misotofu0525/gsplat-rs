//! Frame timers and Android-safe wgpu labels.

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) type TimerInstant = Instant;

#[cfg(target_arch = "wasm32")]
pub(crate) type TimerInstant = f64;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn timer_now() -> TimerInstant {
    Instant::now()
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn timer_now() -> TimerInstant {
    js_sys::Date::now()
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn timer_elapsed_ms(start: TimerInstant) -> f32 {
    start.elapsed().as_secs_f32() * 1000.0
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn timer_elapsed_ms(start: TimerInstant) -> f32 {
    (js_sys::Date::now() - start).max(0.0) as f32
}

#[cfg(target_os = "android")]
pub(crate) const fn wgpu_label(_label: &'static str) -> Option<&'static str> {
    None
}

#[cfg(not(target_os = "android"))]
pub(crate) const fn wgpu_label(label: &'static str) -> Option<&'static str> {
    Some(label)
}
