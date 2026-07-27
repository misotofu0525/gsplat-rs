use gsplat_core::{
    Camera, ErrorCode, FrameStats, GSPLAT_API_VERSION_MAJOR, GSPLAT_API_VERSION_MINOR, RenderMode,
    RendererConfig, Vec3f,
};
use gsplat_io_ply::{
    DecodedPlySplat, IncrementalPlyDecoder, PlySceneSummary, parse_ply_bytes,
    parse_ply_bytes_summary, visit_ply_bytes_splats,
};
use gsplat_render_wgpu::{
    GeometryPath, Renderer, ResidentSceneBuilder, ResidentSourceSplat,
    SurfaceAdaptiveGpuFailureReason, SurfaceAdaptiveState, SurfaceCpuOrderMeasurement,
    SurfaceCurrentStatsCountSemantics, SurfaceCurrentStatsPlan, SurfaceCurrentStatsPoll,
    SurfaceCurrentStatsRequest, SurfaceCurrentStatsSubmission,
    SurfaceCurrentStatsSubmissionReceipt, SurfaceCurrentStatsTerminal,
    SurfaceCurrentStatsUnsampledReason, SurfaceFrameOutput, SurfaceGpuOrderProducer,
    SurfaceGpuProducerDrawScope, SurfaceGpuProducerMeasurement,
    SurfaceGpuProducerMeasurementFailure, SurfaceGpuProducerMeasurementFailureReason,
    SurfaceGpuProducerMeasurementSubmission, SurfaceGpuProducerMeasurementUnsampledReason,
    SurfaceOrderBackend, SurfaceOrderBackendUsed, SurfaceOrderMeasurement,
    SurfaceOrderMeasurementFailure, SurfaceOrderMeasurementFailureReason,
    SurfaceOrderMeasurementSubmission, SurfaceOrderMeasurementUnsampledReason,
    SurfaceProjectedDrawAdaptiveState, SurfaceProjectedDrawExecution,
    SurfaceProjectedDrawMeasurement, SurfaceProjectedDrawMeasurementFailure,
    SurfaceProjectedDrawMeasurementFailureReason, SurfaceProjectedDrawMeasurementSubmission,
    SurfaceProjectedDrawMeasurementUnsampledReason, SurfaceProjectedDrawPolicy,
    SurfaceRasterExecutionPlan, SurfaceRenderSession, SurfaceTimingSource,
};
use js_sys::{Array, Float32Array, Object, Reflect, Uint8Array};
use sha2::{Digest, Sha256};
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

#[cfg(feature = "diagnostic-web-depth-key-candidate20")]
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console, js_name = log)]
    fn diagnostic_console_log(message: &str);
}

const SURFACE_CAMERA_MAX_PITCH: f32 = 1.45;
const SURFACE_CAMERA_MIN_DISTANCE_MULTIPLIER: f32 = 0.2;
const SURFACE_CAMERA_MAX_DISTANCE_MULTIPLIER: f32 = 20.0;
#[wasm_bindgen]
pub fn api_version_major() -> u32 {
    GSPLAT_API_VERSION_MAJOR
}

#[wasm_bindgen]
pub fn api_version_minor() -> u32 {
    GSPLAT_API_VERSION_MINOR
}

#[wasm_bindgen(js_name = createRenderer)]
pub async fn create_renderer(
    canvas: HtmlCanvasElement,
    ply_bytes: Uint8Array,
    width: u32,
    height: u32,
) -> Result<GsplatWebRenderer, JsValue> {
    create_renderer_for_path(canvas, ply_bytes, width, height, GeometryPath::PackedAtlas).await
}

#[wasm_bindgen(js_name = createRendererWithGeometryPath)]
pub async fn create_renderer_with_geometry_path(
    canvas: HtmlCanvasElement,
    ply_bytes: Uint8Array,
    width: u32,
    height: u32,
    geometry_path: u32,
) -> Result<GsplatWebRenderer, JsValue> {
    let geometry_path = geometry_path_from_id(geometry_path)?;
    create_renderer_for_path(canvas, ply_bytes, width, height, geometry_path).await
}

async fn create_renderer_for_path(
    canvas: HtmlCanvasElement,
    ply_bytes: Uint8Array,
    width: u32,
    height: u32,
    geometry_path: GeometryPath,
) -> Result<GsplatWebRenderer, JsValue> {
    let raw = ply_bytes.to_vec();
    let mut renderer = Renderer::with_config_for_surface(RendererConfig {
        width,
        height,
        mode: RenderMode::SortedAlpha,
    })
    .map_err(renderer_error)?;
    renderer.set_geometry_path(geometry_path);
    let (source_summary, decoded_summary) = load_ply_bytes_into_renderer(&raw, &mut renderer)?;

    finish_surface_renderer(
        canvas,
        width,
        height,
        renderer,
        decoded_summary,
        WebLoadReceipt {
            transport_bytes: raw.len() as u64,
            peak_decoder_buffer_bytes: raw.len() as u64,
            streamed: false,
            input_sha256: format!("{:x}", Sha256::digest(&raw)),
            source_count: source_summary.gaussians,
            decoded_count: decoded_summary.gaussians,
            source_sh_degree: source_summary.sh_degree,
            encoded_count: 0,
            resident_count: 0,
            addressable_count: 0,
            resident_sh_degree: 0,
        },
    )
    .await
}

async fn finish_surface_renderer(
    canvas: HtmlCanvasElement,
    width: u32,
    height: u32,
    renderer: Renderer,
    summary: PlySceneSummary,
    load_receipt: WebLoadReceipt,
) -> Result<GsplatWebRenderer, JsValue> {
    let encoded_count = renderer
        .resident_scene()
        .map(|scene| scene.report.encoded_count)
        .or_else(|| renderer.scene_len())
        .ok_or_else(|| js_error("renderer has no resident scene after loading"))?;
    let resident_count = renderer
        .scene_len()
        .ok_or_else(|| js_error("renderer has no resident scene after loading"))?;
    let resident_sh_degree = renderer
        .scene_sh_degree()
        .ok_or_else(|| js_error("renderer has no resident SH degree after loading"))?;
    let camera_control = auto_surface_camera_control(&renderer).map_err(error_code)?;
    let camera = surface_camera_from_control(camera_control, renderer.config());
    let session = SurfaceRenderSession::from_canvas(renderer, canvas, width, height, camera)
        .await
        .map_err(renderer_error)?;
    let addressable_count = session.addressable_splat_count();

    Ok(GsplatWebRenderer {
        session,
        camera_control,
        camera_override: None,
        summary,
        load_receipt: WebLoadReceipt {
            encoded_count,
            resident_count,
            addressable_count,
            resident_sh_degree,
            ..load_receipt
        },
    })
}

fn load_ply_bytes_into_renderer(
    input: &[u8],
    renderer: &mut Renderer,
) -> Result<(PlySceneSummary, PlySceneSummary), JsValue> {
    if renderer.geometry_path() != GeometryPath::PackedAtlas {
        let source = parse_ply_bytes_summary(input).map_err(|error| js_error(error.to_string()))?;
        let loaded = parse_ply_bytes(input).map_err(|error| js_error(error.to_string()))?;
        let summary = loaded.summary;
        if summary != source {
            return Err(js_error(format!(
                "PLY summary changed while decoding: expected {source:?}, decoded {summary:?}"
            )));
        }
        renderer.load_scene(loaded.scene).map_err(renderer_error)?;
        return Ok((source, summary));
    }

    let expected = parse_ply_bytes_summary(input).map_err(|error| js_error(error.to_string()))?;
    let mut builder = ResidentSceneBuilder::new(expected.gaussians, expected.sh_degree)
        .map_err(|error| js_error(error.to_string()))?;
    let mut builder_error = None;
    let decoded = visit_ply_bytes_splats(input, |splat| {
        if builder_error.is_none() {
            builder_error = builder.push(resident_source_splat(splat)).err();
        }
    })
    .map_err(|error| js_error(error.to_string()))?;
    if let Some(error) = builder_error {
        return Err(js_error(error.to_string()));
    }
    if decoded != expected {
        return Err(js_error(format!(
            "PLY summary changed while decoding: expected {expected:?}, decoded {decoded:?}"
        )));
    }
    let resident = builder
        .finish()
        .map_err(|error| js_error(error.to_string()))?;
    renderer
        .load_resident_scene(resident)
        .map_err(renderer_error)?;
    Ok((expected, decoded))
}

fn resident_source_splat(splat: &DecodedPlySplat) -> ResidentSourceSplat {
    ResidentSourceSplat {
        position: splat.position_ruf,
        opacity_logit: splat.opacity_logit,
        log_scale: splat.log_scale_xyz,
        rotation_xyzw: splat.rotation_xyzw,
        color_dc: splat.color_dc,
        sh_rest: splat.sh_rest,
        sh_len: splat.sh_rest_len,
        sh_degree: splat.sh_degree,
    }
}

#[derive(Debug, Clone)]
struct WebLoadReceipt {
    transport_bytes: u64,
    peak_decoder_buffer_bytes: u64,
    streamed: bool,
    input_sha256: String,
    /// Header-declared source count, captured before body publication.
    source_count: usize,
    /// Rows actually accepted by the decoder.
    decoded_count: usize,
    /// Records actually committed by the resident encoder.
    encoded_count: usize,
    /// Records retained by the renderer after transactional publication.
    resident_count: usize,
    /// Records allocated by the selected GPU geometry resources.
    addressable_count: usize,
    source_sh_degree: u8,
    resident_sh_degree: u8,
}

/// Stateful browser transport boundary for exact Packed PLY loading.
///
/// JavaScript feeds `ReadableStream` chunks here. Only the incomplete PLY
/// header/row/record and one reusable JS-copy scratch buffer are retained;
/// decoded splats go directly into the final exact-count resident builder.
#[wasm_bindgen(js_name = PackedPlyStream)]
pub struct GsplatWebPackedPlyStream {
    canvas: Option<HtmlCanvasElement>,
    width: u32,
    height: u32,
    decoder: IncrementalPlyDecoder,
    builder: Option<ResidentSceneBuilder>,
    summary: Option<PlySceneSummary>,
    scratch: Vec<u8>,
    input_hasher: Sha256,
    finished: bool,
}

#[wasm_bindgen(js_name = createPackedPlyStream)]
pub fn create_packed_ply_stream(
    canvas: HtmlCanvasElement,
    width: u32,
    height: u32,
) -> Result<GsplatWebPackedPlyStream, JsValue> {
    if width == 0 || height == 0 {
        return Err(error_code(ErrorCode::InvalidArgument));
    }
    Ok(GsplatWebPackedPlyStream {
        canvas: Some(canvas),
        width,
        height,
        decoder: IncrementalPlyDecoder::default(),
        builder: None,
        summary: None,
        scratch: Vec::new(),
        input_hasher: Sha256::new(),
        finished: false,
    })
}

