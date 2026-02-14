//! Packed runtime format primitives.

use gsplat_core::{SceneBuffers, Vec3f};
use thiserror::Error;

const MAGIC: [u8; 4] = *b"GSPK";
const VERSION_V2: u32 = 2;
const FLAG_HAS_SH_REST: u32 = 1 << 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedHeader {
    pub version: u32,
    pub gaussian_count: u32,
    pub flags: u32,
    pub sh_degree: u8,
}

#[derive(Debug, Error)]
pub enum FormatError {
    #[error("unsupported format version")]
    UnsupportedVersion,
    #[error("invalid packed blob")]
    InvalidBlob,
    #[error("invalid packed magic")]
    InvalidMagic,
    #[error("truncated packed blob")]
    Truncated,
    #[error("scene does not fit format constraints")]
    SceneTooLarge,
    #[error("scene failed validation")]
    InvalidScene,
}

pub fn pack_scene(scene: &SceneBuffers) -> Result<Vec<u8>, FormatError> {
    scene.validate().map_err(|_| FormatError::InvalidScene)?;

    let count = u32::try_from(scene.len()).map_err(|_| FormatError::SceneTooLarge)?;
    let mut flags = 0_u32;
    if scene.sh_rest.is_some() {
        flags |= FLAG_HAS_SH_REST;
    }

    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&VERSION_V2.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&(scene.sh_degree as u32).to_le_bytes());

    for pos in &scene.positions {
        out.extend_from_slice(&pos.x.to_le_bytes());
        out.extend_from_slice(&pos.y.to_le_bytes());
        out.extend_from_slice(&pos.z.to_le_bytes());
    }

    for value in &scene.opacity {
        out.extend_from_slice(&value.to_le_bytes());
    }

    for value in &scene.scale_xyz {
        out.extend_from_slice(&value[0].to_le_bytes());
        out.extend_from_slice(&value[1].to_le_bytes());
        out.extend_from_slice(&value[2].to_le_bytes());
    }

    for value in &scene.rotation_xyzw {
        out.extend_from_slice(&value[0].to_le_bytes());
        out.extend_from_slice(&value[1].to_le_bytes());
        out.extend_from_slice(&value[2].to_le_bytes());
        out.extend_from_slice(&value[3].to_le_bytes());
    }

    for value in &scene.color_dc {
        out.extend_from_slice(&value[0].to_le_bytes());
        out.extend_from_slice(&value[1].to_le_bytes());
        out.extend_from_slice(&value[2].to_le_bytes());
    }

    if let Some(values) = &scene.sh_rest {
        for value in values {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }

    Ok(out)
}

pub fn unpack_scene(blob: &[u8]) -> Result<SceneBuffers, FormatError> {
    let (header, mut cursor) = parse_header(blob)?;

    let count = header.gaussian_count as usize;
    let mut scene = SceneBuffers {
        positions: Vec::with_capacity(count),
        opacity: Vec::with_capacity(count),
        scale_xyz: Vec::with_capacity(count),
        rotation_xyzw: Vec::with_capacity(count),
        color_dc: Vec::with_capacity(count),
        sh_degree: header.sh_degree,
        sh_rest: if header.flags & FLAG_HAS_SH_REST != 0 {
            let coeff_total = (header.sh_degree as usize + 1).pow(2);
            let rest_stride = 3 * (coeff_total.saturating_sub(1));
            Some(Vec::with_capacity(count * rest_stride))
        } else {
            None
        },
    };

    for _ in 0..count {
        let x = read_f32(blob, &mut cursor)?;
        let y = read_f32(blob, &mut cursor)?;
        let z = read_f32(blob, &mut cursor)?;
        scene.positions.push(Vec3f::new(x, y, z));
    }

    for _ in 0..count {
        scene.opacity.push(read_f32(blob, &mut cursor)?);
    }

    for _ in 0..count {
        scene.scale_xyz.push([
            read_f32(blob, &mut cursor)?,
            read_f32(blob, &mut cursor)?,
            read_f32(blob, &mut cursor)?,
        ]);
    }

    for _ in 0..count {
        scene.rotation_xyzw.push([
            read_f32(blob, &mut cursor)?,
            read_f32(blob, &mut cursor)?,
            read_f32(blob, &mut cursor)?,
            read_f32(blob, &mut cursor)?,
        ]);
    }

    for _ in 0..count {
        scene.color_dc.push([
            read_f32(blob, &mut cursor)?,
            read_f32(blob, &mut cursor)?,
            read_f32(blob, &mut cursor)?,
        ]);
    }

    if let Some(values) = scene.sh_rest.as_mut() {
        let coeff_total = (scene.sh_degree as usize + 1).pow(2);
        let rest_stride = 3 * (coeff_total.saturating_sub(1));
        for _ in 0..(count * rest_stride) {
            values.push(read_f32(blob, &mut cursor)?);
        }
    }

    if cursor != blob.len() {
        return Err(FormatError::InvalidBlob);
    }

    scene.validate().map_err(|_| FormatError::InvalidScene)?;
    Ok(scene)
}

