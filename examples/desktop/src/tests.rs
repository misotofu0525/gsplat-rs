use std::fs;
use std::path::Path;

use gsplat_core::{RenderMode, RendererConfig, SceneBuffers, Vec3f};
use gsplat_render_wgpu::{GeometryPath, Renderer, SurfaceGpuOrderProducer, SurfaceOrderBackend};
#[cfg(feature = "interactive-viewer")]
use gsplat_render_wgpu::{
    SurfaceAdaptivePendingSample, SurfaceCpuOrderMeasurement, SurfaceGpuProducerDrawScope,
    SurfaceGpuProducerMeasurement, SurfaceOrderBackendUsed,
};

use crate::cli::{Args, SurfaceBenchmarkMode, SurfaceEvidencePlanArg};
#[cfg(not(feature = "diagnostic-surface-depth-key-candidate24"))]
use crate::cli::{SurfaceSortPolicyArg, validate_surface_trace_geometry};
use crate::image_output::write_png;
use crate::scene::{auto_camera, load_ply_path_into_renderer, scene_bounds};
#[cfg(not(feature = "diagnostic-surface-depth-key-candidate24"))]
use crate::trace::{CameraTracePlayback, load_camera_trace};
#[cfg(feature = "interactive-viewer")]
use crate::viewer::{
    SurfaceBenchmarkAction, SurfaceBenchmarkProducerTicket, SurfaceBenchmarkSummary,
    SurfaceBenchmarkTicket, SurfaceResolutionReceipt, surface_benchmark_action,
};

const CAMERA_TRACE_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/perf/trace/fixtures/camera-trace-v1.json"
);

fn parse_args(args: &[&str]) -> Result<Args, String> {
    Args::parse(args.iter().map(|value| value.to_string()))
}

fn scene_with_positions(positions: Vec<Vec3f>) -> SceneBuffers {
    let len = positions.len();
    SceneBuffers {
        positions,
        opacity: vec![1.0; len],
        scale_xyz: vec![[0.0, 0.0, 0.0]; len],
        rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; len],
        color_dc: vec![[0.1, 0.2, 0.3]; len],
        sh_degree: 0,
        sh_rest: None,
    }
}

#[cfg(feature = "interactive-viewer")]
#[test]
fn adaptive_pending_sample_blocks_trace_advance_and_benchmark_completion() {
    let pending = Some(SurfaceAdaptivePendingSample {
        backend: SurfaceOrderBackendUsed::Cpu,
        ticket: 2,
    });
    assert_eq!(
        surface_benchmark_action(SurfaceBenchmarkMode::Isolated, 0, 1, false, pending),
        SurfaceBenchmarkAction::PollReceipts
    );
    assert_eq!(
        surface_benchmark_action(SurfaceBenchmarkMode::Isolated, 1, 1, false, pending),
        SurfaceBenchmarkAction::PollReceipts
    );
    assert_eq!(
        surface_benchmark_action(SurfaceBenchmarkMode::Isolated, 0, 1, true, None),
        SurfaceBenchmarkAction::PollReceipts
    );
    assert_eq!(
        surface_benchmark_action(SurfaceBenchmarkMode::Isolated, 1, 1, true, None),
        SurfaceBenchmarkAction::PollReceipts
    );
    assert_eq!(
        surface_benchmark_action(SurfaceBenchmarkMode::Isolated, 0, 1, false, None),
        SurfaceBenchmarkAction::RenderTraceStep
    );
    assert_eq!(
        surface_benchmark_action(SurfaceBenchmarkMode::Isolated, 1, 1, false, None),
        SurfaceBenchmarkAction::Complete
    );
    assert_eq!(
        surface_benchmark_action(SurfaceBenchmarkMode::Throughput, 0, 1, true, pending),
        SurfaceBenchmarkAction::RenderTraceStep
    );
    assert_eq!(
        surface_benchmark_action(SurfaceBenchmarkMode::Throughput, 1, 1, true, pending),
        SurfaceBenchmarkAction::PollReceipts
    );
}

