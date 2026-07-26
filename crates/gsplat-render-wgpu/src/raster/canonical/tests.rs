#![cfg(not(target_arch = "wasm32"))]

use wgpu::util::DeviceExt;

use super::*;

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;
const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

async fn request_device() -> Option<(wgpu::AdapterInfo, wgpu::Device, wgpu::Queue)> {
    #[cfg(target_os = "macos")]
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..Default::default()
    });
    #[cfg(not(target_os = "macos"))]
    let instance = wgpu::Instance::default();

    let adapter = match instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        })
        .await
    {
        Ok(adapter) => adapter,
        #[cfg(target_os = "macos")]
        Err(error) => panic!("required CanonicalRaster Metal adapter unavailable: {error}"),
        #[cfg(not(target_os = "macos"))]
        Err(error) => {
            eprintln!("skipping optional CanonicalRaster GPU test; adapter unavailable: {error}");
            return None;
        }
    };
    let info = adapter.get_info();
    #[cfg(target_os = "macos")]
    assert_eq!(info.backend, wgpu::Backend::Metal, "Metal adapter required");

    let mut limits = wgpu::Limits::downlevel_defaults();
    limits.max_storage_buffers_per_shader_stage =
        crate::resident_gpu::RESIDENT_COLOR_STORAGE_BINDINGS;
    limits.max_storage_buffer_binding_size = 128 << 20;
    limits.max_buffer_size = 128 << 20;
    if !limits.check_limits(&adapter.limits()) {
        #[cfg(target_os = "macos")]
        panic!("required CanonicalRaster Metal limits unavailable: {limits:?}");
        #[cfg(not(target_os = "macos"))]
        {
            eprintln!("skipping optional CanonicalRaster GPU test; limits unavailable");
            return None;
        }
    }
    let descriptor = wgpu::DeviceDescriptor {
        label: Some("canonical-raster-test-device"),
        required_features: wgpu::Features::empty(),
        required_limits: limits,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    };
    match adapter.request_device(&descriptor).await {
        Ok((device, queue)) => Some((info, device, queue)),
        #[cfg(target_os = "macos")]
        Err(error) => panic!("required CanonicalRaster Metal device unavailable: {error}"),
        #[cfg(not(target_os = "macos"))]
        Err(error) => {
            eprintln!("skipping optional CanonicalRaster GPU test; device unavailable: {error}");
            None
        }
    }
}

fn storage_buffer(device: &wgpu::Device, label: &'static str, bytes: &[u8]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytes,
        usage: wgpu::BufferUsages::STORAGE,
    })
}

fn target(device: &wgpu::Device, label: &'static str) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: TARGET_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn copy_target(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    label: &'static str,
) -> wgpu::Buffer {
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: u64::from(WIDTH * HEIGHT * 4),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(WIDTH * 4),
                rows_per_image: Some(HEIGHT),
            },
        },
        wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
    );
    readback
}

fn read_buffer(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Vec<u8> {
    let slice = buffer.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("wait for CanonicalRaster readback");
    receiver.recv().expect("map callback").expect("map result");
    let bytes = slice.get_mapped_range().to_vec();
    buffer.unmap();
    bytes
}

fn packed_rgb18e8(rgb: [f32; 3]) -> [u32; 2] {
    let nonnegative = rgb.map(|value| value.max(0.0));
    let maximum = nonnegative.into_iter().fold(0.0_f32, f32::max);
    if maximum == 0.0 {
        return [0, 0];
    }
    let exponent = maximum.log2().ceil().clamp(-126.0, 127.0) as i32;
    let exponent_code = (exponent + 127) as u32;
    let scale = 2.0_f32.powi(exponent);
    let quantize = |value: f32| ((value / scale).clamp(0.0, 1.0) * 262_143.0).round() as u32;
    let q = nonnegative.map(quantize);
    [
        (q[0] & 0x3ffff) | ((q[1] & 0x3fff) << 18),
        ((q[1] >> 14) & 0xf) | ((q[2] & 0x3ffff) << 4) | (exponent_code << 22),
    ]
}

fn reference_rank_raster(
    device: &wgpu::Device,
    center: &wgpu::Buffer,
    axes: &wgpu::Buffer,
    color: &wgpu::Buffer,
) -> (wgpu::RenderPipeline, wgpu::BindGroup) {
    let layout = create_storage_layout(device, "canonical-raster-reference-rank-bgl", 3);
    let pipeline = create_splat_pipeline(
        device,
        &layout,
        TARGET_FORMAT,
        SplatPipeline {
            shader_label: "canonical-raster-reference-rank-shader",
            shader_source: include_str!("../../../shaders/projected_quads_draw.wgsl"),
            layout_label: "canonical-raster-reference-rank-layout",
            pipeline_label: "canonical-raster-reference-rank-pipeline",
            topology: wgpu::PrimitiveTopology::TriangleStrip,
        },
    );
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("canonical-raster-reference-rank-bg"),
        layout: &layout,
        entries: &[
            storage_entry(0, center),
            storage_entry(1, axes),
            storage_entry(2, color),
        ],
    });
    (pipeline, bind_group)
}

