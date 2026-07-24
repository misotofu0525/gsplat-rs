use thiserror::Error;

use crate::raster::{
    QUAD_VERTEX_COUNT, SplatDraw, SplatIndirectDraw, SplatPipeline, create_splat_pipeline,
    encode_splat_draw_into, encode_splat_indirect_draw_into,
};
use crate::wgpu_label;

const PROJECTED_RECORD_BYTES: u64 = 16;
const COLOR_RECORD_BYTES: u64 = 8;
const SOURCE_ID_BYTES: u64 = 4;
const DRAW_INDIRECT_ARGS_BYTES: u64 = 16;

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CanonicalRasterError {
    #[error("canonical raster requires at least one prepared input family")]
    Empty,
    #[error("canonical raster {resource} count mismatch: expected {expected}, got {actual}")]
    CountMismatch {
        resource: &'static str,
        expected: u32,
        actual: u32,
    },
    #[error(
        "canonical raster {resource} buffer is too small: requires {required} bytes, got {actual}"
    )]
    BufferTooSmall {
        resource: &'static str,
        required: u64,
        actual: u64,
    },
    #[error("canonical raster {resource} buffer is missing required {usage:?} usage")]
    BufferUsage {
        resource: &'static str,
        usage: wgpu::BufferUsages,
    },
    #[error("canonical raster {input} input is not prepared")]
    InputUnavailable { input: &'static str },
    #[error(
        "canonical raster direct draw count {count} exceeds projected rank capacity {capacity}"
    )]
    DirectCountExceedsCapacity { count: u32, capacity: u32 },
    #[error("canonical raster target format mismatch: prepared {expected:?}, got {actual:?}")]
    TargetFormatMismatch {
        expected: wgpu::TextureFormat,
        actual: wgpu::TextureFormat,
    },
    #[error("canonical raster byte-size calculation overflowed for {resource}")]
    SizeOverflow { resource: &'static str },
}

#[derive(Clone, Copy)]
pub(crate) struct RankIndexedRasterResources<'a> {
    pub(crate) projected_center_source: &'a wgpu::Buffer,
    pub(crate) projected_axes: &'a wgpu::Buffer,
    pub(crate) resolved_color: &'a wgpu::Buffer,
    pub(crate) indirect_args: Option<&'a wgpu::Buffer>,
    pub(crate) projected_capacity: u32,
    pub(crate) source_count: u32,
}

#[derive(Clone, Copy)]
pub(crate) struct SourceIndexedRasterResources<'a> {
    pub(crate) ordered_source_ids: &'a wgpu::Buffer,
    pub(crate) projected_center_alpha_key: &'a wgpu::Buffer,
    pub(crate) projected_axes: &'a wgpu::Buffer,
    pub(crate) resolved_color: &'a wgpu::Buffer,
    pub(crate) indirect_args: &'a wgpu::Buffer,
    pub(crate) source_count: u32,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct CanonicalRasterResources<'a> {
    pub(crate) rank_indexed: Option<RankIndexedRasterResources<'a>>,
    pub(crate) source_indexed: Option<SourceIndexedRasterResources<'a>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum CanonicalRasterInput {
    RankIndexedDirect { instance_count: u32 },
    RankIndexedIndirect,
    SourceIndexedIndirect,
}

struct RankIndexedRaster {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    indirect_args: Option<wgpu::Buffer>,
    projected_capacity: u32,
}

struct SourceIndexedRaster {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    indirect_args: wgpu::Buffer,
}