#[cfg(feature = "interactive-viewer")]
#[test]
fn surface_resolution_receipt_fails_closed_on_any_scaled_stage() {
    let receipt =
        SurfaceResolutionReceipt::validated((1920, 1080), (1920, 1080), (1920, 1080)).unwrap();
    assert!(receipt.full_resolution());
    assert!(SurfaceResolutionReceipt::validated((1920, 1080), (1280, 720), (1920, 1080),).is_err());
    assert!(SurfaceResolutionReceipt::validated((1920, 1080), (1920, 1080), (960, 540),).is_err());
}

#[cfg(feature = "interactive-viewer")]
#[test]
fn benchmark_ticket_ledger_accepts_exactly_one_matching_cpu_terminal() {
    let mut summary = SurfaceBenchmarkSummary::default();
    summary.outstanding_order_tickets.insert(
        2,
        SurfaceBenchmarkTicket {
            backend: SurfaceOrderBackendUsed::Cpu,
            camera_revision: 7,
            measured: true,
        },
    );
    let measurement = SurfaceCpuOrderMeasurement {
        ticket: 2,
        camera_revision: 7,
        preprocess_ms: 1.0,
        sort_ms: 2.0,
        frame_complete_ms: 4.0,
        visible_count: 10,
        contributor_count: 8,
        drawn_count: 8,
        exact_contributor_compaction: true,
    };

    assert_eq!(summary.record_cpu_measurement(measurement), Ok(true));
    assert!(!summary.has_outstanding_tickets());
    assert_eq!(summary.cpu_completion_ms, vec![4.0]);
    assert!(
        summary
            .record_cpu_measurement(measurement)
            .unwrap_err()
            .contains("more than one terminal")
    );
}

#[cfg(feature = "interactive-viewer")]
#[test]
fn producer_ticket_ledger_requires_matching_identity_and_exact_scope_counts() {
    let ticket = 1_u64 << 51;
    let mut summary = SurfaceBenchmarkSummary::default();
    summary.outstanding_producer_tickets.insert(
        ticket,
        SurfaceBenchmarkProducerTicket {
            producer: SurfaceGpuOrderProducer::Preproject,
            camera_revision: 11,
            measured: true,
        },
    );
    let measurement = SurfaceGpuProducerMeasurement {
        ticket,
        camera_revision: 11,
        producer: SurfaceGpuOrderProducer::Preproject,
        order_generation: 3,
        projection_generation: 5,
        source_count: 100,
        contributor_count: 73,
        drawn_count: 73,
        order_refreshed: true,
        draw_scope: SurfaceGpuProducerDrawScope::ExactCurrentContributors,
        frame_complete_ms: 7.5,
    };

    assert_eq!(
        summary.record_producer_measurement(measurement, 100),
        Ok(true)
    );
    assert!(!summary.has_outstanding_tickets());
    assert_eq!(summary.producer_completion_ms, vec![7.5]);
    assert_eq!(summary.producer_exact_frames, 1);
    assert!(
        summary
            .record_producer_measurement(measurement, 100)
            .unwrap_err()
            .contains("more than one terminal")
    );

    let invalid_ticket = ticket + 1;
    summary.outstanding_producer_tickets.insert(
        invalid_ticket,
        SurfaceBenchmarkProducerTicket {
            producer: SurfaceGpuOrderProducer::Preproject,
            camera_revision: 12,
            measured: false,
        },
    );
    let invalid = SurfaceGpuProducerMeasurement {
        ticket: invalid_ticket,
        camera_revision: 12,
        drawn_count: 72,
        ..measurement
    };
    assert!(
        summary
            .record_producer_measurement(invalid, 100)
            .unwrap_err()
            .contains("requires refreshed D=C")
    );
}

#[test]
fn packed_path_loader_builds_resident_scene_without_wide_source() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/datasets/minimal_ascii.ply");
    let mut direct = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
    load_ply_path_into_renderer(&path, &mut direct).unwrap();
    let direct_positions = direct.positions().unwrap().to_vec();
    assert!(direct.scene().is_some());

    let mut packed = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
    packed.set_geometry_path(GeometryPath::PackedAtlas);
    load_ply_path_into_renderer(&path, &mut packed).unwrap();
    assert!(packed.scene().is_none());
    assert!(packed.resident_scene().is_some());
    assert_eq!(packed.positions(), Some(direct_positions.as_slice()));
    assert_eq!(packed.scene_len(), direct.scene_len());
    assert_eq!(packed.scene_sh_degree(), direct.scene_sh_degree());
}

