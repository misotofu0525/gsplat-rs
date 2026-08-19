//! PlayCanvas SOG and Streamed SOG import.
//!
//! Unbundled SOG (`meta.json` plus 8-bit images) and bundled `.sog` ZIP decode
//! to one resident [`SceneBuffers`](gsplat_core::SceneBuffers). Streamed SOG
//! (`lod-meta.json`) is selected from spatial metadata first; only the
//! budgeted subset is decoded. GPU residency is an independent gaussian
//! cap, not a page pool.

mod archive;
mod decode;
mod error;
mod meta;
mod stream;

use std::path::Path;

pub use archive::{decode_sog_archive, decode_sog_archive_path, is_zip_magic};
pub use decode::{decode_sog_dir, decode_sog_range};
pub use error::SogError;
pub use meta::{
    LodMeta, LodNode, LodRange, SogChunkMeta, SogShnMeta, load_chunk_meta, load_lod_meta,
    parse_chunk_meta, parse_lod_meta,
};
pub use stream::{
    StreamAssembleResult, StreamedSogSession, StreamingBudgets, assemble_streamed_sog,
};

/// True when `path` is a Streamed SOG index (`lod-meta.json`).
pub fn is_streamed_sog_path(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some("lod-meta.json")
}

/// True when `path` is an unbundled whole-scene SOG (`meta.json`).
pub fn is_unbundled_sog_path(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some("meta.json")
}