#[wasm_bindgen]
impl GsplatWebPackedPlyStream {
    #[wasm_bindgen(js_name = pushChunk)]
    pub fn push_chunk(&mut self, chunk: Uint8Array) -> Result<(), JsValue> {
        if self.finished {
            return Err(js_error("packed PLY stream has already been finished"));
        }
        let len = usize::try_from(chunk.length())
            .map_err(|_| js_error("PLY transport chunk exceeds addressable memory"))?;
        if self.scratch.len() < len {
            self.scratch
                .try_reserve(len - self.scratch.len())
                .map_err(|_| js_error("failed to reserve PLY transport scratch memory"))?;
            self.scratch.resize(len, 0);
        } else {
            self.scratch.truncate(len);
        }
        chunk.copy_to(&mut self.scratch);
        self.input_hasher.update(&self.scratch);

        let header = feed_incremental_ply(&mut self.decoder, &self.scratch, self.builder.as_mut())?;
        if let Some(summary) = header {
            if self.summary.is_some() || self.builder.is_some() {
                return Err(js_error("PLY stream emitted more than one header"));
            }
            self.builder = Some(
                ResidentSceneBuilder::new(summary.gaussians, summary.sh_degree)
                    .map_err(|error| js_error(error.to_string()))?,
            );
            self.summary = Some(summary);
            let repeated_header =
                feed_incremental_ply(&mut self.decoder, &[], self.builder.as_mut())?;
            if repeated_header.is_some() {
                return Err(js_error("PLY stream emitted more than one header"));
            }
        }
        Ok(())
    }

    #[wasm_bindgen(js_name = inputBytes)]
    pub fn input_bytes(&self) -> u64 {
        self.decoder.total_input_bytes() as u64
    }

    #[wasm_bindgen(js_name = peakDecoderBufferBytes)]
    pub fn peak_decoder_buffer_bytes(&self) -> u64 {
        self.decoder.peak_buffered_bytes() as u64
    }

    pub async fn finish(&mut self) -> Result<GsplatWebRenderer, JsValue> {
        if self.finished {
            return Err(js_error("packed PLY stream has already been finished"));
        }
        let mut builder = self
            .builder
            .take()
            .ok_or_else(|| js_error("PLY stream ended before a complete header"))?;
        let mut builder_error = None;
        let decoded = self
            .decoder
            .finish(|splat| {
                if builder_error.is_none() {
                    builder_error = builder.push(resident_source_splat(splat)).err();
                }
            })
            .map_err(|error| js_error(error.to_string()))?;
        if let Some(error) = builder_error {
            return Err(js_error(error.to_string()));
        }
        let expected = self
            .summary
            .ok_or_else(|| js_error("PLY stream has no validated summary"))?;
        if decoded != expected {
            return Err(js_error(format!(
                "PLY summary changed while streaming: expected {expected:?}, decoded {decoded:?}"
            )));
        }
        let resident = builder
            .finish()
            .map_err(|error| js_error(error.to_string()))?;
        let mut renderer = Renderer::with_config_for_surface(RendererConfig {
            width: self.width,
            height: self.height,
            mode: RenderMode::SortedAlpha,
        })
        .map_err(renderer_error)?;
        renderer.set_geometry_path(GeometryPath::PackedAtlas);
        renderer
            .load_resident_scene(resident)
            .map_err(renderer_error)?;

        self.finished = true;
        let canvas = self
            .canvas
            .take()
            .ok_or_else(|| js_error("packed PLY stream canvas has already been consumed"))?;
        finish_surface_renderer(
            canvas,
            self.width,
            self.height,
            renderer,
            decoded,
            WebLoadReceipt {
                transport_bytes: self.decoder.total_input_bytes() as u64,
                peak_decoder_buffer_bytes: self.decoder.peak_buffered_bytes() as u64,
                streamed: true,
                input_sha256: format!("{:x}", self.input_hasher.clone().finalize()),
                source_count: expected.gaussians,
                decoded_count: decoded.gaussians,
                source_sh_degree: expected.sh_degree,
                encoded_count: 0,
                resident_count: 0,
                addressable_count: 0,
                resident_sh_degree: 0,
            },
        )
        .await
    }
}

fn feed_incremental_ply(
    decoder: &mut IncrementalPlyDecoder,
    input: &[u8],
    mut builder: Option<&mut ResidentSceneBuilder>,
) -> Result<Option<PlySceneSummary>, JsValue> {
    let mut builder_error = None;
    let mut missing_builder = false;
    let summary = decoder
        .push(input, |splat| {
            if builder_error.is_some() || missing_builder {
                return;
            }
            if let Some(builder) = builder.as_deref_mut() {
                builder_error = builder.push(resident_source_splat(splat)).err();
            } else {
                missing_builder = true;
            }
        })
        .map_err(|error| js_error(error.to_string()))?;
    if missing_builder {
        return Err(js_error("PLY body was decoded before its validated header"));
    }
    if let Some(error) = builder_error {
        return Err(js_error(error.to_string()));
    }
    Ok(summary)
}

#[wasm_bindgen]
pub struct GsplatWebRenderer {
    session: SurfaceRenderSession,
    camera_control: SurfaceCameraControl,
    camera_override: Option<Camera>,
    summary: PlySceneSummary,
    load_receipt: WebLoadReceipt,
}