#[test]
fn packed_path_loader_does_not_publish_a_partially_decoded_scene() {
    let valid_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/datasets/minimal_ascii.ply");
    let mut renderer = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
    renderer.set_geometry_path(GeometryPath::PackedAtlas);
    load_ply_path_into_renderer(&valid_path, &mut renderer).unwrap();
    let original_positions = renderer.positions().unwrap().to_vec();

    let mut malformed = fs::read_to_string(&valid_path)
        .unwrap()
        .replace("element vertex 3", "element vertex 4");
    malformed.push_str("not a valid vertex row\n");
    let malformed_path = std::env::temp_dir().join(format!(
        "gsplat-desktop-transactional-resident-{}.ply",
        std::process::id()
    ));
    fs::write(&malformed_path, malformed).unwrap();
    let result = load_ply_path_into_renderer(&malformed_path, &mut renderer);
    let _ = fs::remove_file(&malformed_path);

    assert!(result.is_err());
    assert_eq!(renderer.positions(), Some(original_positions.as_slice()));
    assert_eq!(renderer.scene_len(), Some(original_positions.len()));
}

#[cfg(not(feature = "diagnostic-surface-depth-key-candidate24"))]
#[test]
fn args_parse_defaults_to_minimal_dataset() {
    let args = parse_args(&[]).unwrap();

    assert_eq!(
        args.dataset_path,
        std::path::PathBuf::from("tests/datasets/minimal_ascii.ply")
    );
    assert_eq!(args.config.width, 1280);
    assert_eq!(args.config.height, 720);
    assert_eq!(args.config.mode, RenderMode::SortedAlpha);
    assert_eq!(args.frames, 1);
    assert!(!args.orbit);
    assert!(!args.auto_camera);
    assert!(!args.interactive);
    assert_eq!(args.geometry_path, GeometryPath::PackedAtlas);
    assert_eq!(args.order_backend, SurfaceOrderBackend::Adaptive);
    assert_eq!(args.surface_benchmark_mode, SurfaceBenchmarkMode::Isolated);
    assert_eq!(args.surface_sort_policy, SurfaceSortPolicyArg::EveryFrame);
    assert_eq!(args.surface_gpu_producer, None);
    assert_eq!(args.surface_evidence_plan, None);
    assert!(!args.surface_diagnostic_capture_receipt);
    assert!(!args.surface_diagnostic_multi_capture);
    assert!(args.png_out.is_none());
    assert!(args.camera_trace_path.is_none());
    assert_eq!(args.camera_frame, 0);
    assert!(!args.camera_sequence);
    assert!(args.camera_frame_indices.is_none());
    assert_eq!(args.camera_warmup_frames, 0);
    assert!(args.camera_measured_frames.is_none());
    assert_eq!(args.camera_loops, 1);
}

#[cfg(not(feature = "diagnostic-surface-depth-key-candidate24"))]
#[test]
fn args_parse_flags_and_clamps_frames() {
    let args = parse_args(&[
        "scene.ply",
        "--frames",
        "0",
        "--width",
        "640",
        "--height",
        "480",
        "--orbit",
        "--auto-camera",
        "--yaw-deg",
        "15",
        "--geometry-path",
        "paged",
        "--png",
        "target/out.png",
    ])
    .unwrap();

    assert_eq!(args.dataset_path, std::path::PathBuf::from("scene.ply"));
    assert_eq!(args.frames, 1);
    assert_eq!(args.config.width, 640);
    assert_eq!(args.config.height, 480);
    assert!(args.orbit);
    assert!(args.auto_camera);
    assert_eq!(args.yaw_deg, Some(15.0));
    assert_eq!(args.geometry_path, GeometryPath::PagedActiveAtlas);
    assert_eq!(
        args.png_out,
        Some(std::path::PathBuf::from("target/out.png"))
    );
}

