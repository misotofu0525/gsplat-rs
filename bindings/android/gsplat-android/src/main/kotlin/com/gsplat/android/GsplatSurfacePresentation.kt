package com.gsplat.android

/** Native pixel-resolution and actual-presentation receipt for this Surface. */
data class GsplatSurfacePresentation(
    val requestedWidth: Int,
    val requestedHeight: Int,
    val surfaceWidth: Int,
    val surfaceHeight: Int,
    val internalRenderWidth: Int,
    val internalRenderHeight: Int,
    val presentedWidth: Int,
    val presentedHeight: Int,
    val presentedCameraRevision: Long,
    val lastFramePresented: Boolean,
    val everPresented: Boolean,
    val dynamicResolutionDisabled: Boolean,
    val upscalingDisabled: Boolean,
    val fullResolution: Boolean,
    val flags: Int
) {
    val dimensionsMatch: Boolean
        get() = requestedWidth == surfaceWidth &&
            requestedHeight == surfaceHeight &&
            surfaceWidth == internalRenderWidth &&
            surfaceHeight == internalRenderHeight &&
            internalRenderWidth == presentedWidth &&
            internalRenderHeight == presentedHeight

    internal companion object {
        const val RAW_VALUE_COUNT: Int = 11
        private const val LAST_FRAME_PRESENTED = 1 shl 0
        private const val EVER_PRESENTED = 1 shl 1
        private const val DYNAMIC_RESOLUTION_DISABLED = 1 shl 2
        private const val UPSCALING_DISABLED = 1 shl 3
        private const val FULL_RESOLUTION = 1 shl 4

        fun fromRaw(raw: LongArray): GsplatSurfacePresentation {
            require(raw.size >= RAW_VALUE_COUNT)
            val flags = raw[9].toInt()
            val receipt = GsplatSurfacePresentation(
                requestedWidth = raw[0].toInt(),
                requestedHeight = raw[1].toInt(),
                surfaceWidth = raw[2].toInt(),
                surfaceHeight = raw[3].toInt(),
                internalRenderWidth = raw[4].toInt(),
                internalRenderHeight = raw[5].toInt(),
                presentedWidth = raw[6].toInt(),
                presentedHeight = raw[7].toInt(),
                presentedCameraRevision = raw[8],
                lastFramePresented = flags and LAST_FRAME_PRESENTED != 0,
                everPresented = flags and EVER_PRESENTED != 0,
                dynamicResolutionDisabled = flags and DYNAMIC_RESOLUTION_DISABLED != 0,
                upscalingDisabled = flags and UPSCALING_DISABLED != 0,
                fullResolution = flags and FULL_RESOLUTION != 0,
                flags = flags
            )
            check(raw[10] == 0L) { "native presentation receipt reserved field is non-zero" }
            check(receipt.requestedWidth > 0 && receipt.requestedHeight > 0) {
                "native presentation receipt has an invalid requested size"
            }
            check(receipt.surfaceWidth > 0 && receipt.surfaceHeight > 0) {
                "native presentation receipt has an invalid Surface size"
            }
            check(receipt.internalRenderWidth > 0 && receipt.internalRenderHeight > 0) {
                "native presentation receipt has an invalid internal-render size"
            }
            check(!receipt.lastFramePresented || receipt.everPresented) {
                "native presentation receipt marks the last frame presented without history"
            }
            check(!receipt.fullResolution || (
                receipt.lastFramePresented &&
                    receipt.everPresented &&
                    receipt.dynamicResolutionDisabled &&
                    receipt.upscalingDisabled &&
                    receipt.dimensionsMatch
                )) {
                "native full-resolution presentation receipt is inconsistent"
            }
            return receipt
        }
    }
}