fn indirect_buffer(device: &wgpu::Device, label: &'static str, count: u32) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::cast_slice(&[QUAD_VERTEX_COUNT, count, 0_u32, 0]),
        usage: wgpu::BufferUsages::INDIRECT,
    })
}

struct RankFixture {
    centers: Vec<[f32; 4]>,
    axes: Vec<[f32; 4]>,
    colors: Vec<[u32; 2]>,
}

fn rank_fixture(count: u32) -> RankFixture {
    let allocated = count.max(1);
    let centers = (0..allocated)
        .map(|index| {
            let column = (index % 17) as f32;
            let row = ((index / 17) % 17) as f32;
            [
                -0.8 + column * 0.1,
                -0.8 + row * 0.1,
                0.82,
                f32::from_bits(index),
            ]
        })
        .collect::<Vec<_>>();
    let axes = (0..allocated)
        .map(|index| {
            let skew = (index % 3) as f32 * 0.002;
            [0.035, skew, -skew, 0.035]
        })
        .collect::<Vec<_>>();
    let colors = (0..allocated)
        .map(|index| {
            let phase = (index % 5) as f32 / 5.0;
            packed_rgb18e8([1.0 - phase * 0.5, 0.25 + phase * 0.5, 0.4])
        })
        .collect::<Vec<_>>();
    RankFixture {
        centers,
        axes,
        colors,
    }
}

fn rank_resources<'a>(
    center: &'a wgpu::Buffer,
    axes: &'a wgpu::Buffer,
    color: &'a wgpu::Buffer,
    indirect_args: Option<&'a wgpu::Buffer>,
    projected_capacity: u32,
    source_count: u32,
) -> RankIndexedRasterResources<'a> {
    RankIndexedRasterResources {
        projected_center_source: center,
        projected_axes: axes,
        resolved_color: color,
        indirect_args,
        projected_capacity,
        source_count,
    }
}

fn assert_clear_only(rgba: &[u8]) {
    assert!(rgba.iter().all(|&byte| byte == 0), "D=0 must clear only");
}

fn assert_contributing(rgba: &[u8]) {
    assert!(
        rgba.chunks_exact(4).any(|pixel| pixel[3] != 0),
        "nonzero D must produce contributing output"
    );
}