#[wasm_bindgen]
impl GsplatWebRenderer {
    /// Compatibility entrypoint. Browser size changes are asynchronous; a
    /// changed size fails closed here and must use `resizeAsync`.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), JsValue> {
        self.session.resize(width, height).map_err(renderer_error)?;
        let camera = self.camera_override.unwrap_or_else(|| {
            surface_camera_from_control(self.camera_control, self.session.renderer().config())
        });
        self.session.set_camera(camera).map_err(renderer_error)?;
        Ok(())
    }

    /// Transactionally reconfigures the production Packed + Projected WebGPU
    /// Surface and publishes the new renderer/camera dimensions only after
    /// validation/OOM/internal error scopes complete.
    #[wasm_bindgen(js_name = resizeAsync)]
    pub async fn resize_async(&mut self, width: u32, height: u32) -> Result<(), JsValue> {
        let candidate_config = RendererConfig {
            width,
            height,
            ..self.session.renderer().config()
        };
        candidate_config.validate().map_err(error_code)?;
        let camera = self
            .camera_override
            .unwrap_or_else(|| surface_camera_from_control(self.camera_control, candidate_config));
        camera.validate().map_err(error_code)?;

        self.session
            .resize_async(width, height)
            .await
            .map_err(renderer_error)?;
        self.session.set_camera(camera).map_err(renderer_error)?;
        Ok(())
    }

    #[wasm_bindgen(js_name = resetCamera)]
    pub fn reset_camera(&mut self) -> Result<(), JsValue> {
        self.camera_override = None;
        self.camera_control =
            auto_surface_camera_control(self.session.renderer()).map_err(error_code)?;
        self.apply_camera_control()?;
        self.session.force_sort_refresh();
        Ok(())
    }

    pub fn orbit(
        &mut self,
        delta_yaw_radians: f32,
        delta_pitch_radians: f32,
    ) -> Result<(), JsValue> {
        if !delta_yaw_radians.is_finite() || !delta_pitch_radians.is_finite() {
            return Err(error_code(ErrorCode::InvalidArgument));
        }

        self.camera_override = None;
        self.camera_control.yaw += delta_yaw_radians;
        self.camera_control.pitch = (self.camera_control.pitch + delta_pitch_radians)
            .clamp(-SURFACE_CAMERA_MAX_PITCH, SURFACE_CAMERA_MAX_PITCH);
        self.apply_camera_control()
    }

    pub fn zoom(&mut self, distance_scale: f32) -> Result<(), JsValue> {
        if !distance_scale.is_finite() || distance_scale <= 0.0 {
            return Err(error_code(ErrorCode::InvalidArgument));
        }

        self.camera_override = None;
        let min_distance =
            (self.camera_control.radius * SURFACE_CAMERA_MIN_DISTANCE_MULTIPLIER).max(0.01);
        let max_distance =
            (self.camera_control.radius * SURFACE_CAMERA_MAX_DISTANCE_MULTIPLIER).max(min_distance);
        self.camera_control.distance =
            (self.camera_control.distance * distance_scale).clamp(min_distance, max_distance);
        self.apply_camera_control()
    }

    pub fn pan(&mut self, normalized_delta_x: f32, normalized_delta_y: f32) -> Result<(), JsValue> {
        if !normalized_delta_x.is_finite() || !normalized_delta_y.is_finite() {
            return Err(error_code(ErrorCode::InvalidArgument));
        }

        self.camera_override = None;
        let config = self.session.renderer().config();
        let aspect = (config.width as f32 / config.height.max(1) as f32).max(1.0e-3);
        let view_height = 2.0
            * self.camera_control.distance
            * (self.session.camera().intrinsics.vertical_fov_radians * 0.5).tan();
        let view_width = view_height * aspect;
        let camera = surface_camera_from_control(self.camera_control, config);
        let (right, up, _) = camera_basis(&camera, self.camera_control.target);

        self.camera_control.target = vec3_add(
            self.camera_control.target,
            vec3_add(
                vec3_scale(right, -normalized_delta_x * view_width),
                vec3_scale(up, normalized_delta_y * view_height),
            ),
        );
        self.apply_camera_control()
    }

    #[wasm_bindgen(js_name = setSortInterval)]
    pub fn set_sort_interval(&mut self, interval: u32) {
        // Preserve the existing Web API behavior by clamping zero to one.
        let _ = self.session.set_sort_interval(interval.max(1));
    }

    /// Constructor-only compatibility setter. Repeating the active geometry
    /// path is idempotent; every changed-path request returns Unsupported.
    #[wasm_bindgen(js_name = setGeometryPath)]
    pub fn set_geometry_path(&mut self, path: u32) -> Result<(), JsValue> {
        let path = geometry_path_from_id(path)?;
        self.session.set_geometry_path(path).map_err(renderer_error)
    }

    /// Async-shaped constructor-only compatibility setter. It preserves the
    /// public Promise ABI without preparing or publishing runtime geometry:
    /// same-path requests are idempotent and changed paths return Unsupported.
    #[wasm_bindgen(js_name = setGeometryPathAsync)]
    pub async fn set_geometry_path_async(&mut self, path: u32) -> Result<(), JsValue> {
        let path = geometry_path_from_id(path)?;
        self.session
            .set_geometry_path_async(path)
            .await
            .map_err(renderer_error)
    }

    /// Builds the complete GPU sorter and its draw/projection bindings under
    /// asynchronous WebGPU error scopes. This does not change the active
    /// backend; callers then select GPU or Adaptive with `setOrderBackend`.
    #[wasm_bindgen(js_name = prepareGpuOrder)]
    pub async fn prepare_gpu_order(&mut self) -> Result<(), JsValue> {
        self.session
            .prepare_gpu_order()
            .await
            .map_err(renderer_error)
    }

    #[wasm_bindgen(js_name = setOrderBackend)]
    pub fn set_order_backend(&mut self, backend: u32) -> Result<(), JsValue> {
        let backend = match backend {
            0 => SurfaceOrderBackend::Cpu,
            1 => SurfaceOrderBackend::Gpu,
            2 => SurfaceOrderBackend::Adaptive,
            _ => return Err(error_code(ErrorCode::InvalidArgument)),
        };
        self.session
            .set_order_backend(backend)
            .map_err(renderer_error)
    }

    /// Selects Candidate, Compact, or Adaptive projected drawing. The shared
    /// session validates the complete Compact graph before changing policy,
    /// so a rejected request leaves the previously published policy live.
    #[wasm_bindgen(js_name = setProjectedPolicy)]
    pub fn set_projected_policy(&mut self, policy: u32) -> Result<(), JsValue> {
        let policy = projected_policy_from_id(policy)?;
        self.session
            .set_projected_draw_policy(policy)
            .map_err(renderer_error)
    }

    /// Builds the complete dormant Packed GPU-producer graph under WebGPU
    /// error scopes without changing the producer currently used by GPU
    /// frames. The diagnostic is intentionally admitted only for the exact
    /// Packed + ProjectedQuadsExact + forced Compact experiment context.
    #[wasm_bindgen(js_name = prepareGpuOrderProducer)]
    pub async fn prepare_gpu_order_producer(&mut self, producer: u32) -> Result<(), JsValue> {
        let producer = gpu_order_producer_from_id(producer)?;
        require_gpu_producer_experiment_context(&self.session)?;
        self.session
            .prepare_gpu_order_producer(producer)
            .await
            .map_err(renderer_error)
    }

    /// Transactionally prepares and selects the producer inside the shared
    /// Exact plan. Current-stats receipts are the sole Exact terminal ledger;
    /// the legacy producer-specific learner/receipt stream stays disabled.
    #[wasm_bindgen(js_name = setGpuOrderProducerAsync)]
    pub async fn set_gpu_order_producer_async(&mut self, producer: u32) -> Result<(), JsValue> {
        let producer = gpu_order_producer_from_id(producer)?;
        require_gpu_producer_experiment_context(&self.session)?;
        self.session
            .set_gpu_order_producer_async(producer)
            .await
            .map_err(renderer_error)
    }

    #[wasm_bindgen(js_name = gpuOrderProducer)]
    pub fn gpu_order_producer(&self) -> String {
        gpu_order_producer_label(self.session.gpu_order_producer()).to_owned()
    }

    #[wasm_bindgen(js_name = setCamera)]
    pub fn set_camera(&mut self, values: Float32Array) -> Result<(), JsValue> {
        let values = values.to_vec();
        if values.len() != 10 || values.iter().any(|value| !value.is_finite()) {
            return Err(error_code(ErrorCode::InvalidArgument));
        }
        let mut camera = Camera::default();
        camera.pose.position = Vec3f::new(values[0], values[1], values[2]);
        camera.pose.rotation_xyzw = [values[3], values[4], values[5], values[6]];
        camera.intrinsics.vertical_fov_radians = values[7];
        camera.intrinsics.near_plane = values[8];
        camera.intrinsics.far_plane = values[9];
        self.session.set_camera(camera).map_err(renderer_error)?;
        self.session.force_sort_refresh();
        self.camera_override = Some(camera);
        Ok(())
    }

    #[wasm_bindgen(js_name = cameraReceipt)]
    pub fn camera_receipt(&self) -> Float32Array {
        let camera = self.session.camera();
        Float32Array::from(
            [
                camera.pose.position.x,
                camera.pose.position.y,
                camera.pose.position.z,
                camera.pose.rotation_xyzw[0],
                camera.pose.rotation_xyzw[1],
                camera.pose.rotation_xyzw[2],
                camera.pose.rotation_xyzw[3],
                camera.intrinsics.vertical_fov_radians,
                camera.intrinsics.near_plane,
                camera.intrinsics.far_plane,
            ]
            .as_slice(),
        )
    }

    #[wasm_bindgen(js_name = rasterPath)]
    pub fn raster_path(&self) -> String {
        match self.session.geometry_path() {
            GeometryPath::SortedIndexDirect => "sorted_index_direct",
            GeometryPath::PackedAtlas => "packed_atlas",
            GeometryPath::PagedActiveAtlas => "paged_active_atlas",
        }
        .to_owned()
    }

    #[wasm_bindgen(js_name = renderFrame)]
    pub fn render_frame(&mut self) -> Result<JsValue, JsValue> {
        let output = self.session.render_frame().map_err(renderer_error)?;
        let current_stats_submission = self.session.current_stats_submission();
        let completed_cpu_order_measurements = self.session.drain_cpu_order_measurements();
        let completed_order_measurements = self.session.drain_order_measurements();
        let failed_order_measurements = self.session.drain_order_measurement_failures();
        let completed_projected_measurements = self.session.drain_projected_draw_measurements();
        let failed_projected_measurements =
            self.session.drain_projected_draw_measurement_failures();
        let completed_gpu_producer_measurements = self.session.drain_gpu_producer_measurements();
        let failed_gpu_producer_measurements =
            self.session.drain_gpu_producer_measurement_failures();
        let stats = frame_stats_object(
            output,
            self.session.surface_size(),
            self.session.internal_render_size(),
            self.session.last_presented_size(),
            current_stats_submission,
            &completed_cpu_order_measurements,
            &completed_order_measurements,
            &failed_order_measurements,
            &completed_projected_measurements,
            &failed_projected_measurements,
            &completed_gpu_producer_measurements,
            &failed_gpu_producer_measurements,
        )?;
        #[cfg(feature = "diagnostic-web-depth-key-candidate20")]
        if output.frame_presented {
            log_presented_depth_precision_receipt(&self.session);
        }
        Ok(stats)
    }

    /// Arms one renderer-owned capture for the next successfully presented
    /// diagnostic frame. This can reconfigure the WebGPU canvas Surface for
    /// COPY_SRC and therefore remains absent from ordinary builds.
    #[cfg(feature = "diagnostic-web-surface-capture")]
    #[wasm_bindgen(js_name = requestDiagnosticSurfaceCapture)]
    pub async fn request_diagnostic_surface_capture(&mut self) -> Result<(), JsValue> {
        self.session
            .request_surface_capture_async()
            .await
            .map_err(renderer_error)
    }

    /// Takes the RGBA8 bytes and all precision identities sealed by the same
    /// successful presentation. An unpresented, failed, absent or duplicate
    /// take rejects instead of returning a canvas/compositor approximation.
    #[cfg(feature = "diagnostic-web-surface-capture")]
    #[wasm_bindgen(js_name = takeDiagnosticSurfaceCapture)]
    pub async fn take_diagnostic_surface_capture(&mut self) -> Result<JsValue, JsValue> {
        let receipt = self
            .session
            .take_diagnostic_surface_capture_receipt_async()
            .await
            .map_err(renderer_error)?;
        let rgba8_sha256 = format!("{:x}", Sha256::digest(receipt.rgba8()));
        let rgba8 = Uint8Array::from(receipt.rgba8());
        let frame = receipt.frame_identity();
        let identity = Object::new();
        set_u64(&identity, "sceneGeneration", frame.scene_generation())?;
        set_u64(&identity, "cameraRevision", frame.camera_revision())?;
        set_u64(&identity, "viewportGeneration", frame.viewport_generation())?;
        set_u64(&identity, "contractGeneration", frame.contract_generation())?;
        set_u64(&identity, "planSetGeneration", frame.plan_set_generation())?;
        set_string(&identity, "planId", receipt.plan_id())?;
        set_u64(&identity, "orderGeneration", receipt.order_generation())?;
        set_u64(
            &identity,
            "presentationSequence",
            receipt.presentation_sequence(),
        )?;
        set_u32(&identity, "width", receipt.width())?;
        set_u32(&identity, "height", receipt.height())?;
        set_string(&identity, "rgba8Sha256", &rgba8_sha256)?;

        let depth = Object::new();
        set_string(&depth, "profile", receipt.depth_precision_profile())?;
        copy_capture_identity_fields(&depth, &identity)?;

        let projected = Object::new();
        set_string(
            &projected,
            "profile",
            receipt.projected_cache_precision_profile(),
        )?;
        set_u64(
            &projected,
            "axisRecordBytes",
            receipt.projected_axis_record_bytes(),
        )?;
        copy_capture_identity_fields(&projected, &identity)?;

        let resident = Object::new();
        set_string(&resident, "profile", receipt.resident_sh_codec_profile())?;
        set_u32(
            &resident,
            "mantissaBits",
            u32::from(receipt.resident_sh_mantissa_bits()),
        )?;
        set_u32(
            &resident,
            "symmetricMaxCode",
            u32::from(receipt.resident_sh_symmetric_max_code()),
        )?;
        set_u32(
            &resident,
            "pointScaleBits",
            u32::from(receipt.resident_sh_point_scale_bits()),
        )?;
        set_u32(
            &resident,
            "pointScaleMaxCode",
            u32::from(receipt.resident_sh_point_scale_max_code()),
        )?;
        set_u32(
            &resident,
            "rangeChunkSplats",
            u32::from(receipt.resident_sh_range_chunk_splats()),
        )?;
        set_u32(&resident, "sourceCount", receipt.resident_sh_source_count())?;
        set_u32(
            &resident,
            "encodedCount",
            receipt.resident_sh_encoded_count(),
        )?;
        set_u32(
            &resident,
            "residentCount",
            receipt.resident_sh_resident_count(),
        )?;
        set_u32(
            &resident,
            "addressableCount",
            receipt.resident_sh_addressable_count(),
        )?;
        set_u32(
            &resident,
            "sourceShDegree",
            u32::from(receipt.resident_sh_source_degree()),
        )?;
        set_u32(
            &resident,
            "residentShDegree",
            u32::from(receipt.resident_sh_resident_degree()),
        )?;
        set_u32(
            &resident,
            "residualCoefficientsPerSource",
            u32::from(receipt.resident_sh_residual_coefficients_per_source()),
        )?;
        set_u32(
            &resident,
            "planeCount",
            u32::from(receipt.resident_sh_plane_count()),
        )?;
        set_u32(
            &resident,
            "bytesPerSource",
            u32::from(receipt.resident_sh_bytes_per_source()),
        )?;
        copy_capture_identity_fields(&resident, &identity)?;

        let object = Object::new();
        Reflect::set(&object, &JsValue::from_str("rgba8"), &rgba8)?;
        Reflect::set(&object, &JsValue::from_str("identity"), &identity)?;
        Reflect::set(&object, &JsValue::from_str("depthPrecision"), &depth)?;
        Reflect::set(
            &object,
            &JsValue::from_str("projectedCachePrecision"),
            &projected,
        )?;
        Reflect::set(&object, &JsValue::from_str("residentSh"), &resident)?;
        Ok(object.into())
    }

    /// Requests one observer receipt from the next presented Exact frame.
    /// This does not create another controller or change renderer policy.
    #[wasm_bindgen(js_name = requestCurrentStats)]
    pub fn request_current_stats(&mut self) -> Result<JsValue, JsValue> {
        current_stats_request_object(self.session.request_current_stats())
    }

    /// Polls at most one renderer-owned current-stats terminal without
    /// submitting another frame or blocking for GPU readback.
    #[wasm_bindgen(js_name = pollCurrentStats)]
    pub fn poll_current_stats(&mut self) -> Result<JsValue, JsValue> {
        current_stats_poll_object(self.session.poll_current_stats())
    }

    /// Drains terminal order and projected-draw receipts independently of rendering.
    /// JS calls this after a fail-closed render error so a receipt collected at
    /// frame start cannot be stranded behind the failed presentation.
    #[wasm_bindgen(js_name = drainOrderMeasurementReceipts)]
    pub fn drain_order_measurement_receipts(&mut self) -> Result<JsValue, JsValue> {
        self.session.poll_order_measurement_receipts();
        let completed_cpu = self.session.drain_cpu_order_measurements();
        let completed_gpu = self.session.drain_order_measurements();
        let failed = self.session.drain_order_measurement_failures();
        let completed_projected = self.session.drain_projected_draw_measurements();
        let failed_projected = self.session.drain_projected_draw_measurement_failures();
        let completed_gpu_producer = self.session.drain_gpu_producer_measurements();
        let failed_gpu_producer = self.session.drain_gpu_producer_measurement_failures();
        measurement_receipts_object(
            &completed_cpu,
            &completed_gpu,
            &failed,
            &completed_projected,
            &failed_projected,
            &completed_gpu_producer,
            &failed_gpu_producer,
        )
    }

    #[wasm_bindgen(js_name = sceneSummary)]
    pub fn scene_summary(&self) -> Result<JsValue, JsValue> {
        let object = Object::new();
        set_u32(&object, "gaussians", self.summary.gaussians as u32)?;
        set_u32(&object, "shDegree", self.summary.sh_degree as u32)?;
        set_bool(&object, "hasShRest", self.summary.has_sh_rest)?;
        Ok(object.into())
    }

    #[wasm_bindgen(js_name = loadReceipt)]
    pub fn load_receipt(&self) -> Result<JsValue, JsValue> {
        let object = Object::new();
        set_f64(
            &object,
            "transportBytes",
            self.load_receipt.transport_bytes as f64,
        )?;
        set_f64(
            &object,
            "peakDecoderBufferBytes",
            self.load_receipt.peak_decoder_buffer_bytes as f64,
        )?;
        set_bool(&object, "streamed", self.load_receipt.streamed)?;
        set_string(&object, "inputSha256", &self.load_receipt.input_sha256)?;
        set_f64(
            &object,
            "sourceCount",
            self.load_receipt.source_count as f64,
        )?;
        set_f64(
            &object,
            "decodedCount",
            self.load_receipt.decoded_count as f64,
        )?;
        set_f64(
            &object,
            "encodedCount",
            self.load_receipt.encoded_count as f64,
        )?;
        set_f64(
            &object,
            "residentCount",
            self.load_receipt.resident_count as f64,
        )?;
        set_f64(
            &object,
            "addressableCount",
            self.load_receipt.addressable_count as f64,
        )?;
        set_u32(
            &object,
            "sourceShDegree",
            self.load_receipt.source_sh_degree as u32,
        )?;
        set_u32(
            &object,
            "residentShDegree",
            self.load_receipt.resident_sh_degree as u32,
        )?;
        // Compatibility alias retained for 0.1.x callers.
        set_u32(
            &object,
            "shDegree",
            self.load_receipt.resident_sh_degree as u32,
        )?;
        let paged = self.session.geometry_path() == GeometryPath::PagedActiveAtlas;
        let full_quality = self.load_receipt.source_count == self.load_receipt.decoded_count
            && self.load_receipt.decoded_count == self.load_receipt.encoded_count
            && self.load_receipt.encoded_count == self.load_receipt.resident_count
            && self.load_receipt.resident_count == self.load_receipt.addressable_count
            && self.load_receipt.source_sh_degree == self.load_receipt.resident_sh_degree
            && !paged;
        set_bool(&object, "fullQuality", full_quality)?;
        set_string(
            &object,
            "sourceMembership",
            if full_quality {
                "all"
            } else if paged {
                "active_subset"
            } else {
                "incomplete"
            },
        )?;
        set_bool(&object, "samplingEnabled", false)?;
        set_bool(&object, "lodEnabled", false)?;
        set_bool(
            &object,
            "partialScenePublished",
            paged || self.load_receipt.resident_count != self.load_receipt.addressable_count,
        )?;
        Ok(object.into())
    }

    #[wasm_bindgen(js_name = surfaceSize)]
    pub fn surface_size(&self) -> Result<JsValue, JsValue> {
        let (width, height) = self.session.surface_size();
        let object = Object::new();
        set_u32(&object, "width", width)?;
        set_u32(&object, "height", height)?;
        Ok(object.into())
    }
}