#[test]
fn args_parse_rejects_unknown_and_extra_args() {
    let err = parse_args(&["--nope"]).unwrap_err();
    assert!(err.contains("unknown flag: --nope"));

    let err = parse_args(&["a.ply", "b.ply"]).unwrap_err();
    assert!(err.contains("unexpected extra arg: b.ply"));
}

#[cfg(not(feature = "diagnostic-surface-depth-key-candidate24"))]
#[test]
fn args_parse_surface_order_backends_and_rejects_non_surface_gpu() {
    for (label, expected) in [
        ("cpu", SurfaceOrderBackend::Cpu),
        ("gpu", SurfaceOrderBackend::Gpu),
        ("adaptive", SurfaceOrderBackend::Adaptive),
    ] {
        let args = parse_args(&["--interactive", "--order-backend", label]).unwrap();
        assert_eq!(args.order_backend, expected);
    }

    assert!(
        parse_args(&["--order-backend", "gpu"])
            .unwrap_err()
            .contains("requires --interactive")
    );
    assert!(
        parse_args(&["--interactive", "--order-backend", "automatic"])
            .unwrap_err()
            .contains("expected cpu|gpu|adaptive")
    );
    assert!(
        parse_args(&[
            "--interactive",
            "--geometry-path",
            "paged",
            "--order-backend",
            "adaptive",
        ])
        .unwrap_err()
        .contains("paged geometry")
    );

    let paged_cpu = parse_args(&[
        "--interactive",
        "--geometry-path",
        "paged",
        "--order-backend",
        "cpu",
    ])
    .unwrap();
    assert!(validate_surface_trace_geometry(&paged_cpu, false).is_ok());
    assert!(
        validate_surface_trace_geometry(&paged_cpu, true)
            .unwrap_err()
            .contains("full-quality")
    );

    assert!(
        parse_args(&["--surface-benchmark-mode", "throughput"])
            .unwrap_err()
            .contains("requires --interactive")
    );
    let throughput =
        parse_args(&["--interactive", "--surface-benchmark-mode", "throughput"]).unwrap();
    assert_eq!(
        throughput.surface_benchmark_mode,
        SurfaceBenchmarkMode::Throughput
    );

    for (label, expected) in [
        ("every-frame", SurfaceSortPolicyArg::EveryFrame),
        ("camera-change", SurfaceSortPolicyArg::CameraChange),
    ] {
        let args = parse_args(&["--interactive", "--surface-sort-policy", label]).unwrap();
        assert_eq!(args.surface_sort_policy, expected);
    }
    assert!(
        parse_args(&["--surface-sort-policy", "camera-change"])
            .unwrap_err()
            .contains("requires --interactive")
    );
    assert!(
        parse_args(&["--interactive", "--surface-sort-policy", "never"])
            .unwrap_err()
            .contains("expected every-frame|camera-change")
    );

    for (label, expected) in [
        ("post-sort", SurfaceGpuOrderProducer::PostSort),
        ("preproject", SurfaceGpuOrderProducer::Preproject),
    ] {
        let args = parse_args(&[
            "--interactive",
            "--order-backend",
            "gpu",
            "--surface-gpu-producer",
            label,
        ])
        .unwrap();
        assert_eq!(args.surface_gpu_producer, Some(expected));
    }
    assert!(
        parse_args(&["--surface-gpu-producer", "preproject"])
            .unwrap_err()
            .contains("requires --interactive")
    );
    assert!(
        parse_args(&[
            "--interactive",
            "--order-backend",
            "cpu",
            "--surface-gpu-producer",
            "preproject",
        ])
        .unwrap_err()
        .contains("requires --order-backend gpu|adaptive")
    );
    assert!(
        parse_args(&[
            "--interactive",
            "--geometry-path",
            "direct",
            "--order-backend",
            "gpu",
            "--surface-gpu-producer",
            "preproject",
        ])
        .unwrap_err()
        .contains("requires --geometry-path packed")
    );
    assert!(
        parse_args(&[
            "--interactive",
            "--order-backend",
            "gpu",
            "--surface-gpu-producer",
            "automatic",
        ])
        .unwrap_err()
        .contains("expected post-sort|preproject")
    );
}

