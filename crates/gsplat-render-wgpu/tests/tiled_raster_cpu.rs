#[cfg(not(target_arch = "wasm32"))]
#[path = "../src/tiled_gpu.rs"]
mod tiled_gpu;
#[path = "../src/tiled_raster.rs"]
mod tiled_raster;

use std::collections::BTreeSet;

use gsplat_core::{Camera, CameraIntrinsics, CameraPose, RenderMode, RendererConfig, Vec3f};
use tiled_raster::{
    ALPHA_THRESHOLD, ProjectedSplat, TILE_HEIGHT, TILE_WIDTH, TiledRasterError, TiledSourceSplat,
    build_tile_bins, project_sources, rasterize_naive_back_to_front, rasterize_tiles_front_to_back,
    sample_alpha,
};

fn config(width: u32, height: u32) -> RendererConfig {
    RendererConfig {
        width,
        height,
        mode: RenderMode::SortedAlpha,
    }
}

fn camera() -> Camera {
    Camera {
        pose: CameraPose {
            position: Vec3f::new(0.0, 0.0, 0.0),
            rotation_xyzw: [0.0, 0.0, 0.0, 1.0],
        },
        intrinsics: CameraIntrinsics {
            vertical_fov_radians: 60.0_f32.to_radians(),
            near_plane: 0.1,
            far_plane: 100.0,
        },
    }
}

fn source(
    position_world: [f32; 3],
    covariance_world: [f32; 6],
    opacity: f32,
    color_rgb: [f32; 3],
) -> TiledSourceSplat {
    TiledSourceSplat {
        position_world,
        covariance_world,
        opacity,
        color_rgb,
    }
}

fn isotropic(
    position_world: [f32; 3],
    sigma: f32,
    opacity: f32,
    color_rgb: [f32; 3],
) -> TiledSourceSplat {
    let variance = sigma * sigma;
    source(
        position_world,
        [variance, 0.0, 0.0, variance, 0.0, variance],
        opacity,
        color_rgb,
    )
}

#[test]
fn projected_and_entry_layouts_are_gpu_storage_compatible() {
    assert_eq!(std::mem::size_of::<ProjectedSplat>(), 80);
    assert_eq!(std::mem::size_of::<tiled_raster::TileContribution>(), 16);
}

#[test]
fn projection_keeps_large_offscreen_support_and_culls_only_zero_contribution() {
    let sources = [
        isotropic([0.0, 0.0, 2.0], 0.15, 0.8, [-0.25, 0.4, 0.7]),
        // Peak alpha is below the contribution threshold everywhere.
        isotropic(
            [0.0, 0.0, 2.0],
            0.15,
            ALPHA_THRESHOLD * 0.5,
            [1.0, 0.0, 0.0],
        ),
        // Outside the near plane.
        isotropic([0.0, 0.0, 0.05], 0.15, 0.8, [1.0, 0.0, 0.0]),
        // Center is well outside the viewport, but the unbounded covariance
        // still overlaps framebuffer samples and must not be dropped.
        isotropic([4.0, 0.0, 2.0], 2.0, 0.9, [0.2, 0.6, 0.1]),
    ];
    let projected = project_sources(&sources, &camera(), config(64, 48)).unwrap();
    let source_ids = projected
        .splats
        .iter()
        .map(|splat| splat.source_id())
        .collect::<Vec<_>>();
    assert_eq!(projected.source_count, 4);
    assert_eq!(source_ids, vec![0, 3]);
    assert_eq!(projected.splats[0].color_rgb(), [0.0, 0.4, 0.7]);
    assert_eq!(projected.splats[0].depth(), 2.0);
    assert!(projected.splats[0].qmax() > 0.0);

    let bins = build_tile_bins(&projected).unwrap();
    assert!(bins.entries.iter().any(|entry| entry.source_id == 3));
}