fn geometry_path_from_id(path: u32) -> Result<GeometryPath, JsValue> {
    match path {
        0 => Ok(GeometryPath::SortedIndexDirect),
        1 => Ok(GeometryPath::PackedAtlas),
        2 => Ok(GeometryPath::PagedActiveAtlas),
        _ => Err(error_code(ErrorCode::InvalidArgument)),
    }
}

fn projected_policy_from_id(policy: u32) -> Result<SurfaceProjectedDrawPolicy, JsValue> {
    match policy {
        0 => Ok(SurfaceProjectedDrawPolicy::Candidate),
        1 => Ok(SurfaceProjectedDrawPolicy::Compact),
        2 => Ok(SurfaceProjectedDrawPolicy::Adaptive),
        _ => Err(error_code(ErrorCode::InvalidArgument)),
    }
}

fn gpu_order_producer_from_id(producer: u32) -> Result<SurfaceGpuOrderProducer, JsValue> {
    match producer {
        0 => Ok(SurfaceGpuOrderProducer::PostSort),
        1 => Ok(SurfaceGpuOrderProducer::Preproject),
        _ => Err(error_code(ErrorCode::InvalidArgument)),
    }
}

fn require_gpu_producer_experiment_context(session: &SurfaceRenderSession) -> Result<(), JsValue> {
    if session.geometry_path() != GeometryPath::PackedAtlas
        || session.raster_execution_plan() != SurfaceRasterExecutionPlan::ProjectedQuadsExact
        || session.projected_draw_policy() != SurfaceProjectedDrawPolicy::Compact
    {
        return Err(js_error(
            "GPU producer diagnostics require Packed geometry, ProjectedQuadsExact raster, and forced Compact projected drawing",
        ));
    }
    Ok(())
}

impl GsplatWebRenderer {
    fn apply_camera_control(&mut self) -> Result<(), JsValue> {
        let camera =
            surface_camera_from_control(self.camera_control, self.session.renderer().config());
        self.session.set_camera(camera).map_err(renderer_error)
    }
}

#[derive(Debug, Clone, Copy)]
struct SurfaceCameraControl {
    target: Vec3f,
    radius: f32,
    yaw: f32,
    pitch: f32,
    distance: f32,
}

fn auto_surface_camera_control(renderer: &Renderer) -> Result<SurfaceCameraControl, ErrorCode> {
    let Some(positions) = renderer.positions() else {
        return Err(ErrorCode::SceneNotLoaded);
    };
    let Some((min, max)) = scene_bounds(positions) else {
        return Err(ErrorCode::InvalidArgument);
    };

    let config = renderer.config();
    let center = Vec3f::new(
        (min.x + max.x) * 0.5,
        (min.y + max.y) * 0.5,
        (min.z + max.z) * 0.5,
    );
    let extent = Vec3f::new(max.x - min.x, max.y - min.y, max.z - min.z);
    let half_x = (extent.x * 0.5).max(1e-3);
    let half_y = (extent.y * 0.5).max(1e-3);
    let half_z = (extent.z * 0.5).max(1e-3);

    let aspect = (config.width as f32 / config.height.max(1) as f32).max(1.0e-3);
    let vfov = Camera::default().intrinsics.vertical_fov_radians.max(1e-3);
    let hfov = 2.0 * ((vfov * 0.5).tan() * aspect).atan();

    let dist_y = half_y / (vfov * 0.5).tan();
    let dist_x = half_x / (hfov * 0.5).tan();
    let dist = (dist_y.max(dist_x) + half_z) * 1.2;
    let radius = half_x.max(half_y).max(half_z);

    Ok(SurfaceCameraControl {
        target: center,
        radius,
        yaw: 0.0,
        pitch: 0.0,
        distance: dist,
    })
}

fn surface_camera_from_control(control: SurfaceCameraControl, config: RendererConfig) -> Camera {
    let mut camera = Camera::default();
    let pitch = control
        .pitch
        .clamp(-SURFACE_CAMERA_MAX_PITCH, SURFACE_CAMERA_MAX_PITCH);
    let cos_pitch = pitch.cos();
    let offset = Vec3f::new(
        control.yaw.sin() * cos_pitch * control.distance,
        pitch.sin() * control.distance,
        -control.yaw.cos() * cos_pitch * control.distance,
    );
    camera.pose.position = vec3_add(control.target, offset);
    camera.pose.rotation_xyzw = camera_rotation_looking_at(camera.pose.position, control.target);

    let radius = control.radius.max(1.0e-3);
    camera.intrinsics.near_plane = (control.distance - radius * 2.0).max(0.01);
    camera.intrinsics.far_plane = (control.distance + radius * 8.0).max(100.0);

    let aspect = (config.width as f32 / config.height.max(1) as f32).max(1.0e-3);
    if aspect < 0.6 {
        camera.intrinsics.vertical_fov_radians = 65.0_f32.to_radians();
    }

    camera
}

fn camera_rotation_looking_at(position: Vec3f, target: Vec3f) -> [f32; 4] {
    let (_, _, forward) = camera_basis_from_position(position, target);
    let world_up = if forward.y.abs() > 0.98 {
        Vec3f::new(0.0, 0.0, 1.0)
    } else {
        Vec3f::new(0.0, 1.0, 0.0)
    };
    let right = vec3_normalize(vec3_cross(world_up, forward)).unwrap_or(Vec3f::new(1.0, 0.0, 0.0));
    let up = vec3_cross(forward, right);
    quat_from_camera_basis(right, up, forward)
}

fn camera_basis(camera: &Camera, target: Vec3f) -> (Vec3f, Vec3f, Vec3f) {
    camera_basis_from_position(camera.pose.position, target)
}

fn camera_basis_from_position(position: Vec3f, target: Vec3f) -> (Vec3f, Vec3f, Vec3f) {
    let forward = vec3_normalize(vec3_sub(target, position)).unwrap_or(Vec3f::new(0.0, 0.0, 1.0));
    let world_up = if forward.y.abs() > 0.98 {
        Vec3f::new(0.0, 0.0, 1.0)
    } else {
        Vec3f::new(0.0, 1.0, 0.0)
    };
    let right = vec3_normalize(vec3_cross(world_up, forward)).unwrap_or(Vec3f::new(1.0, 0.0, 0.0));
    let up = vec3_cross(forward, right);
    (right, up, forward)
}

