package com.gsplat.android

/** Exact projected-draw execution policy, independent from CPU/GPU ordering. */
enum class GsplatSurfaceProjectedPolicy(internal val nativeValue: Int) {
    CANDIDATE(1),
    COMPACT(2),
    ADAPTIVE(3);

    internal companion object {
        fun fromNative(value: Int): GsplatSurfaceProjectedPolicy =
            entries.firstOrNull { it.nativeValue == value }
                ?: error("unsupported native Surface projected policy: $value")
    }
}

enum class GsplatSurfaceProjectedExecution(internal val nativeValue: Int) {
    CANDIDATE(1),
    COMPACT(2);

    internal companion object {
        fun fromNative(value: Int): GsplatSurfaceProjectedExecution =
            entries.firstOrNull { it.nativeValue == value }
                ?: error("unsupported native Surface projected execution: $value")
    }
}

enum class GsplatSurfaceProjectedAdaptiveState(internal val nativeValue: Int) {
    DISABLED(0),
    CANDIDATE_LEARNING(1),
    CANDIDATE_STABLE(2),
    COMPACT_PROBE(3),
    COMPACT_STABLE(4),
    CANDIDATE_PROBE(5),
    COOLDOWN(6),
    CANDIDATE_ONLY(7);

    internal companion object {
        fun fromNative(value: Int): GsplatSurfaceProjectedAdaptiveState =
            entries.firstOrNull { it.nativeValue == value }
                ?: error("unsupported native Surface projected adaptive state: $value")
    }
}

enum class GsplatSurfaceProjectedUnsampledReason {
    RING_BUSY,
    SURFACE_UNAVAILABLE
}