#[test]
fn every_emitted_tile_and_only_emitted_tiles_have_a_contributing_pixel() {
    let sources = [
        isotropic([-0.45, 0.32, 2.0], 0.23, 0.75, [1.0, 0.0, 0.0]),
        source(
            [0.1, -0.18, 2.4],
            [0.07, 0.035, 0.0, 0.045, 0.0, 0.025],
            0.62,
            [0.0, 1.0, 0.0],
        ),
        source(
            [0.58, 0.16, 3.1],
            [0.025, -0.018, 0.0, 0.08, 0.0, 0.03],
            0.91,
            [0.0, 0.0, 1.0],
        ),
        isotropic([0.0, 0.0, 1.4], 0.04, 0.4, [0.8, 0.8, 0.2]),
    ];
    let projected = project_sources(&sources, &camera(), config(35, 33)).unwrap();
    let bins = build_tile_bins(&projected).unwrap();
    let actual = bins
        .entries
        .iter()
        .map(|entry| (entry.tile_id, entry.source_id))
        .collect::<BTreeSet<_>>();

    let mut expected = BTreeSet::new();
    for splat in projected.splats.iter().copied() {
        for tile_y in 0..bins.tiles_y {
            for tile_x in 0..bins.tiles_x {
                let tile_id = tile_y * bins.tiles_x + tile_x;
                let min_x = tile_x * TILE_WIDTH;
                let min_y = tile_y * TILE_HEIGHT;
                let max_x = (min_x + TILE_WIDTH).min(projected.width);
                let max_y = (min_y + TILE_HEIGHT).min(projected.height);
                let contributes = (min_y..max_y)
                    .any(|y| (min_x..max_x).any(|x| sample_alpha(splat, x, y) >= ALPHA_THRESHOLD));
                if contributes {
                    expected.insert((tile_id, splat.source_id()));
                }
            }
        }
    }

    assert_eq!(actual, expected);
    assert_eq!(bins.entries.len(), actual.len());
}

#[test]
fn fixed_three_sigma_bbox_does_not_leak_to_the_rest_of_a_boundary_tile() {
    let projected = project_sources(
        &[isotropic([-0.9, 0.0, 2.0], 0.42, 1.0, [1.0, 0.3, 0.1])],
        &camera(),
        config(64, 48),
    )
    .unwrap();
    let splat = projected.splats[0];
    assert_eq!(splat.qmax(), 9.0);
    let bbox = splat.bbox();
    let center = splat.screen_center();
    let conic = splat.inverse_conic();
    let mut threshold_tail_outside_bbox = None;
    'outer: for y in 0..projected.height {
        for x in 0..projected.width {
            if x >= bbox[0] && x < bbox[2] && y >= bbox[1] && y < bbox[3] {
                continue;
            }
            let dx = x as f32 + 0.5 - center[0];
            let dy = y as f32 + 0.5 - center[1];
            let q = conic[0] * dx * dx + 2.0 * conic[1] * dx * dy + conic[2] * dy * dy;
            let unbounded_alpha = (splat.opacity() * (-0.5 * q).exp()).min(0.99);
            if unbounded_alpha >= ALPHA_THRESHOLD {
                threshold_tail_outside_bbox = Some((x, y));
                break 'outer;
            }
        }
    }
    let (x, y) = threshold_tail_outside_bbox
        .expect("high-opacity Gaussian should have a theoretical >3sigma threshold tail");
    assert_eq!(sample_alpha(splat, x, y), 0.0);

    let bins = build_tile_bins(&projected).unwrap();
    let tiled = rasterize_tiles_front_to_back(&projected, &bins).unwrap();
    assert_eq!(
        tiled.rgba[y as usize * projected.width as usize + x as usize],
        [0.0; 4]
    );
}