fn quat_from_camera_basis(right: Vec3f, up: Vec3f, forward: Vec3f) -> [f32; 4] {
    let m00 = right.x;
    let m01 = up.x;
    let m02 = forward.x;
    let m10 = right.y;
    let m11 = up.y;
    let m12 = forward.y;
    let m20 = right.z;
    let m21 = up.z;
    let m22 = forward.z;
    let trace = m00 + m11 + m22;

    let (x, y, z, w) = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        ((m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, 0.25 * s)
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        (0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s)
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        ((m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s)
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        ((m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s)
    };

    let norm = (x * x + y * y + z * z + w * w).sqrt().max(1.0e-6);
    [x / norm, y / norm, z / norm, w / norm]
}

fn vec3_add(a: Vec3f, b: Vec3f) -> Vec3f {
    Vec3f::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

fn vec3_sub(a: Vec3f, b: Vec3f) -> Vec3f {
    Vec3f::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

fn vec3_scale(v: Vec3f, scale: f32) -> Vec3f {
    Vec3f::new(v.x * scale, v.y * scale, v.z * scale)
}

fn vec3_cross(a: Vec3f, b: Vec3f) -> Vec3f {
    Vec3f::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

fn vec3_normalize(v: Vec3f) -> Option<Vec3f> {
    let len = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
    if !len.is_finite() || len <= 1.0e-6 {
        return None;
    }

    Some(vec3_scale(v, 1.0 / len))
}

fn scene_bounds(positions: &[Vec3f]) -> Option<(Vec3f, Vec3f)> {
    if positions.is_empty() {
        return None;
    }
    let mut min = Vec3f::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
    let mut max = Vec3f::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for p in positions {
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        min.z = min.z.min(p.z);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y);
        max.z = max.z.max(p.z);
    }
    Some((min, max))
}

fn current_stats_request_object(request: SurfaceCurrentStatsRequest) -> Result<JsValue, JsValue> {
    let object = Object::new();
    match request {
        SurfaceCurrentStatsRequest::Requested => {
            set_string(&object, "status", "requested")?;
            set_null(&object, "reason")?;
        }
        SurfaceCurrentStatsRequest::Unsampled(reason) => {
            set_string(&object, "status", "unsampled")?;
            set_string(
                &object,
                "reason",
                current_stats_unsampled_reason_label(reason),
            )?;
        }
    }
    Ok(object.into())
}

fn current_stats_poll_object(poll: SurfaceCurrentStatsPoll) -> Result<JsValue, JsValue> {
    let object = Object::new();
    match poll {
        SurfaceCurrentStatsPoll::Empty => {
            set_string(&object, "status", "empty")?;
        }
        SurfaceCurrentStatsPoll::Unsampled(reason) => {
            set_string(&object, "status", "unsampled")?;
            set_string(
                &object,
                "reason",
                current_stats_unsampled_reason_label(reason),
            )?;
        }
        SurfaceCurrentStatsPoll::Terminal(terminal) => {
            let (status, submission) = match terminal {
                SurfaceCurrentStatsTerminal::Ready(receipt) => {
                    let counts = receipt.counts();
                    set_u32(&object, "sourceCount", counts.source())?;
                    set_u32(&object, "visibleCount", counts.visible())?;
                    set_u32(&object, "contributorCount", counts.contributor())?;
                    set_u32(&object, "drawnCount", counts.drawn())?;
                    set_string(
                        &object,
                        "countSemantics",
                        current_stats_count_semantics_label(receipt.count_semantics()),
                    )?;
                    ("ready", receipt.submission())
                }
                SurfaceCurrentStatsTerminal::MapFailure(failure) => {
                    ("map_failure", failure.submission())
                }
                SurfaceCurrentStatsTerminal::GenerationInvalidated(failure) => {
                    ("generation_invalidated", failure.submission())
                }
                SurfaceCurrentStatsTerminal::Expired(failure) => ("expired", failure.submission()),
                SurfaceCurrentStatsTerminal::Dropped(failure) => ("dropped", failure.submission()),
            };
            set_string(&object, "status", status)?;
            set_current_stats_receipt_fields(&object, submission)?;
        }
    }
    Ok(object.into())
}

fn set_current_stats_submission_fields(
    object: &Object,
    submission: SurfaceCurrentStatsSubmission,
) -> Result<(), JsValue> {
    match submission {
        SurfaceCurrentStatsSubmission::NotRequested => {
            set_string(object, "currentStatsSubmission", "not_requested")?;
            for key in [
                "currentStatsTicket",
                "currentStatsPlan",
                "currentStatsSceneGeneration",
                "currentStatsCameraRevision",
                "currentStatsViewportGeneration",
                "currentStatsContractGeneration",
                "currentStatsPlanSetGeneration",
                "currentStatsOrderGeneration",
                "currentStatsRasterGeneration",
                "currentStatsEncodeAttempt",
                "currentStatsPresentationSequence",
            ] {
                set_null(object, key)?;
            }
        }
        SurfaceCurrentStatsSubmission::Issued(receipt) => {
            set_string(object, "currentStatsSubmission", "issued")?;
            set_current_stats_receipt_fields_prefixed(object, receipt, "currentStats")?;
        }
    }
    Ok(())
}

#[cfg(feature = "diagnostic-web-surface-capture")]
fn copy_capture_identity_fields(target: &Object, identity: &Object) -> Result<(), JsValue> {
    for key in [
        "sceneGeneration",
        "cameraRevision",
        "viewportGeneration",
        "contractGeneration",
        "planSetGeneration",
        "planId",
        "orderGeneration",
        "presentationSequence",
        "width",
        "height",
        "rgba8Sha256",
    ] {
        let key = JsValue::from_str(key);
        let value = Reflect::get(identity, &key)?;
        Reflect::set(target, &key, &value)?;
    }
    Ok(())
}

fn set_current_stats_receipt_fields(
    object: &Object,
    receipt: SurfaceCurrentStatsSubmissionReceipt,
) -> Result<(), JsValue> {
    set_current_stats_receipt_fields_prefixed(object, receipt, "")
}

fn set_current_stats_receipt_fields_prefixed(
    object: &Object,
    receipt: SurfaceCurrentStatsSubmissionReceipt,
    prefix: &str,
) -> Result<(), JsValue> {
    let join = receipt.join();
    let frame = join.frame_identity();
    let key = |suffix: &str| {
        if prefix.is_empty() {
            let mut chars = suffix.chars();
            match chars.next() {
                Some(first) => first.to_ascii_lowercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        } else {
            format!("{prefix}{suffix}")
        }
    };
    set_u64(object, &key("Ticket"), receipt.ticket())?;
    set_string(
        object,
        &key("Plan"),
        current_stats_plan_label(join.executed_plan()),
    )?;
    set_u64(object, &key("SceneGeneration"), frame.scene_generation())?;
    set_u64(object, &key("CameraRevision"), frame.camera_revision())?;
    set_u64(
        object,
        &key("ViewportGeneration"),
        frame.viewport_generation(),
    )?;
    set_u64(
        object,
        &key("ContractGeneration"),
        frame.contract_generation(),
    )?;
    set_u64(
        object,
        &key("PlanSetGeneration"),
        frame.plan_set_generation(),
    )?;
    set_u64(object, &key("OrderGeneration"), join.order_generation())?;
    set_u64(object, &key("RasterGeneration"), join.raster_generation())?;
    set_u64(object, &key("EncodeAttempt"), join.encode_attempt())?;
    set_u64(
        object,
        &key("PresentationSequence"),
        join.presentation_sequence(),
    )?;
    Ok(())
}

const fn current_stats_plan_label(plan: SurfaceCurrentStatsPlan) -> &'static str {
    match plan {
        SurfaceCurrentStatsPlan::CpuPostSort => "cpu_post_sort",
        SurfaceCurrentStatsPlan::GpuPostSort => "gpu_post_sort",
        SurfaceCurrentStatsPlan::GpuPreproject => "gpu_preproject",
    }
}

#[cfg(feature = "diagnostic-web-depth-key-candidate20")]
fn log_presented_depth_precision_receipt(session: &SurfaceRenderSession) {
    let Some(receipt) = session.diagnostic_presented_depth_precision_receipt() else {
        return;
    };
    diagnostic_console_log(&format!(
        concat!(
            "GSPLAT_DIAGNOSTIC_PRESENTED_DEPTH_PRECISION ",
            "{{\"record_type\":\"presented_depth_precision\",",
            "\"depth_precision_profile\":\"{}\",",
            "\"scene_generation\":{},\"camera_revision\":{},",
            "\"viewport_generation\":{},\"contract_generation\":{},",
            "\"plan_set_generation\":{},\"plan_id\":\"{}\",",
            "\"order_generation\":{},\"presentation_sequence\":{}}}"
        ),
        receipt.depth_precision_profile(),
        receipt.scene_generation(),
        receipt.camera_revision(),
        receipt.viewport_generation(),
        receipt.contract_generation(),
        receipt.plan_set_generation(),
        receipt.plan_id(),
        receipt.order_generation(),
        receipt.presentation_sequence(),
    ));
}

const fn current_stats_count_semantics_label(
    semantics: SurfaceCurrentStatsCountSemantics,
) -> &'static str {
    match semantics {
        SurfaceCurrentStatsCountSemantics::DirectDrawEqualsVisible => "draw_equals_visible",
        SurfaceCurrentStatsCountSemantics::IndirectDrawEqualsVisible => {
            "indirect_draw_equals_visible"
        }
        SurfaceCurrentStatsCountSemantics::IndirectDrawEqualsContributor => {
            "indirect_draw_equals_contributor"
        }
    }
}

const fn current_stats_unsampled_reason_label(
    reason: SurfaceCurrentStatsUnsampledReason,
) -> &'static str {
    match reason {
        SurfaceCurrentStatsUnsampledReason::Busy => "busy",
        SurfaceCurrentStatsUnsampledReason::GpuUnavailable => "gpu_unavailable",
        SurfaceCurrentStatsUnsampledReason::ResourceUnavailable => "resource_unavailable",
        SurfaceCurrentStatsUnsampledReason::TicketExhausted => "ticket_exhausted",
    }
}

fn frame_stats_object(
    output: SurfaceFrameOutput,
    surface_size: (u32, u32),
    internal_render_size: (u32, u32),
    presented_size: Option<(u32, u32)>,
    current_stats_submission: SurfaceCurrentStatsSubmission,
    completed_cpu_order_measurements: &[SurfaceCpuOrderMeasurement],
    completed_order_measurements: &[SurfaceOrderMeasurement],
    failed_order_measurements: &[SurfaceOrderMeasurementFailure],
    completed_projected_measurements: &[SurfaceProjectedDrawMeasurement],
    failed_projected_measurements: &[SurfaceProjectedDrawMeasurementFailure],
    completed_gpu_producer_measurements: &[SurfaceGpuProducerMeasurement],
    failed_gpu_producer_measurements: &[SurfaceGpuProducerMeasurementFailure],
) -> Result<JsValue, JsValue> {
    let stats: FrameStats = output.stats;
    let current_visible_count =
        (output.frame_presented && !output.visible_count_pending).then_some(stats.visible_count);
    let current_drawn_count =
        (output.frame_presented && !output.visible_count_pending).then_some(stats.drawn_count);
    let timings = output.timings;
    let object = Object::new();
    set_current_stats_submission_fields(&object, current_stats_submission)?;
    set_f32(&object, "frameMs", stats.frame_ms)?;
    set_f32(&object, "preprocessMs", stats.preprocess_ms)?;
    set_f32(&object, "sortMs", stats.sort_ms)?;
    set_f32(&object, "rasterMs", stats.raster_ms)?;
    set_f32(&object, "cpuGeometryMs", timings.cpu_geometry_ms)?;
    set_f32(&object, "renderSubmitMs", timings.render_submit_ms)?;
    set_f32(&object, "frameWallMs", timings.frame_wall_ms)?;
    set_bool(&object, "framePresented", output.frame_presented)?;
    set_bool(
        &object,
        "gpuOrderPreparationPending",
        output.gpu_order_preparation_pending,
    )?;
    set_string(
        &object,
        "rasterExecutionPlan",
        match output.raster_execution_plan {
            SurfaceRasterExecutionPlan::GlobalQuads => "global_quads",
            SurfaceRasterExecutionPlan::ProjectedQuadsExact => "projected_quads_exact",
        },
    )?;
    set_optional_u32(&object, "visibleCount", current_visible_count)?;
    set_optional_u32(&object, "drawnCount", current_drawn_count)?;
    set_bool(&object, "refreshSort", output.sort_refreshed)?;
    set_string(
        &object,
        "orderBackend",
        match output.order_backend {
            SurfaceOrderBackendUsed::Cpu => "cpu",
            SurfaceOrderBackendUsed::Gpu => "gpu",
        },
    )?;
    set_optional_string(
        &object,
        "adaptiveGpuFailure",
        output.adaptive_gpu_failure.map(|reason| match reason {
            SurfaceAdaptiveGpuFailureReason::Unsupported => "unsupported",
            SurfaceAdaptiveGpuFailureReason::Initialization => "initialization",
            SurfaceAdaptiveGpuFailureReason::OutOfMemory => "out_of_memory",
            SurfaceAdaptiveGpuFailureReason::Validation => "validation",
        }),
    )?;
    set_bool(&object, "gpuSortFallback", output.gpu_sort_fallback)?;
    set_string(
        &object,
        "adaptiveState",
        match output.adaptive_state {
            SurfaceAdaptiveState::Disabled => "disabled",
            SurfaceAdaptiveState::CpuLearning => "cpu_learning",
            SurfaceAdaptiveState::CpuStable => "cpu_stable",
            SurfaceAdaptiveState::GpuProbe => "gpu_probe",
            SurfaceAdaptiveState::GpuStable => "gpu_stable",
            SurfaceAdaptiveState::CpuProbe => "cpu_probe",
            SurfaceAdaptiveState::Cooldown => "cooldown",
        },
    )?;
    set_string(
        &object,
        "projectedPolicy",
        projected_policy_label(output.projected_draw_policy),
    )?;
    set_string(
        &object,
        "projectedExecution",
        projected_execution_label(output.projected_draw_execution),
    )?;
    set_string(
        &object,
        "projectedAdaptiveState",
        projected_adaptive_state_label(output.projected_draw_adaptive_state),
    )?;
    match output.projected_draw_measurement_submission {
        SurfaceProjectedDrawMeasurementSubmission::NotRequested => {
            set_string(&object, "projectedMeasurementSubmission", "not_requested")?;
            set_null(&object, "projectedMeasurementTicket")?;
            set_null(&object, "projectedMeasurementExecution")?;
            set_null(&object, "projectedMeasurementUnsampledReason")?;
        }
        SurfaceProjectedDrawMeasurementSubmission::Issued { execution, ticket } => {
            set_string(&object, "projectedMeasurementSubmission", "issued")?;
            set_u64(&object, "projectedMeasurementTicket", ticket)?;
            set_string(
                &object,
                "projectedMeasurementExecution",
                projected_execution_label(execution),
            )?;
            set_null(&object, "projectedMeasurementUnsampledReason")?;
        }
        SurfaceProjectedDrawMeasurementSubmission::Unsampled { execution, reason } => {
            set_string(&object, "projectedMeasurementSubmission", "unsampled")?;
            set_null(&object, "projectedMeasurementTicket")?;
            set_string(
                &object,
                "projectedMeasurementExecution",
                projected_execution_label(execution),
            )?;
            set_string(
                &object,
                "projectedMeasurementUnsampledReason",
                projected_unsampled_reason_label(reason),
            )?;
        }
    }
    set_optional_string(
        &object,
        "gpuOrderProducer",
        output.gpu_order_producer.map(gpu_order_producer_label),
    )?;
    match output.gpu_producer_measurement_submission {
        SurfaceGpuProducerMeasurementSubmission::NotRequested => {
            set_string(&object, "gpuProducerMeasurementSubmission", "not_requested")?;
            set_null(&object, "gpuProducerMeasurementTicket")?;
            set_null(&object, "gpuProducerMeasurementProducer")?;
            set_null(&object, "gpuProducerMeasurementUnsampledReason")?;
        }
        SurfaceGpuProducerMeasurementSubmission::Issued { producer, ticket } => {
            set_string(&object, "gpuProducerMeasurementSubmission", "issued")?;
            set_u64(&object, "gpuProducerMeasurementTicket", ticket)?;
            set_string(
                &object,
                "gpuProducerMeasurementProducer",
                gpu_order_producer_label(producer),
            )?;
            set_null(&object, "gpuProducerMeasurementUnsampledReason")?;
        }
        SurfaceGpuProducerMeasurementSubmission::Unsampled { producer, reason } => {
            set_string(&object, "gpuProducerMeasurementSubmission", "unsampled")?;
            set_null(&object, "gpuProducerMeasurementTicket")?;
            set_string(
                &object,
                "gpuProducerMeasurementProducer",
                gpu_order_producer_label(producer),
            )?;
            set_string(
                &object,
                "gpuProducerMeasurementUnsampledReason",
                gpu_producer_unsampled_reason_label(reason),
            )?;
        }
    }
    set_u64(&object, "cameraRevision", output.camera_revision)?;
    set_u64(
        &object,
        "appliedOrderRevision",
        output.applied_order_revision,
    )?;
    set_u32(
        &object,
        "presentedOrderRevisionLag",
        output.presented_order_revision_lag,
    )?;
    set_optional_u64(
        &object,
        "submittedMeasurementTicket",
        output.submitted_measurement_ticket,
    )?;
    match output.order_measurement_submission {
        SurfaceOrderMeasurementSubmission::NotRequested => {
            set_null(&object, "submittedMeasurementBackend")?;
            set_null(&object, "measurementUnsampledReason")?;
        }
        SurfaceOrderMeasurementSubmission::Issued { backend, .. } => {
            set_string(
                &object,
                "submittedMeasurementBackend",
                order_backend_label(backend),
            )?;
            set_null(&object, "measurementUnsampledReason")?;
        }
        SurfaceOrderMeasurementSubmission::Unsampled { backend, reason } => {
            set_string(
                &object,
                "submittedMeasurementBackend",
                order_backend_label(backend),
            )?;
            set_string(
                &object,
                "measurementUnsampledReason",
                match reason {
                    SurfaceOrderMeasurementUnsampledReason::RingBusy => "ring_busy",
                    SurfaceOrderMeasurementUnsampledReason::SurfaceUnavailable => {
                        "surface_unavailable"
                    }
                },
            )?;
        }
    }
    set_optional_u64(
        &object,
        "visibleCountRevision",
        output.visible_count_revision,
    )?;
    set_bool(&object, "visibleCountPending", output.visible_count_pending)?;
    set_bool(
        &object,
        "gpuTimestampQueriesEnabled",
        output.gpu_timestamp_queries_enabled,
    )?;
    if let Some(measurement) = output.completed_order_measurement {
        set_bool(&object, "completedMeasurementAvailable", true)?;
        set_u64(&object, "completedMeasurementTicket", measurement.ticket)?;
        set_u64(
            &object,
            "completedMeasurementRevision",
            measurement.camera_revision,
        )?;
        set_string(
            &object,
            "completedMeasurementTimingSource",
            match measurement.timing_source {
                SurfaceTimingSource::TimestampQuery => "timestamp_query",
                SurfaceTimingSource::CompletionOnly => "completion_only",
            },
        )?;
        set_optional_f32(&object, "gpuPreprocessMs", measurement.gpu_preprocess_ms)?;
        set_optional_f32(&object, "gpuRadixMs", measurement.gpu_radix_ms)?;
        set_optional_f32(&object, "gpuOrderMs", measurement.gpu_order_ms)?;
        set_f32(&object, "gpuCompleteMs", measurement.gpu_complete_ms)?;
        set_optional_f32(
            &object,
            "gpuTimestampPeriodNs",
            measurement.timestamp_period_ns,
        )?;
        set_bool(
            &object,
            "gpuBelowTimestampResolution",
            measurement.below_timestamp_resolution,
        )?;
        set_u32(&object, "completedVisibleCount", measurement.visible_count)?;
        set_u32(
            &object,
            "completedContributorCount",
            measurement.contributor_count,
        )?;
        set_u32(&object, "completedDrawnCount", measurement.drawn_count)?;
        set_bool(
            &object,
            "completedExactContributorCompaction",
            measurement.exact_contributor_compaction,
        )?;
    } else {
        set_bool(&object, "completedMeasurementAvailable", false)?;
        for key in [
            "completedMeasurementTicket",
            "completedMeasurementRevision",
            "completedMeasurementTimingSource",
            "gpuPreprocessMs",
            "gpuRadixMs",
            "gpuOrderMs",
            "gpuCompleteMs",
            "gpuTimestampPeriodNs",
            "gpuBelowTimestampResolution",
            "completedVisibleCount",
            "completedContributorCount",
            "completedDrawnCount",
            "completedExactContributorCompaction",
        ] {
            set_null(&object, key)?;
        }
    }
    if let Some(failure) = output.completed_order_measurement_failure {
        set_bool(&object, "failedMeasurementAvailable", true)?;
        set_u64(&object, "failedMeasurementTicket", failure.ticket)?;
        set_u64(
            &object,
            "failedMeasurementRevision",
            failure.camera_revision,
        )?;
        set_string(
            &object,
            "failedMeasurementReason",
            order_measurement_failure_reason(failure.reason),
        )?;
    } else {
        set_bool(&object, "failedMeasurementAvailable", false)?;
        for key in [
            "failedMeasurementTicket",
            "failedMeasurementRevision",
            "failedMeasurementReason",
        ] {
            set_null(&object, key)?;
        }
    }
    if let Some(measurement) = output.completed_projected_draw_measurement {
        set_bool(&object, "completedProjectedMeasurementAvailable", true)?;
        set_u64(
            &object,
            "completedProjectedMeasurementTicket",
            measurement.ticket,
        )?;
        set_u64(
            &object,
            "completedProjectedMeasurementRevision",
            measurement.camera_revision,
        )?;
        set_string(
            &object,
            "completedProjectedMeasurementExecution",
            projected_execution_label(measurement.execution),
        )?;
        set_string(
            &object,
            "completedProjectedMeasurementOrderBackend",
            order_backend_label(measurement.order_backend),
        )?;
        set_u64(
            &object,
            "completedProjectedProjectionGeneration",
            measurement.projection_generation,
        )?;
        set_u64(
            &object,
            "completedProjectedProbeGeneration",
            measurement.probe_generation,
        )?;
        set_bool(
            &object,
            "completedProjectedProjectionRebuilt",
            measurement.projection_rebuilt,
        )?;
        set_bool(
            &object,
            "completedProjectedOrderRefreshed",
            measurement.order_refreshed,
        )?;
        set_f32(
            &object,
            "completedProjectedFrameCompleteMs",
            measurement.frame_complete_ms,
        )?;
        set_u32(
            &object,
            "completedProjectedVisibleCount",
            measurement.visible_count,
        )?;
        set_u32(
            &object,
            "completedProjectedContributorCount",
            measurement.contributor_count,
        )?;
        set_u32(
            &object,
            "completedProjectedDrawnCount",
            measurement.drawn_count,
        )?;
        set_bool(
            &object,
            "completedProjectedExactContributorCompaction",
            measurement.exact_contributor_compaction,
        )?;
    } else {
        set_bool(&object, "completedProjectedMeasurementAvailable", false)?;
        for key in [
            "completedProjectedMeasurementTicket",
            "completedProjectedMeasurementRevision",
            "completedProjectedMeasurementExecution",
            "completedProjectedMeasurementOrderBackend",
            "completedProjectedProjectionGeneration",
            "completedProjectedProbeGeneration",
            "completedProjectedProjectionRebuilt",
            "completedProjectedOrderRefreshed",
            "completedProjectedFrameCompleteMs",
            "completedProjectedVisibleCount",
            "completedProjectedContributorCount",
            "completedProjectedDrawnCount",
            "completedProjectedExactContributorCompaction",
        ] {
            set_null(&object, key)?;
        }
    }
    if let Some(failure) = output.completed_projected_draw_measurement_failure {
        set_bool(&object, "failedProjectedMeasurementAvailable", true)?;
        set_u64(&object, "failedProjectedMeasurementTicket", failure.ticket)?;
        set_u64(
            &object,
            "failedProjectedMeasurementRevision",
            failure.camera_revision,
        )?;
        set_string(
            &object,
            "failedProjectedMeasurementExecution",
            projected_execution_label(failure.execution),
        )?;
        set_string(
            &object,
            "failedProjectedMeasurementOrderBackend",
            order_backend_label(failure.order_backend),
        )?;
        set_u64(
            &object,
            "failedProjectedProjectionGeneration",
            failure.projection_generation,
        )?;
        set_u64(
            &object,
            "failedProjectedProbeGeneration",
            failure.probe_generation,
        )?;
        set_string(
            &object,
            "failedProjectedMeasurementReason",
            projected_failure_reason_label(failure.reason),
        )?;
    } else {
        set_bool(&object, "failedProjectedMeasurementAvailable", false)?;
        for key in [
            "failedProjectedMeasurementTicket",
            "failedProjectedMeasurementRevision",
            "failedProjectedMeasurementExecution",
            "failedProjectedMeasurementOrderBackend",
            "failedProjectedProjectionGeneration",
            "failedProjectedProbeGeneration",
            "failedProjectedMeasurementReason",
        ] {
            set_null(&object, key)?;
        }
    }
    if let Some(measurement) = output.completed_gpu_producer_measurement {
        set_bool(&object, "completedGpuProducerMeasurementAvailable", true)?;
        set_u64(
            &object,
            "completedGpuProducerMeasurementTicket",
            measurement.ticket,
        )?;
        set_u64(
            &object,
            "completedGpuProducerMeasurementRevision",
            measurement.camera_revision,
        )?;
        set_string(
            &object,
            "completedGpuProducerMeasurementProducer",
            gpu_order_producer_label(measurement.producer),
        )?;
        set_u64(
            &object,
            "completedGpuProducerOrderGeneration",
            measurement.order_generation,
        )?;
        set_u64(
            &object,
            "completedGpuProducerProjectionGeneration",
            measurement.projection_generation,
        )?;
        set_u32(
            &object,
            "completedGpuProducerSourceCount",
            measurement.source_count,
        )?;
        set_u32(
            &object,
            "completedGpuProducerContributorCount",
            measurement.contributor_count,
        )?;
        set_u32(
            &object,
            "completedGpuProducerDrawnCount",
            measurement.drawn_count,
        )?;
        set_bool(
            &object,
            "completedGpuProducerOrderRefreshed",
            measurement.order_refreshed,
        )?;
        set_string(
            &object,
            "completedGpuProducerDrawScope",
            gpu_producer_draw_scope_label(measurement.draw_scope),
        )?;
        set_bool(
            &object,
            "completedGpuProducerExactCurrentContributorDraw",
            measurement.exact_current_contributor_draw(),
        )?;
        set_bool(
            &object,
            "completedGpuProducerStaleOrder",
            measurement.stale_order(),
        )?;
        set_f32(
            &object,
            "completedGpuProducerQueueCompleteMs",
            measurement.frame_complete_ms,
        )?;
    } else {
        set_bool(&object, "completedGpuProducerMeasurementAvailable", false)?;
        for key in [
            "completedGpuProducerMeasurementTicket",
            "completedGpuProducerMeasurementRevision",
            "completedGpuProducerMeasurementProducer",
            "completedGpuProducerOrderGeneration",
            "completedGpuProducerProjectionGeneration",
            "completedGpuProducerSourceCount",
            "completedGpuProducerContributorCount",
            "completedGpuProducerDrawnCount",
            "completedGpuProducerOrderRefreshed",
            "completedGpuProducerDrawScope",
            "completedGpuProducerExactCurrentContributorDraw",
            "completedGpuProducerStaleOrder",
            "completedGpuProducerQueueCompleteMs",
        ] {
            set_null(&object, key)?;
        }
    }
    if let Some(failure) = output.completed_gpu_producer_measurement_failure {
        set_bool(&object, "failedGpuProducerMeasurementAvailable", true)?;
        set_u64(
            &object,
            "failedGpuProducerMeasurementTicket",
            failure.ticket,
        )?;
        set_u64(
            &object,
            "failedGpuProducerMeasurementRevision",
            failure.camera_revision,
        )?;
        set_string(
            &object,
            "failedGpuProducerMeasurementProducer",
            gpu_order_producer_label(failure.producer),
        )?;
        set_u64(
            &object,
            "failedGpuProducerOrderGeneration",
            failure.order_generation,
        )?;
        set_u64(
            &object,
            "failedGpuProducerProjectionGeneration",
            failure.projection_generation,
        )?;
        set_string(
            &object,
            "failedGpuProducerMeasurementReason",
            gpu_producer_failure_reason_label(failure.reason),
        )?;
    } else {
        set_bool(&object, "failedGpuProducerMeasurementAvailable", false)?;
        for key in [
            "failedGpuProducerMeasurementTicket",
            "failedGpuProducerMeasurementRevision",
            "failedGpuProducerMeasurementProducer",
            "failedGpuProducerOrderGeneration",
            "failedGpuProducerProjectionGeneration",
            "failedGpuProducerMeasurementReason",
        ] {
            set_null(&object, key)?;
        }
    }
    let receipts = measurement_receipts_object(
        completed_cpu_order_measurements,
        completed_order_measurements,
        failed_order_measurements,
        completed_projected_measurements,
        failed_projected_measurements,
        completed_gpu_producer_measurements,
        failed_gpu_producer_measurements,
    )?;
    for key in [
        "completedCpuOrderMeasurements",
        "completedOrderMeasurements",
        "failedOrderMeasurements",
        "completedProjectedMeasurements",
        "failedProjectedMeasurements",
        "completedGpuProducerMeasurements",
        "failedGpuProducerMeasurements",
    ] {
        Reflect::set(
            &object,
            &JsValue::from_str(key),
            &Reflect::get(&receipts, &JsValue::from_str(key))?,
        )?;
    }
    set_u32(&object, "surfaceWidth", surface_size.0)?;
    set_u32(&object, "surfaceHeight", surface_size.1)?;
    set_u32(&object, "internalRenderWidth", internal_render_size.0)?;
    set_u32(&object, "internalRenderHeight", internal_render_size.1)?;
    set_optional_u64(
        &object,
        "presentedWidth",
        presented_size.map(|size| u64::from(size.0)),
    )?;
    set_optional_u64(
        &object,
        "presentedHeight",
        presented_size.map(|size| u64::from(size.1)),
    )?;
    Ok(object.into())
}

fn measurement_receipts_object(
    completed_cpu_order_measurements: &[SurfaceCpuOrderMeasurement],
    completed_order_measurements: &[SurfaceOrderMeasurement],
    failed_order_measurements: &[SurfaceOrderMeasurementFailure],
    completed_projected_measurements: &[SurfaceProjectedDrawMeasurement],
    failed_projected_measurements: &[SurfaceProjectedDrawMeasurementFailure],
    completed_gpu_producer_measurements: &[SurfaceGpuProducerMeasurement],
    failed_gpu_producer_measurements: &[SurfaceGpuProducerMeasurementFailure],
) -> Result<JsValue, JsValue> {
    let object = Object::new();
    let completed_cpu = Array::new();
    for measurement in completed_cpu_order_measurements {
        completed_cpu.push(&cpu_order_measurement_object(*measurement)?.into());
    }
    Reflect::set(
        &object,
        &JsValue::from_str("completedCpuOrderMeasurements"),
        &completed_cpu,
    )?;
    let completed = Array::new();
    for measurement in completed_order_measurements {
        completed.push(&order_measurement_object(*measurement)?.into());
    }
    Reflect::set(
        &object,
        &JsValue::from_str("completedOrderMeasurements"),
        &completed,
    )?;
    let failed = Array::new();
    for failure in failed_order_measurements {
        failed.push(&order_measurement_failure_object(*failure)?.into());
    }
    Reflect::set(
        &object,
        &JsValue::from_str("failedOrderMeasurements"),
        &failed,
    )?;
    let completed_projected = Array::new();
    for measurement in completed_projected_measurements {
        completed_projected.push(&projected_measurement_object(*measurement)?.into());
    }
    Reflect::set(
        &object,
        &JsValue::from_str("completedProjectedMeasurements"),
        &completed_projected,
    )?;
    let failed_projected = Array::new();
    for failure in failed_projected_measurements {
        failed_projected.push(&projected_failure_object(*failure)?.into());
    }
    Reflect::set(
        &object,
        &JsValue::from_str("failedProjectedMeasurements"),
        &failed_projected,
    )?;
    let completed_gpu_producer = Array::new();
    for measurement in completed_gpu_producer_measurements {
        completed_gpu_producer.push(&gpu_producer_measurement_object(*measurement)?.into());
    }
    Reflect::set(
        &object,
        &JsValue::from_str("completedGpuProducerMeasurements"),
        &completed_gpu_producer,
    )?;
    let failed_gpu_producer = Array::new();
    for failure in failed_gpu_producer_measurements {
        failed_gpu_producer.push(&gpu_producer_failure_object(*failure)?.into());
    }
    Reflect::set(
        &object,
        &JsValue::from_str("failedGpuProducerMeasurements"),
        &failed_gpu_producer,
    )?;
    Ok(object.into())
}

fn order_measurement_failure_object(
    failure: SurfaceOrderMeasurementFailure,
) -> Result<Object, JsValue> {
    let object = Object::new();
    set_u64(&object, "ticket", failure.ticket)?;
    set_u64(&object, "cameraRevision", failure.camera_revision)?;
    set_string(
        &object,
        "actualBackend",
        if failure.ticket & 1 == 0 {
            "cpu"
        } else {
            "gpu"
        },
    )?;
    set_string(
        &object,
        "reason",
        order_measurement_failure_reason(failure.reason),
    )?;
    Ok(object)
}

fn cpu_order_measurement_object(
    measurement: SurfaceCpuOrderMeasurement,
) -> Result<Object, JsValue> {
    let object = Object::new();
    set_u64(&object, "ticket", measurement.ticket)?;
    set_u64(&object, "cameraRevision", measurement.camera_revision)?;
    set_string(&object, "actualBackend", "cpu")?;
    set_f32(&object, "preprocessMs", measurement.preprocess_ms)?;
    set_f32(&object, "sortMs", measurement.sort_ms)?;
    set_f32(&object, "frameCompleteMs", measurement.frame_complete_ms)?;
    set_string(
        &object,
        "countSemantics",
        "candidate_visible_contributor_issued_v1",
    )?;
    set_u32(&object, "visibleCount", measurement.visible_count)?;
    set_u32(&object, "contributorCount", measurement.contributor_count)?;
    set_u32(&object, "drawnCount", measurement.drawn_count)?;
    set_bool(
        &object,
        "exactContributorCompaction",
        measurement.exact_contributor_compaction,
    )?;
    Ok(object)
}

const fn order_backend_label(backend: SurfaceOrderBackendUsed) -> &'static str {
    match backend {
        SurfaceOrderBackendUsed::Cpu => "cpu",
        SurfaceOrderBackendUsed::Gpu => "gpu",
    }
}

const fn order_measurement_failure_reason(
    reason: SurfaceOrderMeasurementFailureReason,
) -> &'static str {
    match reason {
        SurfaceOrderMeasurementFailureReason::ReadbackMap => "readback_map",
        SurfaceOrderMeasurementFailureReason::GenerationInvalidated => "generation_invalidated",
    }
}