#[test]
fn args_parse_rejects_retired_surface_raster_plan() {
    for value in ["projected", "global"] {
        let error = parse_args(&["--interactive", "--surface-raster-plan", value]).unwrap_err();
        assert!(error.contains("unknown flag: --surface-raster-plan"));
    }

    let help = parse_args(&["--help"]).unwrap_err();
    assert!(!help.contains("  --surface-raster-plan"));
}

#[cfg(not(feature = "diagnostic-surface-depth-key-candidate24"))]
#[test]
fn interactive_png_is_restricted_to_explicit_producer_trace_benchmarks() {
    assert!(
        parse_args(&["--interactive", "--png", "target/capture.png"])
            .unwrap_err()
            .contains("requires --camera-trace")
    );
    assert!(
        parse_args(&[
            "--interactive",
            "--camera-trace",
            CAMERA_TRACE_FIXTURE,
            "--png",
            "target/capture.png",
        ])
        .unwrap_err()
        .contains("requires an explicit --surface-gpu-producer")
    );
    let args = parse_args(&[
        "--interactive",
        "--camera-trace",
        CAMERA_TRACE_FIXTURE,
        "--order-backend",
        "gpu",
        "--surface-gpu-producer",
        "post-sort",
        "--png",
        "target/capture.png",
    ])
    .expect("valid diagnostic capture");
    assert_eq!(
        args.png_out,
        Some(std::path::PathBuf::from("target/capture.png"))
    );
}

#[test]
fn surface_evidence_plan_is_a_closed_fail_closed_cli() {
    for (label, expected, backend) in [
        (
            "cpu-post-sort",
            SurfaceEvidencePlanArg::CpuPostSort,
            SurfaceOrderBackend::Cpu,
        ),
        (
            "gpu-post-sort",
            SurfaceEvidencePlanArg::GpuPostSort,
            SurfaceOrderBackend::Gpu,
        ),
        (
            "gpu-preproject",
            SurfaceEvidencePlanArg::GpuPreproject,
            SurfaceOrderBackend::Gpu,
        ),
        (
            "adaptive",
            SurfaceEvidencePlanArg::Adaptive,
            SurfaceOrderBackend::Adaptive,
        ),
    ] {
        let values = vec![
            "--interactive",
            "--camera-trace",
            CAMERA_TRACE_FIXTURE,
            "--surface-evidence-plan",
            label,
            "--png",
            "target/capture.png",
        ];
        #[cfg(feature = "diagnostic-surface-depth-key-candidate24")]
        let values = {
            let mut values = values;
            values.push("--surface-diagnostic-capture-receipt");
            values
        };
        let args = parse_args(&values).unwrap();
        assert_eq!(args.surface_evidence_plan, Some(expected));
        assert_eq!(args.order_backend, backend);
    }

    for (args, message) in [
        (
            vec!["--surface-evidence-plan", "adaptive"],
            "requires --interactive",
        ),
        (
            vec![
                "--interactive",
                "--surface-evidence-plan",
                "adaptive",
                "--png",
                "target/capture.png",
            ],
            "requires --camera-trace",
        ),
        (
            vec![
                "--interactive",
                "--camera-trace",
                CAMERA_TRACE_FIXTURE,
                "--surface-evidence-plan",
                "adaptive",
            ],
            "requires --png",
        ),
        (
            vec![
                "--interactive",
                "--camera-trace",
                CAMERA_TRACE_FIXTURE,
                "--surface-evidence-plan",
                "gpu-preproject",
                "--order-backend",
                "cpu",
                "--png",
                "target/capture.png",
            ],
            "conflicts with --order-backend",
        ),
    ] {
        assert!(parse_args(&args).unwrap_err().contains(message));
    }
}

#[cfg(not(feature = "diagnostic-surface-capture-receipt"))]
#[test]
fn diagnostic_surface_capture_receipt_flag_is_unavailable_without_feature() {
    assert!(
        parse_args(&["--surface-diagnostic-capture-receipt"])
            .unwrap_err()
            .contains("unknown flag")
    );
}