#[test]
fn tile_entries_are_stably_sorted_by_complete_tuple() {
    let sources = [
        isotropic([0.0, 0.0, 2.0], 0.35, 0.8, [1.0, 0.0, 0.0]),
        isotropic([0.02, 0.0, 2.0], 0.35, 0.8, [0.0, 1.0, 0.0]),
        isotropic([0.0, 0.0, 1.5], 0.35, 0.8, [0.0, 0.0, 1.0]),
        isotropic([0.0, 0.0, 3.0], 0.35, 0.8, [1.0, 1.0, 0.0]),
    ];
    let projected = project_sources(&sources, &camera(), config(31, 31)).unwrap();
    let bins = build_tile_bins(&projected).unwrap();

    assert!(bins.entries.windows(2).all(|entries| {
        let left = (
            entries[0].tile_id,
            entries[0].depth_key,
            !entries[0].source_id,
        );
        let right = (
            entries[1].tile_id,
            entries[1].depth_key,
            !entries[1].source_id,
        );
        left <= right
    }));
    for tile_id in 0..bins.tiles_x * bins.tiles_y {
        let equal_depth_ids = bins
            .entries
            .iter()
            .filter(|entry| entry.tile_id == tile_id && entry.depth_key == 2.0_f32.to_bits())
            .map(|entry| entry.source_id)
            .collect::<Vec<_>>();
        if !equal_depth_ids.is_empty() {
            assert_eq!(equal_depth_ids, vec![1, 0]);
        }
    }
}

#[test]
fn equal_depth_tie_matches_current_back_to_front_source_order() {
    let sources = [
        isotropic([0.0, 0.0, 2.0], 0.3, 0.6, [1.0, 0.0, 0.0]),
        isotropic([0.0, 0.0, 2.0], 0.3, 0.6, [0.0, 1.0, 0.0]),
    ];
    let projected = project_sources(&sources, &camera(), config(17, 17)).unwrap();
    let bins = build_tile_bins(&projected).unwrap();
    let tiled = rasterize_tiles_front_to_back(&projected, &bins).unwrap();
    let naive = rasterize_naive_back_to_front(&projected).unwrap();
    for (left, right) in tiled.rgba.iter().zip(&naive.rgba) {
        for channel in 0..4 {
            assert!((left[channel] - right[channel]).abs() <= 2.0e-6);
        }
    }

    // Existing BTF semantics draw source 0 then source 1 at equal depth, so
    // source 1 is composited over source 0. The FTB tile order must be 1, 0.
    let center_index = 8 * 17 + 8;
    let alpha = sample_alpha(projected.splats[0], 8, 8);
    let expected = [
        alpha * (1.0 - alpha),
        alpha,
        0.0,
        1.0 - (1.0 - alpha).powi(2),
    ];
    for (actual, expected) in tiled.rgba[center_index].iter().zip(expected) {
        assert!((actual - expected).abs() <= 1.0e-7);
    }
}

#[test]
fn tiled_front_to_back_matches_naive_global_back_to_front() {
    let sources = [
        isotropic([-0.18, 0.08, 1.4], 0.22, 0.72, [0.9, 0.1, 0.2]),
        source(
            [0.14, 0.03, 1.8],
            [0.055, 0.027, 0.0, 0.03, 0.0, 0.02],
            0.84,
            [0.15, 0.85, 0.3],
        ),
        source(
            [0.02, -0.2, 2.3],
            [0.025, -0.016, 0.0, 0.065, 0.0, 0.03],
            0.64,
            [0.2, 0.35, 0.95],
        ),
        isotropic([0.0, 0.0, 3.2], 0.5, 0.45, [0.85, 0.75, 0.1]),
    ];
    let projected = project_sources(&sources, &camera(), config(37, 29)).unwrap();
    let bins = build_tile_bins(&projected).unwrap();
    let tiled = rasterize_tiles_front_to_back(&projected, &bins).unwrap();
    let naive = rasterize_naive_back_to_front(&projected).unwrap();

    let mut max_error = 0.0_f32;
    for (left, right) in tiled.rgba.iter().zip(&naive.rgba) {
        for channel in 0..4 {
            max_error = max_error.max((left[channel] - right[channel]).abs());
        }
    }
    assert!(max_error <= 2.0e-6, "max float error was {max_error}");
}