fn order_measurement_object(measurement: SurfaceOrderMeasurement) -> Result<Object, JsValue> {
    let object = Object::new();
    set_u64(&object, "ticket", measurement.ticket)?;
    set_u64(&object, "cameraRevision", measurement.camera_revision)?;
    set_string(&object, "actualBackend", "gpu")?;
    set_string(
        &object,
        "timingSource",
        match measurement.timing_source {
            SurfaceTimingSource::TimestampQuery => "timestamp_query",
            SurfaceTimingSource::CompletionOnly => "completion_only",
        },
    )?;
    set_optional_f32(&object, "gpuPreprocessMs", measurement.gpu_preprocess_ms)?;
    set_optional_f32(&object, "gpuRadixMs", measurement.gpu_radix_ms)?;
    set_optional_f32(&object, "gpuOrderMs", measurement.gpu_order_ms)?;
    set_f32(&object, "gpuCompleteMs", measurement.gpu_complete_ms)?;
    set_optional_f32(
        &object,
        "timestampPeriodNs",
        measurement.timestamp_period_ns,
    )?;
    set_bool(
        &object,
        "belowTimestampResolution",
        measurement.below_timestamp_resolution,
    )?;
    set_string(
        &object,
        "countSemantics",
        "candidate_visible_contributor_issued_v1",
    )?;
    set_u32(&object, "visibleCount", measurement.visible_count)?;
    set_u32(&object, "contributorCount", measurement.contributor_count)?;
    set_u32(&object, "drawnCount", measurement.drawn_count)?;
    set_bool(
        &object,
        "exactContributorCompaction",
        measurement.exact_contributor_compaction,
    )?;
    Ok(object)
}