/// True when `path` is a bundled whole-scene SOG ZIP (`.sog`).
pub fn is_bundled_sog_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("sog"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use gsplat_core::{Camera, CameraPose, Vec3f};
    use image::{Rgba, RgbaImage};
    use serde_json::json;

    use super::*;

    #[test]
    fn parse_lod_meta_reads_playcanvas_example_shape() {
        let raw = r#"{
          "version": 1,
          "count": 6,
          "counts": [4, 2],
          "lodLevels": 2,
          "environment": null,
          "filenames": ["0_0/meta.json", "1_0/meta.json"],
          "tree": {
            "bound": { "min": [-10, 0, -10], "max": [10, 5, 10] },
            "children": [
              {
                "bound": { "min": [-10, 0, -10], "max": [0.5, 5, 10] },
                "lods": {
                  "0": { "file": 0, "offset": 0, "count": 2 },
                  "1": { "file": 1, "offset": 0, "count": 1 }
                }
              },
              {
                "bound": { "min": [0.5, 0, -10], "max": [10, 4.5, 10] },
                "lods": {
                  "0": { "file": 0, "offset": 2, "count": 2 },
                  "1": { "file": 1, "offset": 1, "count": 1 }
                }
              }
            ]
          }
        }"#;
        let meta = parse_lod_meta(raw).expect("example lod-meta");
        assert_eq!(meta.version, 1);
        assert_eq!(meta.lod_levels, 2);
        assert!(meta.environment.is_none());
        let mut leaves = Vec::new();
        meta.tree.walk_leaves(&mut leaves);
        assert_eq!(leaves.len(), 2);
        assert_eq!(leaves[0].lods[&0].count, 2);
    }

    #[test]
    fn decode_roundtrips_identity_splat_in_ruf() {
        let dir = temp_dir("sog-roundtrip");
        write_unbundled_sog(&dir, &[test_splat(-0.25, 0.125, 1.5)]);
        let scene = decode_sog_dir(&dir.join("meta.json")).expect("decode SOG");
        assert_eq!(scene.len(), 1);
        assert!(scene.validate().is_ok());
        assert!((scene.positions[0].x + 0.25).abs() < 1.0e-3);
        assert!((scene.positions[0].y - 0.125).abs() < 1.0e-3);
        assert!((scene.positions[0].z - 1.5).abs() < 1.0e-3);
        assert!((scene.rotation_xyzw[0][0]).abs() < 5.0e-3);
        assert!((scene.rotation_xyzw[0][1]).abs() < 5.0e-3);
        assert!((scene.rotation_xyzw[0][2]).abs() < 5.0e-3);
        assert!((scene.rotation_xyzw[0][3] - 1.0).abs() < 5.0e-3);
        assert!((scene.scale_xyz[0][0] + 2.0).abs() < 1.0e-6);
        assert!((scene.color_dc[0][0] - 0.25).abs() < 1.0e-6);
        assert!(scene.opacity[0] > 8.0);
    }

    #[test]
    fn streamed_session_selects_near_leaf_under_gaussian_budget() {
        let dir = temp_dir("sog-stream");
        write_streamed_sog(&dir);
        let mut session = StreamedSogSession::open(
            &dir.join("lod-meta.json"),
            StreamingBudgets {
                max_gaussians: 1,
                max_source_bytes: 256 * 1024,
                max_decoded_bytes: 256 * 1024,
                ..StreamingBudgets::default()
            },
        )
        .expect("open streamed SOG");

        let without_camera = session.assemble(None);
        assert!(matches!(
            without_camera,
            Err(SogError::ResourceLimit {
                resource: "gaussians",
                requested: 2,
                limit: 1
            })
        ));

        let camera = Camera {
            pose: CameraPose {
                position: Vec3f::new(-1.0, 0.0, 0.0),
                rotation_xyzw: [0.0, 0.0, 0.0, 1.0],
            },
            ..Camera::default()
        };
        let assembled = session.assemble(Some(&camera)).expect("near leaf fits");
        assert_eq!(assembled.gaussians, 1);
        assert_eq!(assembled.selected_leaves, 1);
        assert_eq!(assembled.dropped_leaves, 1);
        assert!(assembled.scene.positions[0].x < 0.0);

        let far_camera = Camera {
            pose: CameraPose {
                position: Vec3f::new(1.0, 0.0, 0.0),
                rotation_xyzw: [0.0, 0.0, 0.0, 1.0],
            },
            ..Camera::default()
        };
        let far = session.assemble(Some(&far_camera)).expect("far leaf fits");
        assert_eq!(far.gaussians, 1);
        assert!(far.scene.positions[0].x > 0.0);
        assert_ne!(assembled.fingerprint, far.fingerprint);
    }

    #[test]
    fn streamed_session_applies_independent_resident_gaussian_budget() {
        let dir = temp_dir("sog-resident");
        write_streamed_sog(&dir);
        let mut session = StreamedSogSession::open(
            &dir.join("lod-meta.json"),
            StreamingBudgets {
                max_resident_gaussians: Some(1),
                ..StreamingBudgets::default()
            },
        )
        .expect("open streamed SOG");
        assert_eq!(session.peek_sh_degree().expect("peek SH"), 0);

        assert!(matches!(
            session.assemble(None),
            Err(SogError::ResourceLimit {
                resource: "resident gaussians",
                requested: 2,
                limit: 1
            })
        ));

        let camera = Camera {
            pose: CameraPose {
                position: Vec3f::new(-1.0, 0.0, 0.0),
                rotation_xyzw: [0.0, 0.0, 0.0, 1.0],
            },
            ..Camera::default()
        };
        let assembled = session
            .assemble(Some(&camera))
            .expect("near leaf fits resident cap");
        assert_eq!(assembled.gaussians, 1);
        assert_eq!(assembled.selected_leaves, 1);
        assert_eq!(assembled.dropped_leaves, 1);
    }

    #[test]
    fn bundled_archive_roundtrips_unbundled_dir() {
        let dir = temp_dir("sog-zip");
        write_unbundled_sog(&dir, &[test_splat(-0.25, 0.125, 1.5)]);
        let bytes = bundle_dir(&dir);
        let scene = decode_sog_archive(&bytes).expect("decode bundled SOG");
        assert_eq!(scene.len(), 1);
        assert!(scene.validate().is_ok());
        assert!((scene.positions[0].x + 0.25).abs() < 1.0e-3);
    }

    #[test]
    fn write_committed_sog_fixtures_when_requested() {
        if std::env::var_os("GSPLAT_WRITE_SOG_FIXTURE").is_none() {
            return;
        }
        let datasets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/datasets");
        write_unbundled_sog(
            &datasets.join("minimal_sog"),
            &[
                test_splat(-0.25, 0.125, 1.5),
                test_splat(0.25, -0.125, 1.25),
            ],
        );
        write_streamed_sog(&datasets.join("minimal_streamed_sog"));
        fs::write(
            datasets.join("minimal.sog"),
            bundle_dir(&datasets.join("minimal_sog")),
        )
        .unwrap();
    }

    #[test]
    fn loads_committed_minimal_sog_and_streamed_fixtures() {
        let datasets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/datasets");
        let sog = datasets.join("minimal_sog/meta.json");
        let streamed = datasets.join("minimal_streamed_sog/lod-meta.json");
        if !sog.is_file() || !streamed.is_file() {
            return;
        }
        let scene = decode_sog_dir(&sog).expect("committed unbundled SOG");
        assert_eq!(scene.len(), 2);
        assert!(scene.validate().is_ok());

        let assembled =
            assemble_streamed_sog(&streamed, None, StreamingBudgets::default()).expect("streamed");
        assert_eq!(assembled.gaussians, 2);
        assert_eq!(assembled.selected_leaves, 2);
        assert_eq!(assembled.dropped_leaves, 0);

        let bundled = datasets.join("minimal.sog");
        if bundled.is_file() {
            let scene = decode_sog_archive_path(&bundled).expect("committed bundled SOG");
            assert_eq!(scene.len(), 2);
            assert!(scene.validate().is_ok());
        }
    }

    struct TestSplat {
        position_ruf: [f32; 3],
        alpha: f32,
    }

    fn test_splat(x: f32, y: f32, z: f32) -> TestSplat {
        TestSplat {
            position_ruf: [x, y, z],
            alpha: 1.0,
        }
    }

    fn write_streamed_sog(root: &Path) {
        let left = test_splat(-1.0, 0.0, 1.0);
        let right = test_splat(1.0, 0.0, 1.0);
        write_unbundled_sog(&root.join("0_0"), &[left]);
        write_unbundled_sog(&root.join("0_1"), &[right]);
        fs::create_dir_all(root).unwrap();
        let lod = json!({
            "version": 1,
            "count": 2,
            "counts": [2],
            "lodLevels": 1,
            "filenames": ["0_0/meta.json", "0_1/meta.json"],
            "tree": {
                "bound": { "min": [-2.0, -1.0, -2.0], "max": [2.0, 1.0, 2.0] },
                "children": [
                    {
                        "bound": { "min": [-2.0, -1.0, -2.0], "max": [0.0, 1.0, 2.0] },
                        "lods": { "0": { "file": 0, "offset": 0, "count": 1 } }
                    },
                    {
                        "bound": { "min": [0.0, -1.0, -2.0], "max": [2.0, 1.0, 2.0] },
                        "lods": { "0": { "file": 1, "offset": 0, "count": 1 } }
                    }
                ]
            }
        });
        fs::write(
            root.join("lod-meta.json"),
            serde_json::to_string_pretty(&lod).unwrap(),
        )
        .unwrap();
    }

    fn write_unbundled_sog(dir: &Path, splats: &[TestSplat]) {
        fs::create_dir_all(dir).unwrap();
        let count = splats.len();
        let width = count.max(1) as u32;
        let height = 1_u32;
        let mut means_l = RgbaImage::new(width, height);
        let mut means_u = RgbaImage::new(width, height);
        let mut scales = RgbaImage::new(width, height);
        let mut quats = RgbaImage::new(width, height);
        let mut sh0 = RgbaImage::new(width, height);

        let mut log_mins = [f32::INFINITY; 3];
        let mut log_maxs = [f32::NEG_INFINITY; 3];
        let logs: Vec<[f32; 3]> = splats
            .iter()
            .map(|splat| {
                let rub = [
                    splat.position_ruf[0],
                    splat.position_ruf[1],
                    -splat.position_ruf[2],
                ];
                let logged = [
                    log_transform(rub[0]),
                    log_transform(rub[1]),
                    log_transform(rub[2]),
                ];
                for axis in 0..3 {
                    log_mins[axis] = log_mins[axis].min(logged[axis]);
                    log_maxs[axis] = log_maxs[axis].max(logged[axis]);
                }
                logged
            })
            .collect();
        for axis in 0..3 {
            if (log_maxs[axis] - log_mins[axis]).abs() < 1.0e-6 {
                log_maxs[axis] = log_mins[axis] + 1.0;
            }
        }

        for (index, splat) in splats.iter().enumerate() {
            let x = index as u32;
            let logged = logs[index];
            let mut low = [255_u8; 4];
            let mut high = [255_u8; 4];
            for axis in 0..3 {
                let t = (logged[axis] - log_mins[axis]) / (log_maxs[axis] - log_mins[axis]);
                let quantized = (t.clamp(0.0, 1.0) * 65535.0).round() as u16;
                low[axis] = (quantized & 0xff) as u8;
                high[axis] = (quantized >> 8) as u8;
            }
            means_l.put_pixel(x, 0, Rgba(low));
            means_u.put_pixel(x, 0, Rgba(high));
            scales.put_pixel(x, 0, Rgba([1, 2, 3, 255]));
            quats.put_pixel(x, 0, Rgba([128, 128, 128, 252]));
            sh0.put_pixel(
                x,
                0,
                Rgba([4, 5, 6, (splat.alpha.clamp(0.0, 1.0) * 255.0).round() as u8]),
            );
        }

        means_l.save(dir.join("means_l.png")).unwrap();
        means_u.save(dir.join("means_u.png")).unwrap();
        scales.save(dir.join("scales.png")).unwrap();
        quats.save(dir.join("quats.png")).unwrap();
        sh0.save(dir.join("sh0.png")).unwrap();

        let mut scales_codebook = vec![0.0f32; 256];
        scales_codebook[1] = -2.0;
        scales_codebook[2] = -1.0;
        scales_codebook[3] = 0.0;
        let mut sh0_codebook = vec![0.0f32; 256];
        sh0_codebook[4] = 0.25;
        sh0_codebook[5] = 0.0;
        sh0_codebook[6] = -0.125;

        let meta = json!({
            "version": 2,
            "count": count,
            "means": {
                "mins": log_mins,
                "maxs": log_maxs,
                "files": ["means_l.png", "means_u.png"]
            },
            "scales": {
                "codebook": scales_codebook,
                "files": ["scales.png"]
            },
            "quats": { "files": ["quats.png"] },
            "sh0": {
                "codebook": sh0_codebook,
                "files": ["sh0.png"]
            }
        });
        fs::write(
            dir.join("meta.json"),
            serde_json::to_string_pretty(&meta).unwrap(),
        )
        .unwrap();
    }

    fn bundle_dir(dir: &Path) -> Vec<u8> {
        let names = [
            "meta.json",
            "means_l.png",
            "means_u.png",
            "scales.png",
            "quats.png",
            "sh0.png",
        ];
        let entries: Vec<(String, Vec<u8>)> = names
            .iter()
            .map(|name| (name.to_string(), fs::read(dir.join(name)).unwrap()))
            .collect();
        crate::archive::write_stored_zip(&entries)
    }

    fn log_transform(value: f32) -> f32 {
        value.signum() * (value.abs() + 1.0).ln()
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gsplat-io-sog-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