/// Dormant Exact-core owner of the accepted four-vertex SortedAlpha raster.
/// Projection and ordering resources are borrowed only during preparation;
/// prepared bind groups retain their buffer handles. Frame encoding receives
/// only a target, clear color and neutral direct/indirect draw identity. The
/// caller remains the sole command encoder, submission and target-lifecycle
/// owner.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct CanonicalRaster {
    target_format: wgpu::TextureFormat,
    rank_indexed: Option<RankIndexedRaster>,
    source_indexed: Option<SourceIndexedRaster>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl CanonicalRaster {
    pub(crate) fn prepare(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        resources: CanonicalRasterResources<'_>,
    ) -> Result<Self, CanonicalRasterError> {
        if resources.rank_indexed.is_none() && resources.source_indexed.is_none() {
            return Err(CanonicalRasterError::Empty);
        }

        if let Some(rank) = resources.rank_indexed {
            validate_rank_resources(rank)?;
        }
        if let Some(source) = resources.source_indexed {
            validate_source_resources(source)?;
        }

        // Build every requested family into locals before returning the owner.
        // The caller can therefore publish only this complete prepared unit;
        // no per-frame bind-group creation or partial CanonicalRaster exists.
        let rank_indexed = resources
            .rank_indexed
            .map(|rank| prepare_rank_indexed(device, target_format, rank));
        let source_indexed = resources
            .source_indexed
            .map(|source| prepare_source_indexed(device, target_format, source));

        Ok(Self {
            target_format,
            rank_indexed,
            source_indexed,
        })
    }

    pub(crate) fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        target_format: wgpu::TextureFormat,
        clear: wgpu::Color,
        input: CanonicalRasterInput,
    ) -> Result<(), CanonicalRasterError> {
        if target_format != self.target_format {
            return Err(CanonicalRasterError::TargetFormatMismatch {
                expected: self.target_format,
                actual: target_format,
            });
        }
        match input {
            CanonicalRasterInput::RankIndexedDirect { instance_count } => {
                let rank =
                    self.rank_indexed
                        .as_ref()
                        .ok_or(CanonicalRasterError::InputUnavailable {
                            input: "rank-indexed direct",
                        })?;
                if instance_count > rank.projected_capacity {
                    return Err(CanonicalRasterError::DirectCountExceedsCapacity {
                        count: instance_count,
                        capacity: rank.projected_capacity,
                    });
                }
                encode_splat_draw_into(
                    encoder,
                    &SplatDraw {
                        pass_label: "gsplat-canonical-rank-direct-pass",
                        view: target,
                        pipeline: &rank.pipeline,
                        bind_group: &rank.bind_group,
                        clear,
                        vertex_count: QUAD_VERTEX_COUNT,
                        instance_count,
                    },
                );
            }
            CanonicalRasterInput::RankIndexedIndirect => {
                let rank =
                    self.rank_indexed
                        .as_ref()
                        .ok_or(CanonicalRasterError::InputUnavailable {
                            input: "rank-indexed indirect",
                        })?;
                let indirect_args =
                    rank.indirect_args
                        .as_ref()
                        .ok_or(CanonicalRasterError::InputUnavailable {
                            input: "rank-indexed indirect arguments",
                        })?;
                encode_splat_indirect_draw_into(
                    encoder,
                    &SplatIndirectDraw {
                        pass_label: "gsplat-canonical-rank-indirect-pass",
                        view: target,
                        pipeline: &rank.pipeline,
                        bind_group: &rank.bind_group,
                        clear,
                        indirect_args,
                    },
                );
            }
            CanonicalRasterInput::SourceIndexedIndirect => {
                let source =
                    self.source_indexed
                        .as_ref()
                        .ok_or(CanonicalRasterError::InputUnavailable {
                            input: "source-indexed indirect",
                        })?;
                encode_splat_indirect_draw_into(
                    encoder,
                    &SplatIndirectDraw {
                        pass_label: "gsplat-canonical-source-indirect-pass",
                        view: target,
                        pipeline: &source.pipeline,
                        bind_group: &source.bind_group,
                        clear,
                        indirect_args: &source.indirect_args,
                    },
                );
            }
        }
        Ok(())
    }
}

fn validate_rank_resources(
    resources: RankIndexedRasterResources<'_>,
) -> Result<(), CanonicalRasterError> {
    if resources.projected_capacity != resources.source_count {
        return Err(CanonicalRasterError::CountMismatch {
            resource: "rank-indexed projected capacity",
            expected: resources.source_count,
            actual: resources.projected_capacity,
        });
    }
    validate_storage_buffer(
        "rank-indexed projected centers",
        resources.projected_center_source,
        binding_bytes(
            "rank-indexed projected centers",
            resources.projected_capacity,
            PROJECTED_RECORD_BYTES,
        )?,
    )?;
    validate_storage_buffer(
        "rank-indexed projected axes",
        resources.projected_axes,
        binding_bytes(
            "rank-indexed projected axes",
            resources.projected_capacity,
            PROJECTED_RECORD_BYTES,
        )?,
    )?;
    validate_storage_buffer(
        "rank-indexed resolved color",
        resources.resolved_color,
        binding_bytes(
            "rank-indexed resolved color",
            resources.source_count,
            COLOR_RECORD_BYTES,
        )?,
    )?;
    if let Some(indirect_args) = resources.indirect_args {
        validate_indirect_buffer("rank-indexed indirect arguments", indirect_args)?;
    }
    Ok(())
}

fn validate_source_resources(
    resources: SourceIndexedRasterResources<'_>,
) -> Result<(), CanonicalRasterError> {
    validate_storage_buffer(
        "source-indexed ordered IDs",
        resources.ordered_source_ids,
        binding_bytes(
            "source-indexed ordered IDs",
            resources.source_count,
            SOURCE_ID_BYTES,
        )?,
    )?;
    validate_storage_buffer(
        "source-indexed projected centers",
        resources.projected_center_alpha_key,
        binding_bytes(
            "source-indexed projected centers",
            resources.source_count,
            PROJECTED_RECORD_BYTES,
        )?,
    )?;
    validate_storage_buffer(
        "source-indexed projected axes",
        resources.projected_axes,
        binding_bytes(
            "source-indexed projected axes",
            resources.source_count,
            PROJECTED_RECORD_BYTES,
        )?,
    )?;
    validate_storage_buffer(
        "source-indexed resolved color",
        resources.resolved_color,
        binding_bytes(
            "source-indexed resolved color",
            resources.source_count,
            COLOR_RECORD_BYTES,
        )?,
    )?;
    validate_indirect_buffer("source-indexed indirect arguments", resources.indirect_args)
}

