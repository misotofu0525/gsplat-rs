use gsplat_core::Vec3f;

use crate::data::{
    RESIDENT_CHUNK_SPLATS, ResidentChunkMeta, ResidentColorAux, ResidentCovariance0,
    ResidentCovariance1, ResidentPositionAlpha, ResidentShPlane,
};

use super::{ResidentSceneError, resident_sh_plane_count};

/// Logical CPU payload bytes owned by an exact resident scene.
///
/// This intentionally reports payload bytes rather than allocator overhead.
/// `upload_staging_bytes` becomes zero after a Surface presenter has copied
/// the compact planes into durable GPU buffers; exact positions remain for
/// CPU ordering, camera framing, and benchmark receipts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResidentCpuByteAccounting {
    pub exact_position_bytes: u64,
    pub upload_staging_bytes: u64,
    pub total_payload_bytes: u64,
}

impl ResidentCpuByteAccounting {
    pub fn for_count(splat_count: usize, sh_degree: u8) -> Result<Self, ResidentSceneError> {
        if sh_degree > 3 {
            return Err(ResidentSceneError::UnsupportedShDegree(sh_degree));
        }
        let count = u64::try_from(splat_count).map_err(|_| ResidentSceneError::SizeOverflow)?;
        let sh_plane_count = u64::try_from(resident_sh_plane_count(sh_degree))
            .map_err(|_| ResidentSceneError::SizeOverflow)?;
        let chunk_count = count.div_ceil(RESIDENT_CHUNK_SPLATS as u64);
        let exact_position_bytes = count
            .checked_mul(std::mem::size_of::<Vec3f>() as u64)
            .ok_or(ResidentSceneError::SizeOverflow)?;
        let upload_staging_bytes = count
            .checked_mul(std::mem::size_of::<ResidentPositionAlpha>() as u64)
            .and_then(|bytes| {
                bytes.checked_add(
                    count.checked_mul(std::mem::size_of::<ResidentCovariance0>() as u64)?,
                )
            })
            .and_then(|bytes| {
                bytes
                    .checked_add(count.checked_mul(std::mem::size_of::<ResidentColorAux>() as u64)?)
            })
            .and_then(|bytes| {
                bytes.checked_add(
                    count.checked_mul(std::mem::size_of::<ResidentCovariance1>() as u64)?,
                )
            })
            .and_then(|bytes| {
                bytes.checked_add(
                    count
                        .checked_mul(std::mem::size_of::<ResidentShPlane>() as u64)?
                        .checked_mul(sh_plane_count)?,
                )
            })
            .and_then(|bytes| {
                bytes.checked_add(
                    chunk_count.checked_mul(std::mem::size_of::<ResidentChunkMeta>() as u64)?,
                )
            })
            .ok_or(ResidentSceneError::SizeOverflow)?;
        let total_payload_bytes = exact_position_bytes
            .checked_add(upload_staging_bytes)
            .ok_or(ResidentSceneError::SizeOverflow)?;
        Ok(Self {
            exact_position_bytes,
            upload_staging_bytes,
            total_payload_bytes,
        })
    }
}