#[test]
fn rank_direct_is_byte_identical_to_accepted_projected_draw_on_required_gpu() {
    pollster::block_on(async {
        let Some((info, device, queue)) = request_device().await else {
            return;
        };
        eprintln!(
            "CANONICAL_RASTER adapter={} backend={:?} device_type={:?}",
            info.name, info.backend, info.device_type
        );

        let centers = [
            [-0.28_f32, -0.05, 0.92, f32::from_bits(0)],
            [0.24, 0.08, 0.85, f32::from_bits(1)],
            [0.02, 0.31, 0.74, f32::from_bits(2)],
        ];
        let axes = [
            [0.18_f32, 0.0, 0.0, 0.14],
            [0.13, 0.04, -0.03, 0.17],
            [0.11, -0.02, 0.05, 0.12],
        ];
        let colors = [
            packed_rgb18e8([1.0, 0.1, 0.05]),
            packed_rgb18e8([0.05, 1.0, 0.15]),
            packed_rgb18e8([0.1, 0.2, 1.0]),
        ];
        let center = storage_buffer(
            &device,
            "canonical-raster-rank-centers",
            bytemuck::cast_slice(&centers),
        );
        let axes = storage_buffer(
            &device,
            "canonical-raster-rank-axes",
            bytemuck::cast_slice(&axes),
        );
        let color = storage_buffer(
            &device,
            "canonical-raster-rank-colors",
            bytemuck::cast_slice(&colors),
        );
        let canonical = CanonicalRaster::prepare(
            &device,
            TARGET_FORMAT,
            CanonicalRasterResources {
                rank_indexed: Some(RankIndexedRasterResources {
                    projected_center_source: &center,
                    projected_axes: &axes,
                    resolved_color: &color,
                    indirect_args: None,
                    projected_capacity: centers.len() as u32,
                    source_count: centers.len() as u32,
                }),
                source_indexed: None,
            },
        )
        .expect("prepare CanonicalRaster rank input");
        let (reference_pipeline, reference_bind_group) =
            reference_rank_raster(&device, &center, &axes, &color);
        let (reference_texture, reference_view) =
            target(&device, "canonical-raster-rank-reference-target");
        let (canonical_texture, canonical_view) =
            target(&device, "canonical-raster-rank-canonical-target");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("canonical-raster-rank-parity-encoder"),
        });
        encode_splat_draw_into(
            &mut encoder,
            &SplatDraw {
                pass_label: "canonical-raster-rank-reference-pass",
                view: &reference_view,
                pipeline: &reference_pipeline,
                bind_group: &reference_bind_group,
                clear: wgpu::Color::TRANSPARENT,
                vertex_count: QUAD_VERTEX_COUNT,
                instance_count: centers.len() as u32,
            },
        );
        canonical
            .encode(
                &mut encoder,
                &canonical_view,
                TARGET_FORMAT,
                wgpu::Color::TRANSPARENT,
                CanonicalRasterInput::RankIndexedDirect {
                    instance_count: centers.len() as u32,
                },
            )
            .expect("encode canonical rank direct draw");
        let reference_readback = copy_target(
            &device,
            &mut encoder,
            &reference_texture,
            "canonical-raster-rank-reference-readback",
        );
        let canonical_readback = copy_target(
            &device,
            &mut encoder,
            &canonical_texture,
            "canonical-raster-rank-canonical-readback",
        );
        queue.submit(Some(encoder.finish()));

        let reference_rgba = read_buffer(&device, &reference_readback);
        let canonical_rgba = read_buffer(&device, &canonical_readback);
        assert_eq!(canonical_rgba, reference_rgba);
        assert!(canonical_rgba.iter().any(|&byte| byte != 0));
    });
}