fn projected_measurement_object(
    measurement: SurfaceProjectedDrawMeasurement,
) -> Result<Object, JsValue> {
    let object = Object::new();
    set_u64(&object, "ticket", measurement.ticket)?;
    set_u64(&object, "cameraRevision", measurement.camera_revision)?;
    set_string(
        &object,
        "execution",
        projected_execution_label(measurement.execution),
    )?;
    set_string(
        &object,
        "orderBackend",
        order_backend_label(measurement.order_backend),
    )?;
    set_u64(
        &object,
        "projectionGeneration",
        measurement.projection_generation,
    )?;
    set_u64(&object, "probeGeneration", measurement.probe_generation)?;
    set_bool(&object, "projectionRebuilt", measurement.projection_rebuilt)?;
    set_bool(&object, "orderRefreshed", measurement.order_refreshed)?;
    set_f32(&object, "frameCompleteMs", measurement.frame_complete_ms)?;
    set_string(
        &object,
        "countSemantics",
        "candidate_visible_contributor_issued_v1",
    )?;
    set_u32(&object, "visibleCount", measurement.visible_count)?;
    set_u32(&object, "contributorCount", measurement.contributor_count)?;
    set_u32(&object, "drawnCount", measurement.drawn_count)?;
    set_bool(
        &object,
        "exactContributorCompaction",
        measurement.exact_contributor_compaction,
    )?;
    Ok(object)
}

