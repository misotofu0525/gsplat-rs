use std::f32::consts::PI;
use std::time::Instant;

use gsplat_render_wgpu::Renderer;

use crate::cli::{Args, geometry_path_label};
use crate::image_output::write_png;
use crate::scene::{initial_camera, load_ply_path_into_renderer};
use crate::trace::CameraTracePlayback;

pub(crate) fn run_offscreen(
    args: &Args,
    trace_playback: Option<&CameraTracePlayback>,
) -> Result<(), String> {
    let mut renderer = Renderer::with_config(args.config).map_err(|error| error.to_string())?;
    renderer.set_geometry_path(args.geometry_path);
    load_ply_path_into_renderer(&args.dataset_path, &mut renderer)?;
    let mut camera = initial_camera(args, &renderer, trace_playback)?;
    if let Some(playback) = trace_playback {
        playback.print_header();
    }
    let start = Instant::now();
    let mut last_stats = None;
    let sequence = match trace_playback {
        Some(CameraTracePlayback::Sequence {
            trace,
            frame_indices,
            warmup_frames,
            measured_frames,
            loops,
            ..
        }) => Some(
            trace
                .sequence(
                    frame_indices.clone(),
                    *warmup_frames,
                    *measured_frames,
                    *loops,
                )
                .map_err(|error| error.to_string())?,
        ),
        _ => None,
    };
    let render_frames = match trace_playback {
        Some(CameraTracePlayback::Fixed {
            warmup_frames,
            measured_frames,
            ..
        }) => warmup_frames
            .checked_add(*measured_frames)
            .ok_or_else(|| "fixed camera trace schedule length overflowed usize".to_owned())?,
        Some(CameraTracePlayback::Sequence { .. }) => sequence
            .as_ref()
            .expect("sequence playback constructs a sequence")
            .len(),
        None => args.frames as usize,
    };
    for frame_index in 0..render_frames {
        if let Some(sequence) = &sequence {
            let step = sequence
                .step(frame_index)
                .ok_or_else(|| "camera trace sequence ended unexpectedly".to_owned())?;
            camera = step.frame.camera().map_err(|error| error.to_string())?;
            let trace = trace_playback
                .expect("a sequence can only exist with trace playback")
                .trace();
            println!(
                "CAMERA_TRACE_FRAME trace_id={} trace_sha256={} phase={} loop={} phase_frame={} frame_index={} timestamp_ns={} requested_backend=cpu",
                trace.trace_id,
                trace.content_sha256,
                step.phase.as_str(),
                step.loop_index,
                step.phase_frame_index,
                step.trace_frame_index,
                step.frame.timestamp_ns,
            );
        }
        if args.orbit && args.frames > 1 {
            let t = (frame_index as f32) / ((render_frames - 1) as f32);
            let angle = t * 2.0 * PI;
            camera.pose.rotation_xyzw = [0.0, (angle * 0.5).sin(), 0.0, (angle * 0.5).cos()];
        }

        let stats = renderer
            .render_frame(&camera)
            .map_err(|err| err.to_string())?;
        last_stats = Some(stats);
    }
    let elapsed = start.elapsed();
    let stats = last_stats.unwrap_or_default();

    if let Some(png_path) = args.png_out.as_deref() {
        let rgba = renderer.readback_rgba8().map_err(|err| err.to_string())?;
        write_png(png_path, args.config.width, args.config.height, &rgba)?;
        println!("wrote_png={}", png_path.display());
    }

    println!("desktop-example ok");
    println!("dataset={}", args.dataset_path.display());
    println!("gpu_rasterizer={}", renderer.has_gpu_rasterizer());
    println!(
        "offscreen_geometry_pipeline={}",
        geometry_path_label(args.geometry_path)
    );
    println!("frames={render_frames}");
    println!("elapsed_ms={:.4}", elapsed.as_secs_f32() * 1000.0);
    println!("frame_ms={:.4}", stats.frame_ms);
    println!("preprocess_ms={:.4}", stats.preprocess_ms);
    println!("sort_ms={:.4}", stats.sort_ms);
    println!("geometry_encode_submit_cpu_wall_ms={:.4}", stats.raster_ms);
    println!("visible_count={}", stats.visible_count);
    println!("drawn_count={}", stats.drawn_count);

    Ok(())
}
