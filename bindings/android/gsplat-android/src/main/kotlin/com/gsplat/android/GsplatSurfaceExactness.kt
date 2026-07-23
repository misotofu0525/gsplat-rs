package com.gsplat.android

/** Source-to-GPU exactness and adapter-admission receipt for this Surface. */
data class GsplatSurfaceExactness(
    val sourceSplatCount: Long,
    val decodedSplatCount: Long,
    val encodedSplatCount: Long,
    val residentSplatCount: Long,
    val addressableSplatCount: Long,
    val sourceShDegree: Int,
    val residentShDegree: Int,
    val sourceMembershipAll: Boolean,
    val samplingDisabled: Boolean,
    val lodDisabled: Boolean,
    val sourceShDegreePreserved: Boolean,
    val partialSceneNotPublished: Boolean,
    val maxStorageBuffersPerShaderStage: Int,
    val maxStorageBufferBindingSize: Long,
    val qualityFlags: Int
) {
    val isFullQuality: Boolean
        get() = qualityFlags and FULL_QUALITY_FLAGS == FULL_QUALITY_FLAGS &&
            sourceSplatCount == decodedSplatCount &&
            sourceSplatCount == encodedSplatCount &&
            sourceSplatCount == residentSplatCount &&
            sourceSplatCount == addressableSplatCount &&
            sourceShDegree == residentShDegree

    internal companion object {
        const val RAW_VALUE_COUNT: Int = 10
        private const val SOURCE_MEMBERSHIP_ALL = 1 shl 0
        private const val SAMPLING_DISABLED = 1 shl 1
        private const val LOD_DISABLED = 1 shl 2
        private const val SH_DEGREE_SOURCE = 1 shl 3
        private const val PARTIAL_SCENE_NOT_PUBLISHED = 1 shl 4
        private const val FULL_QUALITY_FLAGS = SOURCE_MEMBERSHIP_ALL or
            SAMPLING_DISABLED or LOD_DISABLED or SH_DEGREE_SOURCE or
            PARTIAL_SCENE_NOT_PUBLISHED

        fun fromRaw(raw: LongArray): GsplatSurfaceExactness {
            require(raw.size >= RAW_VALUE_COUNT)
            val flags = raw[7].toInt()
            return GsplatSurfaceExactness(
                sourceSplatCount = raw[0],
                decodedSplatCount = raw[1],
                encodedSplatCount = raw[2],
                residentSplatCount = raw[3],
                addressableSplatCount = raw[4],
                sourceShDegree = raw[5].toInt(),
                residentShDegree = raw[6].toInt(),
                sourceMembershipAll = flags and SOURCE_MEMBERSHIP_ALL != 0,
                samplingDisabled = flags and SAMPLING_DISABLED != 0,
                lodDisabled = flags and LOD_DISABLED != 0,
                sourceShDegreePreserved = flags and SH_DEGREE_SOURCE != 0,
                partialSceneNotPublished = flags and PARTIAL_SCENE_NOT_PUBLISHED != 0,
                maxStorageBuffersPerShaderStage = raw[8].toInt(),
                maxStorageBufferBindingSize = raw[9],
                qualityFlags = flags
            )
        }
    }
}
