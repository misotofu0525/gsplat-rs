use std::path::PathBuf;

use gsplat_core::{Camera, RenderMode, RendererConfig};
use gsplat_io_ply::load_ply;
use gsplat_render_wgpu::{Renderer, RendererError};

fn dataset_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/datasets/minimal_ascii.ply")
        .canonicalize()
        .expect("dataset path")
}

#[test]
fn sorted_alpha_conformance_baseline() {
    let loaded = load_ply(&dataset_path()).expect("load test dataset");

    let config = RendererConfig {
        width: 64,
        height: 64,
        mode: RenderMode::SortedAlpha,
    };
    let require_gpu = matches!(
        std::env::var("GSPLAT_REQUIRE_GPU_CONFORMANCE").as_deref(),
        Ok("1")
    );
    let mut renderer = match Renderer::with_config(config) {
        Ok(renderer) => renderer,
        Err(error)
            if !require_gpu
                && matches!(
                    error,
                    RendererError::GpuRasterizerUnavailable | RendererError::GpuDeviceCreation
                ) =>
        {
            eprintln!("skipping GPU conformance on unavailable adapter: {error}");
            return;
        }
        Err(error) => panic!("renderer init: {error}"),
    };
    renderer.load_scene(loaded.scene).expect("scene load");

    let stats = renderer
        .render_frame(&Camera::default())
        .expect("render frame");

    assert_eq!(stats.visible_count, 2);
    assert_eq!(stats.drawn_count, 2);
    assert!(stats.frame_ms >= 0.0);

    let rgba = renderer.readback_rgba8().expect("RGBA readback");
    let config = renderer.config();
    assert_eq!(
        rgba.len(),
        config.width as usize * config.height as usize * 4
    );
    assert!(
        rgba.chunks_exact(4).any(|pixel| pixel[3] > 0),
        "successful render must produce at least one non-transparent pixel"
    );

    let mut channel_sums = [0_u64; 4];
    for pixel in rgba.chunks_exact(4) {
        for (sum, value) in channel_sums.iter_mut().zip(pixel) {
            *sum += u64::from(*value);
        }
    }
    let denominator = (config.width as f32) * (config.height as f32) * 255.0;
    let channel_means = channel_sums.map(|sum| sum as f32 / denominator);
    let expected_means = [0.4194_f32, 0.3234, 0.3073, 0.5712];
    for (channel, (actual, expected)) in channel_means.into_iter().zip(expected_means).enumerate() {
        assert!(
            (actual - expected).abs() <= 0.035,
            "channel {channel} mean {actual:.4} exceeded tolerance around {expected:.4}"
        );
    }
}