#[cfg(feature = "diagnostic-surface-capture-receipt")]
#[test]
fn diagnostic_surface_capture_receipt_flag_requires_strict_surface_evidence() {
    assert!(
        parse_args(&["--surface-diagnostic-capture-receipt"])
            .unwrap_err()
            .contains("requires --surface-evidence-plan")
    );

    let args = parse_args(&[
        "--interactive",
        "--camera-trace",
        CAMERA_TRACE_FIXTURE,
        "--surface-evidence-plan",
        "gpu-post-sort",
        "--surface-diagnostic-capture-receipt",
        "--png",
        "target/capture.png",
    ])
    .unwrap();
    assert!(args.surface_diagnostic_capture_receipt);
    assert!(!args.surface_diagnostic_multi_capture);

    assert!(
        parse_args(&["--surface-diagnostic-multi-capture"])
            .unwrap_err()
            .contains("requires --surface-diagnostic-capture-receipt")
    );

    let args = parse_args(&[
        "--interactive",
        "--camera-trace",
        CAMERA_TRACE_FIXTURE,
        "--surface-evidence-plan",
        "gpu-post-sort",
        "--surface-diagnostic-capture-receipt",
        "--surface-diagnostic-multi-capture",
        "--png",
        "target/capture.png",
    ])
    .unwrap();
    assert!(args.surface_diagnostic_multi_capture);
}

#[cfg(feature = "diagnostic-surface-depth-key-candidate24")]
#[test]
fn candidate24_desktop_execution_requires_diagnostic_receipt_flag() {
    for values in [vec![], vec!["--interactive"]] {
        let error = Args::parse(values.into_iter().map(str::to_owned)).unwrap_err();
        assert!(error.contains("Candidate24 desktop execution requires"));
    }

    let error = Args::parse(
        [
            "--interactive",
            "--camera-trace",
            CAMERA_TRACE_FIXTURE,
            "--surface-evidence-plan",
            "gpu-post-sort",
            "--png",
            "target/capture.png",
        ]
        .into_iter()
        .map(str::to_owned),
    )
    .unwrap_err();
    assert!(error.contains("Candidate24 desktop execution requires"));

    let args = Args::parse(
        [
            "--interactive",
            "--camera-trace",
            CAMERA_TRACE_FIXTURE,
            "--surface-evidence-plan",
            "gpu-post-sort",
            "--surface-diagnostic-capture-receipt",
            "--png",
            "target/capture.png",
        ]
        .into_iter()
        .map(str::to_owned),
    )
    .unwrap();
    assert!(args.surface_diagnostic_capture_receipt);
}

#[cfg(not(feature = "diagnostic-surface-depth-key-candidate24"))]
#[test]
fn fixed_camera_trace_sets_contract_display_and_selected_frame() {
    let mut args = parse_args(&[
        "--camera-trace",
        CAMERA_TRACE_FIXTURE,
        "--camera-frame",
        "2",
    ])
    .unwrap();

    let selected = load_camera_trace(&mut args).unwrap().unwrap();

    assert_eq!((args.config.width, args.config.height), (640, 360));
    assert!(matches!(
        &selected,
        CameraTracePlayback::Fixed { frame_index, .. } if *frame_index == 2
    ));
    assert_eq!(
        selected.initial_camera().unwrap().pose.position,
        Vec3f::new(0.5, 0.125, -3.0)
    );
}

#[cfg(not(feature = "diagnostic-surface-depth-key-candidate24"))]
#[test]
fn fixed_camera_trace_rejects_conflicting_controls_and_display() {
    let mut args = parse_args(&["--camera-trace", CAMERA_TRACE_FIXTURE, "--orbit"]).unwrap();
    assert!(
        load_camera_trace(&mut args)
            .unwrap_err()
            .contains("cannot be combined")
    );

    let mut args = parse_args(&["--camera-trace", CAMERA_TRACE_FIXTURE, "--width", "800"]).unwrap();
    assert!(
        load_camera_trace(&mut args)
            .unwrap_err()
            .contains("conflicts")
    );

    let mut args = parse_args(&[
        "--interactive",
        "--camera-trace",
        CAMERA_TRACE_FIXTURE,
        "--order-backend",
        "gpu",
    ])
    .unwrap();
    assert!(load_camera_trace(&mut args).unwrap().is_some());

    let mut args = parse_args(&[
        "--camera-trace",
        CAMERA_TRACE_FIXTURE,
        "--frames",
        "3",
        "--camera-measured-frames",
        "4",
    ])
    .unwrap();
    assert!(
        load_camera_trace(&mut args)
            .unwrap_err()
            .contains("both control fixed-frame measurement length")
    );
}