#[test]
fn early_out_error_is_bounded_by_remaining_transmittance() {
    let sources = (0..40)
        .map(|index| {
            isotropic(
                [0.0, 0.0, 1.0 + index as f32 * 0.01],
                0.4,
                0.7,
                [0.25, 0.5, 0.75],
            )
        })
        .collect::<Vec<_>>();
    let projected = project_sources(&sources, &camera(), config(17, 17)).unwrap();
    let bins = build_tile_bins(&projected).unwrap();
    let tiled = rasterize_tiles_front_to_back(&projected, &bins).unwrap();
    let naive = rasterize_naive_back_to_front(&projected).unwrap();

    let mut max_error = 0.0_f32;
    for (left, right) in tiled.rgba.iter().zip(&naive.rgba) {
        for channel in 0..4 {
            max_error = max_error.max((left[channel] - right[channel]).abs());
        }
    }
    assert!(max_error <= 1.0e-4, "early-out error was {max_error}");
}

#[test]
fn invalid_source_is_structured_instead_of_silently_dropped() {
    let bad = isotropic([0.0, 0.0, 2.0], 0.2, f32::NAN, [1.0, 1.0, 1.0]);
    assert_eq!(
        project_sources(&[bad], &camera(), config(32, 32)),
        Err(TiledRasterError::InvalidSource { source_id: 0 })
    );
}

#[cfg(not(target_arch = "wasm32"))]
fn test_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    pollster::block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .ok()?;
        let format = adapter.get_texture_format_features(wgpu::TextureFormat::Rgba16Float);
        if !format
            .allowed_usages
            .contains(wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC)
        {
            return None;
        }
        let adapter_limits = adapter.limits();
        if adapter_limits.max_storage_buffers_per_shader_stage < 6 {
            return None;
        }
        let mut required_limits = wgpu::Limits::downlevel_defaults();
        required_limits.max_storage_buffers_per_shader_stage = 6;
        adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("tiled-raster-test-device"),
                required_features: wgpu::Features::empty(),
                required_limits,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .ok()
    })
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn gpu_count_prefix_scatter_sort_and_rgba16f_match_cpu_or_skip_without_adapter() {
    let Some((device, queue)) = test_device() else {
        eprintln!("skipping tiled GPU parity test; compatible adapter unavailable");
        return;
    };
    let mut sources = Vec::new();
    for index in 0..180 {
        let x = (index % 15) as f32 * 0.018 - 0.13;
        let y = (index % 11) as f32 * 0.016 - 0.08;
        // Repeated depths exercise the source-ID tie direction. The broad
        // support creates >1024 duplicated entries, exercising multi-group
        // radix and hierarchical prefix scans.
        let z = 1.6 + (index % 5) as f32 * 0.11;
        sources.push(isotropic(
            [x, y, z],
            0.48,
            0.35 + (index % 4) as f32 * 0.12,
            [
                (index % 3) as f32 * 0.35,
                ((index + 1) % 3) as f32 * 0.35,
                ((index + 2) % 3) as f32 * 0.35,
            ],
        ));
    }
    let projected = project_sources(&sources, &camera(), config(65, 49)).unwrap();
    let cpu_bins = build_tile_bins(&projected).unwrap();
    assert!(cpu_bins.entries.len() > 1024);
    let cpu_image = rasterize_tiles_front_to_back(&projected, &cpu_bins).unwrap();

    let gpu = tiled_gpu::rasterize_projected_scene_gpu(&device, &queue, &projected)
        .expect("exact tiled GPU path");
    assert_eq!(gpu.entry_count as usize, cpu_bins.entries.len());
    assert_eq!(gpu.ordered_entries, cpu_bins.entries);

    let mut max_error = 0.0_f32;
    for (left, right) in gpu.image.rgba.iter().zip(&cpu_image.rgba) {
        for channel in 0..4 {
            max_error = max_error.max((left[channel] - right[channel]).abs());
        }
    }
    assert!(max_error <= 1.5e-3, "RGBA16F GPU max error was {max_error}");
}