pub fn parse_header(blob: &[u8]) -> Result<(PackedHeader, usize), FormatError> {
    if blob.len() < 20 {
        return Err(FormatError::Truncated);
    }

    if blob[0..4] != MAGIC {
        return Err(FormatError::InvalidMagic);
    }

    let version = u32::from_le_bytes(
        blob[4..8]
            .try_into()
            .map_err(|_| FormatError::InvalidBlob)?,
    );
    if version != VERSION_V2 {
        return Err(FormatError::UnsupportedVersion);
    }

    let gaussian_count = u32::from_le_bytes(
        blob[8..12]
            .try_into()
            .map_err(|_| FormatError::InvalidBlob)?,
    );
    let flags = u32::from_le_bytes(
        blob[12..16]
            .try_into()
            .map_err(|_| FormatError::InvalidBlob)?,
    );

    let sh_degree_u32 = u32::from_le_bytes(
        blob[16..20]
            .try_into()
            .map_err(|_| FormatError::InvalidBlob)?,
    );
    let sh_degree = u8::try_from(sh_degree_u32).map_err(|_| FormatError::InvalidBlob)?;

    Ok((
        PackedHeader {
            version,
            gaussian_count,
            flags,
            sh_degree,
        },
        20,
    ))
}

fn read_f32(blob: &[u8], cursor: &mut usize) -> Result<f32, FormatError> {
    let end = *cursor + 4;
    if end > blob.len() {
        return Err(FormatError::Truncated);
    }

    let value = f32::from_le_bytes(
        blob[*cursor..end]
            .try_into()
            .map_err(|_| FormatError::InvalidBlob)?,
    );
    *cursor = end;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{FormatError, pack_scene, parse_header, unpack_scene};
    use gsplat_core::{SceneBuffers, Vec3f};

    fn sample_scene() -> SceneBuffers {
        SceneBuffers {
            positions: vec![Vec3f::new(0.0, 0.1, 0.2), Vec3f::new(1.0, 2.0, 3.0)],
            opacity: vec![0.5, 0.8],
            scale_xyz: vec![[1.0, 1.0, 1.0], [0.5, 0.6, 0.7]],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0], [0.1, 0.2, 0.3, 0.9]],
            color_dc: vec![[0.2, 0.3, 0.4], [0.7, 0.1, 0.5]],
            sh_degree: 1,
            sh_rest: Some(vec![
                // gaussian 0, R/G/B coeff1..3
                0.0, 0.0, 0.0, 0.01, 0.02, 0.03, 0.04, 0.05, 0.06,
                // gaussian 1
                0.1, 0.2, 0.3, 0.11, 0.12, 0.13, 0.14, 0.15, 0.16,
            ]),
        }
    }

    #[test]
    fn roundtrip_scene_pack_unpack() {
        let scene = sample_scene();
        let blob = pack_scene(&scene).unwrap();
        let unpacked = unpack_scene(&blob).unwrap();
        assert_eq!(scene, unpacked);
    }

    #[test]
    fn parse_header_works() {
        let scene = sample_scene();
        let blob = pack_scene(&scene).unwrap();
        let (header, offset) = parse_header(&blob).unwrap();
        assert_eq!(header.version, 2);
        assert_eq!(header.gaussian_count, 2);
        assert_eq!(header.sh_degree, 1);
        assert_eq!(offset, 20);
    }

    #[test]
    fn rejects_invalid_magic() {
        let mut blob = pack_scene(&sample_scene()).unwrap();
        blob[0] = b'B';
        assert!(matches!(
            parse_header(&blob),
            Err(FormatError::InvalidMagic)
        ));
    }
}