#[cfg(not(feature = "diagnostic-surface-depth-key-candidate24"))]
#[test]
fn throughput_surface_benchmark_requires_a_trace_but_accepts_a_fixed_pose() {
    let mut no_trace =
        parse_args(&["--interactive", "--surface-benchmark-mode", "throughput"]).unwrap();
    assert!(
        load_camera_trace(&mut no_trace)
            .unwrap_err()
            .contains("require --camera-trace")
    );

    let mut fixed = parse_args(&[
        "--interactive",
        "--camera-trace",
        CAMERA_TRACE_FIXTURE,
        "--surface-benchmark-mode",
        "throughput",
    ])
    .unwrap();
    assert!(matches!(
        load_camera_trace(&mut fixed).unwrap(),
        Some(CameraTracePlayback::Fixed {
            warmup_frames: 0,
            measured_frames: 1,
            ..
        })
    ));
}

#[cfg(not(feature = "diagnostic-surface-depth-key-candidate24"))]
#[test]
fn trace_sequence_defaults_to_each_revision_once_and_accepts_explicit_schedule() {
    let mut args =
        parse_args(&["--camera-trace", CAMERA_TRACE_FIXTURE, "--camera-sequence"]).unwrap();
    let playback = load_camera_trace(&mut args).unwrap().unwrap();
    match playback {
        CameraTracePlayback::Sequence {
            frame_indices,
            warmup_frames,
            measured_frames,
            loops,
            ..
        } => {
            assert_eq!(frame_indices, vec![0, 1, 2]);
            assert_eq!(warmup_frames, 0);
            assert_eq!(measured_frames, 3);
            assert_eq!(loops, 1);
        }
        CameraTracePlayback::Fixed { .. } => panic!("expected sequence"),
    }

    let mut args = parse_args(&[
        "--camera-trace",
        CAMERA_TRACE_FIXTURE,
        "--camera-sequence",
        "--camera-frame-indices",
        "2,0",
        "--camera-warmup-frames",
        "3",
        "--camera-measured-frames",
        "5",
        "--camera-loops",
        "2",
    ])
    .unwrap();
    let playback = load_camera_trace(&mut args).unwrap().unwrap();
    assert!(matches!(
        playback,
        CameraTracePlayback::Sequence {
            frame_indices,
            warmup_frames: 3,
            measured_frames: 5,
            loops: 2,
            ..
        } if frame_indices == vec![2, 0]
    ));
}

#[cfg(not(feature = "diagnostic-surface-depth-key-candidate24"))]
#[test]
fn trace_sequence_rejects_fixed_only_and_duplicate_controls() {
    let mut args = parse_args(&[
        "--camera-trace",
        CAMERA_TRACE_FIXTURE,
        "--camera-sequence",
        "--camera-frame",
        "0",
    ])
    .unwrap();
    assert!(
        load_camera_trace(&mut args)
            .unwrap_err()
            .contains("cannot be combined")
    );

    let mut args = parse_args(&[
        "--camera-trace",
        CAMERA_TRACE_FIXTURE,
        "--camera-sequence",
        "--camera-frame-indices",
        "0,0",
    ])
    .unwrap();
    assert!(
        load_camera_trace(&mut args)
            .unwrap_err()
            .contains("duplicate")
    );
}