fn validate_storage_buffer(
    resource: &'static str,
    buffer: &wgpu::Buffer,
    required: u64,
) -> Result<(), CanonicalRasterError> {
    validate_buffer_size(resource, buffer, required)?;
    validate_buffer_usage(resource, buffer, wgpu::BufferUsages::STORAGE)
}

fn validate_indirect_buffer(
    resource: &'static str,
    buffer: &wgpu::Buffer,
) -> Result<(), CanonicalRasterError> {
    validate_buffer_size(resource, buffer, DRAW_INDIRECT_ARGS_BYTES)?;
    validate_buffer_usage(resource, buffer, wgpu::BufferUsages::INDIRECT)
}

fn validate_buffer_size(
    resource: &'static str,
    buffer: &wgpu::Buffer,
    required: u64,
) -> Result<(), CanonicalRasterError> {
    if buffer.size() < required {
        return Err(CanonicalRasterError::BufferTooSmall {
            resource,
            required,
            actual: buffer.size(),
        });
    }
    Ok(())
}

fn validate_buffer_usage(
    resource: &'static str,
    buffer: &wgpu::Buffer,
    usage: wgpu::BufferUsages,
) -> Result<(), CanonicalRasterError> {
    if !buffer.usage().contains(usage) {
        return Err(CanonicalRasterError::BufferUsage { resource, usage });
    }
    Ok(())
}

fn binding_bytes(
    resource: &'static str,
    count: u32,
    stride: u64,
) -> Result<u64, CanonicalRasterError> {
    u64::from(count)
        .checked_mul(stride)
        .map(|bytes| bytes.max(stride))
        .ok_or(CanonicalRasterError::SizeOverflow { resource })
}

fn prepare_rank_indexed(
    device: &wgpu::Device,
    target_format: wgpu::TextureFormat,
    resources: RankIndexedRasterResources<'_>,
) -> RankIndexedRaster {
    let layout = create_storage_layout(device, "gsplat-canonical-rank-bgl", 3);
    let pipeline = create_splat_pipeline(
        device,
        &layout,
        target_format,
        SplatPipeline {
            shader_label: "gsplat-canonical-rank-shader",
            shader_source: include_str!("../../shaders/projected_quads_draw.wgsl"),
            layout_label: "gsplat-canonical-rank-pipeline-layout",
            pipeline_label: "gsplat-canonical-rank-pipeline",
            topology: wgpu::PrimitiveTopology::TriangleStrip,
        },
    );
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: wgpu_label("gsplat-canonical-rank-bg"),
        layout: &layout,
        entries: &[
            storage_entry(0, resources.projected_center_source),
            storage_entry(1, resources.projected_axes),
            storage_entry(2, resources.resolved_color),
        ],
    });
    RankIndexedRaster {
        pipeline,
        bind_group,
        indirect_args: resources.indirect_args.cloned(),
        projected_capacity: resources.projected_capacity,
    }
}

fn prepare_source_indexed(
    device: &wgpu::Device,
    target_format: wgpu::TextureFormat,
    resources: SourceIndexedRasterResources<'_>,
) -> SourceIndexedRaster {
    let layout = create_storage_layout(device, "gsplat-canonical-source-bgl", 4);
    let pipeline = create_splat_pipeline(
        device,
        &layout,
        target_format,
        SplatPipeline {
            shader_label: "gsplat-canonical-source-shader",
            shader_source: include_str!("../../shaders/preproject_draw.wgsl"),
            layout_label: "gsplat-canonical-source-pipeline-layout",
            pipeline_label: "gsplat-canonical-source-pipeline",
            topology: wgpu::PrimitiveTopology::TriangleStrip,
        },
    );
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: wgpu_label("gsplat-canonical-source-bg"),
        layout: &layout,
        entries: &[
            storage_entry(0, resources.ordered_source_ids),
            storage_entry(1, resources.projected_center_alpha_key),
            storage_entry(2, resources.projected_axes),
            storage_entry(3, resources.resolved_color),
        ],
    });
    SourceIndexedRaster {
        pipeline,
        bind_group,
        indirect_args: resources.indirect_args.clone(),
    }
}

fn create_storage_layout(
    device: &wgpu::Device,
    label: &'static str,
    binding_count: u32,
) -> wgpu::BindGroupLayout {
    let entries = (0..binding_count)
        .map(|binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        })
        .collect::<Vec<_>>();
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: wgpu_label(label),
        entries: &entries,
    })
}

fn storage_entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

#[cfg(test)]
mod tests;