#[test]
fn constructors_and_unprepared_variants_fail_closed() {
    pollster::block_on(async {
        let Some((_info, device, _queue)) = request_device().await else {
            return;
        };
        assert_eq!(
            CanonicalRaster::prepare(&device, TARGET_FORMAT, CanonicalRasterResources::default(),)
                .map(|_| ()),
            Err(CanonicalRasterError::Empty),
        );

        let storage16 = storage_buffer(&device, "canonical-raster-valid-storage16", &[0; 16]);
        let storage8 = storage_buffer(&device, "canonical-raster-valid-storage8", &[0; 8]);
        let storage4 = storage_buffer(&device, "canonical-raster-valid-storage4", &[0; 4]);
        let short_storage = storage_buffer(&device, "canonical-raster-short-storage", &[0; 8]);
        let wrong_usage = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("canonical-raster-wrong-usage"),
            size: 16,
            usage: wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let short_indirect = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("canonical-raster-short-indirect"),
            size: 12,
            usage: wgpu::BufferUsages::INDIRECT,
            mapped_at_creation: false,
        });
        let valid_indirect = indirect_buffer(&device, "canonical-raster-valid-indirect", 1);

        assert_eq!(
            CanonicalRaster::prepare(
                &device,
                TARGET_FORMAT,
                CanonicalRasterResources {
                    rank_indexed: Some(rank_resources(
                        &storage16, &storage16, &storage8, None, 0, 1,
                    )),
                    source_indexed: None,
                },
            )
            .map(|_| ()),
            Err(CanonicalRasterError::CountMismatch {
                resource: "rank-indexed projected capacity",
                expected: 1,
                actual: 0,
            }),
        );
        assert!(matches!(
            CanonicalRaster::prepare(
                &device,
                TARGET_FORMAT,
                CanonicalRasterResources {
                    rank_indexed: Some(rank_resources(
                        &short_storage,
                        &storage16,
                        &storage8,
                        None,
                        1,
                        1,
                    )),
                    source_indexed: None,
                },
            ),
            Err(CanonicalRasterError::BufferTooSmall {
                resource: "rank-indexed projected centers",
                ..
            })
        ));
        assert_eq!(
            CanonicalRaster::prepare(
                &device,
                TARGET_FORMAT,
                CanonicalRasterResources {
                    rank_indexed: Some(rank_resources(
                        &wrong_usage,
                        &storage16,
                        &storage8,
                        None,
                        1,
                        1,
                    )),
                    source_indexed: None,
                },
            )
            .map(|_| ()),
            Err(CanonicalRasterError::BufferUsage {
                resource: "rank-indexed projected centers",
                usage: wgpu::BufferUsages::STORAGE,
            }),
        );
        assert!(matches!(
            CanonicalRaster::prepare(
                &device,
                TARGET_FORMAT,
                CanonicalRasterResources {
                    rank_indexed: Some(rank_resources(
                        &storage16,
                        &storage16,
                        &storage8,
                        Some(&short_indirect),
                        1,
                        1,
                    )),
                    source_indexed: None,
                },
            ),
            Err(CanonicalRasterError::BufferTooSmall {
                resource: "rank-indexed indirect arguments",
                ..
            })
        ));

        let source = SourceIndexedRasterResources {
            ordered_source_ids: &storage4,
            projected_center_alpha_key: &storage16,
            projected_axes: &storage16,
            resolved_color: &storage8,
            indirect_args: &valid_indirect,
            source_count: 1,
        };
        let source_only = CanonicalRaster::prepare(
            &device,
            TARGET_FORMAT,
            CanonicalRasterResources {
                rank_indexed: None,
                source_indexed: Some(source),
            },
        )
        .expect("prepare source-only canonical raster");
        let (_texture, view) = target(&device, "canonical-raster-fail-closed-target");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("canonical-raster-fail-closed-encoder"),
        });
        assert_eq!(
            source_only.encode(
                &mut encoder,
                &view,
                TARGET_FORMAT,
                wgpu::Color::TRANSPARENT,
                CanonicalRasterInput::RankIndexedDirect { instance_count: 0 },
            ),
            Err(CanonicalRasterError::InputUnavailable {
                input: "rank-indexed direct",
            }),
        );
        assert_eq!(
            source_only.encode(
                &mut encoder,
                &view,
                wgpu::TextureFormat::Bgra8Unorm,
                wgpu::Color::TRANSPARENT,
                CanonicalRasterInput::SourceIndexedIndirect,
            ),
            Err(CanonicalRasterError::TargetFormatMismatch {
                expected: TARGET_FORMAT,
                actual: wgpu::TextureFormat::Bgra8Unorm,
            }),
        );

        let rank_only = CanonicalRaster::prepare(
            &device,
            TARGET_FORMAT,
            CanonicalRasterResources {
                rank_indexed: Some(rank_resources(
                    &storage16, &storage16, &storage8, None, 1, 1,
                )),
                source_indexed: None,
            },
        )
        .expect("prepare rank-only canonical raster");
        assert_eq!(
            rank_only.encode(
                &mut encoder,
                &view,
                TARGET_FORMAT,
                wgpu::Color::TRANSPARENT,
                CanonicalRasterInput::RankIndexedIndirect,
            ),
            Err(CanonicalRasterError::InputUnavailable {
                input: "rank-indexed indirect arguments",
            }),
        );
        assert_eq!(
            rank_only.encode(
                &mut encoder,
                &view,
                TARGET_FORMAT,
                wgpu::Color::TRANSPARENT,
                CanonicalRasterInput::RankIndexedDirect { instance_count: 2 },
            ),
            Err(CanonicalRasterError::DirectCountExceedsCapacity {
                count: 2,
                capacity: 1,
            }),
        );
    });
}