#[cfg(feature = "interactive-viewer")]
#[cfg(not(feature = "diagnostic-surface-depth-key-candidate24"))]
#[test]
fn surface_trace_schedule_preserves_warmup_measurement_and_loop_order() {
    let mut fixed_args = parse_args(&[
        "--interactive",
        "--camera-trace",
        CAMERA_TRACE_FIXTURE,
        "--camera-frame",
        "1",
        "--frames",
        "3",
    ])
    .unwrap();
    let fixed = load_camera_trace(&mut fixed_args).unwrap().unwrap();
    let fixed_steps = crate::viewer::surface_trace_steps(&fixed).unwrap();
    assert_eq!(fixed_steps.len(), 3);
    assert!(fixed_steps.iter().all(|step| step.measured()));
    assert!(fixed_steps.iter().all(|step| step.trace_frame_index == 1));
    assert_eq!(
        fixed_steps
            .iter()
            .map(|step| step.playback_index)
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );

    let mut fixed_warmup_args = parse_args(&[
        "--interactive",
        "--camera-trace",
        CAMERA_TRACE_FIXTURE,
        "--camera-frame",
        "1",
        "--camera-warmup-frames",
        "2",
        "--camera-measured-frames",
        "3",
    ])
    .unwrap();
    let fixed_warmup = load_camera_trace(&mut fixed_warmup_args).unwrap().unwrap();
    let fixed_warmup_steps = crate::viewer::surface_trace_steps(&fixed_warmup).unwrap();
    assert_eq!(fixed_warmup_steps.len(), 5);
    assert_eq!(
        fixed_warmup_steps
            .iter()
            .map(|step| step.phase.as_str())
            .collect::<Vec<_>>(),
        vec!["warmup", "warmup", "measure", "measure", "measure"]
    );
    assert_eq!(
        fixed_warmup_steps
            .iter()
            .map(|step| step.measured_sample_index)
            .collect::<Vec<_>>(),
        vec![None, None, Some(0), Some(1), Some(2)]
    );
    assert!(
        fixed_warmup_steps
            .iter()
            .all(|step| step.trace_frame_index == 1)
    );

    let mut sequence_args = parse_args(&[
        "--interactive",
        "--camera-trace",
        CAMERA_TRACE_FIXTURE,
        "--camera-sequence",
        "--camera-frame-indices",
        "2,0",
        "--camera-warmup-frames",
        "2",
        "--camera-measured-frames",
        "3",
        "--camera-loops",
        "2",
    ])
    .unwrap();
    let sequence = load_camera_trace(&mut sequence_args).unwrap().unwrap();
    let steps = crate::viewer::surface_trace_steps(&sequence).unwrap();
    assert_eq!(steps.len(), 8);
    assert_eq!(
        steps
            .iter()
            .map(|step| step.phase.as_str())
            .collect::<Vec<_>>(),
        vec![
            "warmup", "warmup", "measure", "measure", "measure", "measure", "measure", "measure"
        ]
    );
    assert_eq!(
        steps
            .iter()
            .map(|step| step.trace_frame_index)
            .collect::<Vec<_>>(),
        vec![2, 0, 2, 0, 2, 2, 0, 2]
    );
    assert_eq!(steps[2].loop_index, 0);
    assert_eq!(steps[5].loop_index, 1);
}

#[test]
fn scene_bounds_returns_min_and_max_positions() {
    let scene = scene_with_positions(vec![
        Vec3f::new(-1.0, 2.0, 0.5),
        Vec3f::new(3.0, -4.0, 8.0),
        Vec3f::new(0.0, 1.0, -2.0),
    ]);

    let (min, max) = scene_bounds(&scene).unwrap();

    assert_eq!(min, Vec3f::new(-1.0, -4.0, -2.0));
    assert_eq!(max, Vec3f::new(3.0, 2.0, 8.0));
}

#[test]
fn auto_camera_sets_valid_planes_for_deep_scene() {
    let scene = scene_with_positions(vec![
        Vec3f::new(-1.0, -1.0, 1.0),
        Vec3f::new(1.0, 1.0, 20.0),
    ]);
    let mut renderer = Renderer::new_for_surface(RenderMode::SortedAlpha).unwrap();
    renderer.load_scene(scene).unwrap();

    let camera = auto_camera(&renderer, RendererConfig::default());

    assert!(camera.intrinsics.near_plane > 0.0);
    assert!(camera.intrinsics.far_plane > camera.intrinsics.near_plane);
    assert_eq!(camera.validate(), Ok(()));
}

#[test]
fn write_png_rejects_mismatched_rgba_len() {
    let err = write_png(std::path::Path::new("target/unused.png"), 2, 2, &[0; 4]).unwrap_err();

    assert_eq!(err, "png write failed: rgba buffer size mismatch");
}
