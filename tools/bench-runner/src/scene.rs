use std::path::Path;

use gsplat_io_ply::{DecodedPlySplat, load_ply, load_ply_summary, visit_ply_splats};
use gsplat_render_wgpu::{GeometryPath, Renderer, ResidentSceneBuilder, ResidentSourceSplat};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneLoadReceipt {
    pub source_count: usize,
    pub decoded_count: usize,
    pub encoded_count: usize,
    pub resident_count: usize,
    pub addressable_count: usize,
    pub source_sh_degree: u8,
    pub resident_sh_degree: u8,
}

pub fn load_scene_for_path(
    path: &Path,
    renderer: &mut Renderer,
) -> Result<SceneLoadReceipt, String> {
    match renderer.geometry_path() {
        GeometryPath::PackedAtlas => load_resident_scene(path, renderer),
        GeometryPath::SortedIndexDirect | GeometryPath::PagedActiveAtlas => {
            let loaded = load_ply(path).map_err(|error| error.to_string())?;
            let count = loaded.scene.len();
            let sh_degree = loaded.scene.sh_degree;
            renderer
                .load_scene(loaded.scene)
                .map_err(|error| error.to_string())?;
            Ok(SceneLoadReceipt {
                source_count: count,
                decoded_count: count,
                encoded_count: count,
                resident_count: renderer
                    .scene_len()
                    .ok_or_else(|| "renderer did not publish the decoded scene".to_owned())?,
                addressable_count: count,
                source_sh_degree: sh_degree,
                resident_sh_degree: renderer
                    .scene_sh_degree()
                    .ok_or_else(|| "renderer did not publish the decoded SH degree".to_owned())?,
            })
        }
    }
}

fn load_resident_scene(path: &Path, renderer: &mut Renderer) -> Result<SceneLoadReceipt, String> {
    let expected = load_ply_summary(path).map_err(|error| error.to_string())?;
    let mut builder = ResidentSceneBuilder::new(expected.gaussians, expected.sh_degree)
        .map_err(|error| error.to_string())?;
    let mut builder_error = None;
    let decoded = visit_ply_splats(path, |splat| {
        if builder_error.is_none() {
            builder_error = builder.push(resident_source_splat(splat)).err();
        }
    })
    .map_err(|error| error.to_string())?;
    if let Some(error) = builder_error {
        return Err(error.to_string());
    }
    if decoded != expected {
        return Err(format!(
            "PLY changed while loading: expected {expected:?}, decoded {decoded:?}"
        ));
    }

    let resident = builder.finish().map_err(|error| error.to_string())?;
    let report = resident.report;
    let resident_count = resident.len();
    let resident_sh_degree = resident.sh_degree;
    renderer
        .load_resident_scene(resident)
        .map_err(|error| error.to_string())?;

    Ok(SceneLoadReceipt {
        source_count: expected.gaussians,
        decoded_count: decoded.gaussians,
        encoded_count: report.encoded_count,
        resident_count,
        addressable_count: resident_count,
        source_sh_degree: expected.sh_degree,
        resident_sh_degree,
    })
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use gsplat_core::{RenderMode, RendererConfig};
    use gsplat_render_wgpu::{GeometryPath, Renderer};

    use super::load_scene_for_path;

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("tests/datasets/minimal_ascii.ply")
    }

    #[test]
    fn packed_loader_publishes_complete_resident_receipt() {
        let Ok(mut renderer) = Renderer::with_config(RendererConfig::default()) else {
            eprintln!("skipping packed loader test because no offscreen adapter is available");
            return;
        };
        renderer.set_geometry_path(GeometryPath::PackedAtlas);

        let receipt = load_scene_for_path(&fixture(), &mut renderer).expect("packed fixture");

        assert_eq!(receipt.source_count, receipt.decoded_count);
        assert_eq!(receipt.source_count, receipt.encoded_count);
        assert_eq!(receipt.source_count, receipt.resident_count);
        assert_eq!(receipt.source_count, receipt.addressable_count);
        assert_eq!(receipt.source_sh_degree, receipt.resident_sh_degree);
        assert!(renderer.scene().is_none());
        assert!(renderer.resident_scene().is_some());
    }

    #[test]
    fn direct_loader_remains_an_explicit_wide_oracle() {
        let Ok(mut renderer) = Renderer::new(RenderMode::SortedAlpha) else {
            eprintln!("skipping direct loader test because no offscreen adapter is available");
            return;
        };
        renderer.set_geometry_path(GeometryPath::SortedIndexDirect);

        let receipt = load_scene_for_path(&fixture(), &mut renderer).expect("direct fixture");

        assert_eq!(receipt.source_count, receipt.resident_count);
        assert!(renderer.scene().is_some());
        assert!(renderer.resident_scene().is_none());
    }
}