#[test]
fn rank_direct_and_indirect_preserve_d_equals_v_for_boundaries() {
    pollster::block_on(async {
        let Some((_info, device, queue)) = request_device().await else {
            return;
        };
        for count in [0_u32, 1, 127, 128, 129, 257] {
            let fixture = rank_fixture(count);
            let center = storage_buffer(
                &device,
                "canonical-raster-boundary-rank-centers",
                bytemuck::cast_slice(&fixture.centers),
            );
            let axes = storage_buffer(
                &device,
                "canonical-raster-boundary-rank-axes",
                bytemuck::cast_slice(&fixture.axes),
            );
            let color = storage_buffer(
                &device,
                "canonical-raster-boundary-rank-colors",
                bytemuck::cast_slice(&fixture.colors),
            );
            let indirect =
                indirect_buffer(&device, "canonical-raster-boundary-rank-indirect", count);
            let canonical = CanonicalRaster::prepare(
                &device,
                TARGET_FORMAT,
                CanonicalRasterResources {
                    rank_indexed: Some(RankIndexedRasterResources {
                        projected_center_source: &center,
                        projected_axes: &axes,
                        resolved_color: &color,
                        indirect_args: Some(&indirect),
                        projected_capacity: count,
                        source_count: count,
                    }),
                    source_indexed: None,
                },
            )
            .expect("prepare boundary rank raster");
            let (direct_texture, direct_view) =
                target(&device, "canonical-raster-boundary-direct-target");
            let (indirect_texture, indirect_view) =
                target(&device, "canonical-raster-boundary-indirect-target");
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("canonical-raster-boundary-rank-encoder"),
            });
            canonical
                .encode(
                    &mut encoder,
                    &direct_view,
                    TARGET_FORMAT,
                    wgpu::Color::TRANSPARENT,
                    CanonicalRasterInput::RankIndexedDirect {
                        instance_count: count,
                    },
                )
                .expect("encode direct D=V draw");
            canonical
                .encode(
                    &mut encoder,
                    &indirect_view,
                    TARGET_FORMAT,
                    wgpu::Color::TRANSPARENT,
                    CanonicalRasterInput::RankIndexedIndirect,
                )
                .expect("encode indirect D=V draw");
            let direct_readback = copy_target(
                &device,
                &mut encoder,
                &direct_texture,
                "canonical-raster-boundary-direct-readback",
            );
            let indirect_readback = copy_target(
                &device,
                &mut encoder,
                &indirect_texture,
                "canonical-raster-boundary-indirect-readback",
            );
            queue.submit(Some(encoder.finish()));

            let direct_rgba = read_buffer(&device, &direct_readback);
            let indirect_rgba = read_buffer(&device, &indirect_readback);
            assert_eq!(indirect_rgba, direct_rgba, "rank D=V mismatch at {count}");
            if count == 0 {
                assert_clear_only(&indirect_rgba);
            } else {
                assert_contributing(&indirect_rgba);
            }
        }
    });
}

#[test]
fn production_leaf_has_no_plan_policy_or_target_lifecycle_ownership() {
    let source = include_str!("../canonical.rs");
    for forbidden in [
        "PlanId",
        "ProjectedWork",
        "GpuPostSortWork",
        "GpuPreprojectWork",
        ".submit(",
        ".poll(",
        ".map_async(",
        "copy_texture",
        ".present(",
        "std::env",
    ] {
        assert!(
            !source.contains(forbidden),
            "CanonicalRaster leaf must not contain {forbidden}"
        );
    }
    let encode = source
        .split("pub(crate) fn encode(")
        .nth(1)
        .expect("encode")
        .split("\nfn validate_rank_resources")
        .next()
        .expect("encode body");
    assert!(
        !encode.contains("create_bind_group("),
        "bind groups must be prepared before frame encoding"
    );
}