fn projected_failure_object(
    failure: SurfaceProjectedDrawMeasurementFailure,
) -> Result<Object, JsValue> {
    let object = Object::new();
    set_u64(&object, "ticket", failure.ticket)?;
    set_u64(&object, "cameraRevision", failure.camera_revision)?;
    set_string(
        &object,
        "execution",
        projected_execution_label(failure.execution),
    )?;
    set_string(
        &object,
        "orderBackend",
        order_backend_label(failure.order_backend),
    )?;
    set_u64(
        &object,
        "projectionGeneration",
        failure.projection_generation,
    )?;
    set_u64(&object, "probeGeneration", failure.probe_generation)?;
    set_string(
        &object,
        "reason",
        projected_failure_reason_label(failure.reason),
    )?;
    Ok(object)
}

fn gpu_producer_measurement_object(
    measurement: SurfaceGpuProducerMeasurement,
) -> Result<Object, JsValue> {
    let object = Object::new();
    set_u64(&object, "ticket", measurement.ticket)?;
    set_u64(&object, "cameraRevision", measurement.camera_revision)?;
    set_string(
        &object,
        "producer",
        gpu_order_producer_label(measurement.producer),
    )?;
    set_u64(&object, "orderGeneration", measurement.order_generation)?;
    set_u64(
        &object,
        "projectionGeneration",
        measurement.projection_generation,
    )?;
    set_string(&object, "countSemantics", "source_contributor_issued_v1")?;
    set_u32(&object, "sourceCount", measurement.source_count)?;
    set_u32(&object, "contributorCount", measurement.contributor_count)?;
    set_u32(&object, "drawnCount", measurement.drawn_count)?;
    set_bool(&object, "orderRefreshed", measurement.order_refreshed)?;
    set_string(
        &object,
        "drawScope",
        gpu_producer_draw_scope_label(measurement.draw_scope),
    )?;
    set_bool(
        &object,
        "exactCurrentContributorDraw",
        measurement.exact_current_contributor_draw(),
    )?;
    set_bool(&object, "staleOrder", measurement.stale_order())?;
    set_f32(&object, "queueCompleteMs", measurement.frame_complete_ms)?;
    Ok(object)
}

fn gpu_producer_failure_object(
    failure: SurfaceGpuProducerMeasurementFailure,
) -> Result<Object, JsValue> {
    let object = Object::new();
    set_u64(&object, "ticket", failure.ticket)?;
    set_u64(&object, "cameraRevision", failure.camera_revision)?;
    set_string(
        &object,
        "producer",
        gpu_order_producer_label(failure.producer),
    )?;
    set_u64(&object, "orderGeneration", failure.order_generation)?;
    set_u64(
        &object,
        "projectionGeneration",
        failure.projection_generation,
    )?;
    set_string(
        &object,
        "reason",
        gpu_producer_failure_reason_label(failure.reason),
    )?;
    Ok(object)
}

const fn gpu_order_producer_label(producer: SurfaceGpuOrderProducer) -> &'static str {
    match producer {
        SurfaceGpuOrderProducer::PostSort => "post-sort",
        SurfaceGpuOrderProducer::Preproject => "preproject",
    }
}

const fn gpu_producer_draw_scope_label(scope: SurfaceGpuProducerDrawScope) -> &'static str {
    match scope {
        SurfaceGpuProducerDrawScope::ExactCurrentContributors => "exact_current_contributors",
        SurfaceGpuProducerDrawScope::StaleOrderCandidates => "stale_order_candidates",
    }
}

const fn gpu_producer_unsampled_reason_label(
    reason: SurfaceGpuProducerMeasurementUnsampledReason,
) -> &'static str {
    match reason {
        SurfaceGpuProducerMeasurementUnsampledReason::RingBusy => "ring_busy",
        SurfaceGpuProducerMeasurementUnsampledReason::SurfaceUnavailable => "surface_unavailable",
    }
}

const fn gpu_producer_failure_reason_label(
    reason: SurfaceGpuProducerMeasurementFailureReason,
) -> &'static str {
    match reason {
        SurfaceGpuProducerMeasurementFailureReason::ReadbackMap => "readback_map",
        SurfaceGpuProducerMeasurementFailureReason::GenerationInvalidated => {
            "generation_invalidated"
        }
        SurfaceGpuProducerMeasurementFailureReason::InvariantViolation => "invariant_violation",
    }
}

const fn projected_policy_label(policy: SurfaceProjectedDrawPolicy) -> &'static str {
    match policy {
        SurfaceProjectedDrawPolicy::Candidate => "candidate",
        SurfaceProjectedDrawPolicy::Compact => "compact",
        SurfaceProjectedDrawPolicy::Adaptive => "adaptive",
    }
}

const fn projected_execution_label(execution: SurfaceProjectedDrawExecution) -> &'static str {
    match execution {
        SurfaceProjectedDrawExecution::Candidate => "candidate",
        SurfaceProjectedDrawExecution::Compact => "compact",
    }
}

const fn projected_adaptive_state_label(state: SurfaceProjectedDrawAdaptiveState) -> &'static str {
    match state {
        SurfaceProjectedDrawAdaptiveState::Disabled => "disabled",
        SurfaceProjectedDrawAdaptiveState::CandidateLearning => "candidate_learning",
        SurfaceProjectedDrawAdaptiveState::CandidateStable => "candidate_stable",
        SurfaceProjectedDrawAdaptiveState::CompactProbe => "compact_probe",
        SurfaceProjectedDrawAdaptiveState::CompactStable => "compact_stable",
        SurfaceProjectedDrawAdaptiveState::CandidateProbe => "candidate_probe",
        SurfaceProjectedDrawAdaptiveState::CandidateOnly => "candidate_only",
        SurfaceProjectedDrawAdaptiveState::Cooldown => "cooldown",
    }
}

const fn projected_unsampled_reason_label(
    reason: SurfaceProjectedDrawMeasurementUnsampledReason,
) -> &'static str {
    match reason {
        SurfaceProjectedDrawMeasurementUnsampledReason::RingBusy => "ring_busy",
        SurfaceProjectedDrawMeasurementUnsampledReason::SurfaceUnavailable => "surface_unavailable",
    }
}

const fn projected_failure_reason_label(
    reason: SurfaceProjectedDrawMeasurementFailureReason,
) -> &'static str {
    match reason {
        SurfaceProjectedDrawMeasurementFailureReason::ReadbackMap => "readback_map",
        SurfaceProjectedDrawMeasurementFailureReason::GenerationInvalidated => {
            "generation_invalidated"
        }
        SurfaceProjectedDrawMeasurementFailureReason::InvariantViolation => "invariant_violation",
    }
}

fn set_f32(object: &Object, key: &str, value: f32) -> Result<(), JsValue> {
    Reflect::set(
        object,
        &JsValue::from_str(key),
        &JsValue::from_f64(value as f64),
    )
    .map(|_| ())
}

fn set_f64(object: &Object, key: &str, value: f64) -> Result<(), JsValue> {
    Reflect::set(object, &JsValue::from_str(key), &JsValue::from_f64(value)).map(|_| ())
}

fn set_optional_f32(object: &Object, key: &str, value: Option<f32>) -> Result<(), JsValue> {
    match value {
        Some(value) => set_f32(object, key, value),
        None => set_null(object, key),
    }
}

fn set_optional_u64(object: &Object, key: &str, value: Option<u64>) -> Result<(), JsValue> {
    match value {
        Some(value) => set_u64(object, key, value),
        None => set_null(object, key),
    }
}

fn set_u64(object: &Object, key: &str, value: u64) -> Result<(), JsValue> {
    const MAX_JAVASCRIPT_SAFE_INTEGER: u64 = (1_u64 << 53) - 1;
    if value > MAX_JAVASCRIPT_SAFE_INTEGER {
        return Err(js_error(format!(
            "{key} exceeds JavaScript's exact integer range"
        )));
    }
    set_f64(object, key, value as f64)
}

fn set_optional_string(object: &Object, key: &str, value: Option<&str>) -> Result<(), JsValue> {
    match value {
        Some(value) => set_string(object, key, value),
        None => set_null(object, key),
    }
}

fn set_null(object: &Object, key: &str) -> Result<(), JsValue> {
    Reflect::set(object, &JsValue::from_str(key), &JsValue::NULL).map(|_| ())
}

fn set_u32(object: &Object, key: &str, value: u32) -> Result<(), JsValue> {
    Reflect::set(
        object,
        &JsValue::from_str(key),
        &JsValue::from_f64(value as f64),
    )
    .map(|_| ())
}

fn set_optional_u32(object: &Object, key: &str, value: Option<u32>) -> Result<(), JsValue> {
    match value {
        Some(value) => set_u32(object, key, value),
        None => set_null(object, key),
    }
}

fn set_bool(object: &Object, key: &str, value: bool) -> Result<(), JsValue> {
    Reflect::set(object, &JsValue::from_str(key), &JsValue::from_bool(value)).map(|_| ())
}

fn set_string(object: &Object, key: &str, value: &str) -> Result<(), JsValue> {
    Reflect::set(object, &JsValue::from_str(key), &JsValue::from_str(value)).map(|_| ())
}

fn renderer_error(err: gsplat_render_wgpu::RendererError) -> JsValue {
    js_error(err.to_string())
}

fn error_code(code: ErrorCode) -> JsValue {
    js_error(format!("{code:?}"))
}

fn js_error(message: impl Into<String>) -> JsValue {
    js_sys::Error::new(&message.into()).into()
}
