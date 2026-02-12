use std::path::PathBuf;

use gsplat_core::{Camera, RenderMode};
use gsplat_io_ply::load_ply;
use gsplat_render_wgpu::Renderer;

fn dataset_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/datasets/minimal_ascii.ply")
        .canonicalize()
        .expect("dataset path")
}

#[test]
fn sorted_alpha_conformance_baseline() {
    let loaded = load_ply(&dataset_path()).expect("load test dataset");

    let mut renderer = Renderer::new(RenderMode::SortedAlpha).expect("renderer init");
    renderer.load_scene(loaded.scene).expect("scene load");

    let stats = renderer
        .render_frame(&Camera::default())
        .expect("render frame");

    assert_eq!(stats.visible_count, 2);
    assert_eq!(stats.drawn_count, 2);
    assert!(stats.frame_ms >= 0.0);
}
