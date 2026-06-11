import Foundation

#if canImport(GsplatFFI)
import GsplatFFI
#endif

#if canImport(UIKit)
import UIKit
#endif

// Keep internal fallback codes aligned with GsplatErrorCode in gsplat.h without
// exposing raw C enum types in the public Swift API.
private let gsplatOk: Int32 = 0
private let gsplatInvalidArgument: Int32 = 1
private let gsplatUnsupported: Int32 = 4

public struct GsplatKitError: Error, CustomStringConvertible, Equatable {
    public let code: Int32
    public let operation: String
    private let detail: String?

    init(code: Int32, operation: String, detail: String? = nil) {
        self.code = code
        self.operation = operation
        self.detail = detail
    }

    public var message: String {
        detail ?? String(cString: gsplat_error_message(code))
    }

    public var description: String {
        "\(operation): \(message) (code=\(code))"
    }
}

public struct GsplatKitVersion: Equatable {
    public static let supportedMajor: UInt32 = 0
    public static let supportedMinor: UInt32 = 1

    public let major: UInt32
    public let minor: UInt32

    public static var current: GsplatKitVersion {
        GsplatKitVersion(
            major: gsplat_version_major(),
            minor: gsplat_version_minor()
        )
    }

    public static func requireSupported() throws {
        let version = GsplatKitVersion.current
        guard version.major == supportedMajor && version.minor == supportedMinor else {
            throw GsplatKitError(
                code: gsplatUnsupported,
                operation: "gsplat_version",
                detail: "unsupported ABI version \(version.major).\(version.minor)"
            )
        }
    }
}

public struct GsplatRenderConfiguration: Equatable {
    public var width: UInt32
    public var height: UInt32

    public init(width: UInt32 = 800, height: UInt32 = 600) {
        self.width = width
        self.height = height
    }

    func makeRawValue() -> GsplatConfig {
        var config = gsplat_config_default()
        config.width = width
        config.height = height
        return config
    }
}

public struct GsplatFrameStats: Equatable {
    public let frameMs: Float
    public let preprocessMs: Float
    public let sortMs: Float
    public let rasterMs: Float
    public let visibleCount: UInt32
    public let drawnCount: UInt32

    public var hasDrawnContent: Bool {
        visibleCount > 0 && drawnCount > 0
    }

    init(_ raw: GsplatStats) {
        frameMs = raw.frame_ms
        preprocessMs = raw.preprocess_ms
        sortMs = raw.sort_ms
        rasterMs = raw.raster_ms
        visibleCount = raw.visible_count
        drawnCount = raw.drawn_count
    }
}

public final class GsplatContextRenderer {
    private var context: OpaquePointer?
    private let lock = NSLock()

    public init(configuration: GsplatRenderConfiguration = GsplatRenderConfiguration()) throws {
        try GsplatKitVersion.requireSupported()

        var handle: OpaquePointer?
        try check(
            gsplat_context_create(configuration.makeRawValue(), &handle),
            operation: "gsplat_context_create"
        )
        guard let handle else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "gsplat_context_create",
                detail: "native context was nil after a successful create call"
            )
        }
        context = handle
    }

    deinit {
        close()
    }

    public func close() {
        lock.lock()
        defer { lock.unlock() }

        if let context {
            gsplat_context_destroy(context)
            self.context = nil
        }
    }

    public func setDefaultCamera() throws {
        try withContext(operation: "gsplat_context_set_camera") { context in
            try check(
                gsplat_context_set_camera(context, gsplat_camera_default()),
                operation: "gsplat_context_set_camera"
            )
        }
    }

    public func setAutoCamera() throws {
        try withContext(operation: "gsplat_context_set_auto_camera") { context in
            try check(
                gsplat_context_set_auto_camera(context),
                operation: "gsplat_context_set_auto_camera"
            )
        }
    }

    public func loadScene(path: String) throws {
        try withContext(operation: "gsplat_context_load_scene_path") { context in
            try path.withCString { pathPointer in
                try check(
                    gsplat_context_load_scene_path(context, pathPointer),
                    operation: "gsplat_context_load_scene_path"
                )
            }
        }
    }

    public func renderFrame() throws {
        try withContext(operation: "gsplat_context_render_frame") { context in
            try check(
                gsplat_context_render_frame(context),
                operation: "gsplat_context_render_frame"
            )
        }
    }

    public func stats() throws -> GsplatFrameStats {
        try withContext(operation: "gsplat_context_get_stats") { context in
            var stats = GsplatStats()
            try check(
                gsplat_context_get_stats(context, &stats),
                operation: "gsplat_context_get_stats"
            )
            return GsplatFrameStats(stats)
        }
    }

    private func withContext<Result>(
        operation: String,
        _ body: (OpaquePointer) throws -> Result
    ) throws -> Result {
        lock.lock()
        defer { lock.unlock() }

        guard let context else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: operation,
                detail: "renderer is closed"
            )
        }
        return try body(context)
    }
}

#if canImport(UIKit)
public struct GsplatSurfaceOptions: Equatable {
    public var sortInterval: UInt32
    // Experimental benchmark knobs; keep defaults for stable integrations.
    public var gpuPreproject: Bool
    public var gpuPreprojectDoubleBuffer: Bool
    // Static-direct is the default render path (fastest on device benchmarks).
    public var staticDirect: Bool
    public var asyncSort: Bool
    public var asyncGeometry: Bool
    public var instanceBuffers: UInt32
    public var frameLatency: UInt32

    public init(
        sortInterval: UInt32 = 2,
        gpuPreproject: Bool = false,
        gpuPreprojectDoubleBuffer: Bool = false,
        staticDirect: Bool = true,
        asyncSort: Bool = false,
        asyncGeometry: Bool = false,
        instanceBuffers: UInt32 = 1,
        frameLatency: UInt32 = 2
    ) {
        self.sortInterval = sortInterval
        self.gpuPreproject = gpuPreproject
        self.gpuPreprojectDoubleBuffer = gpuPreprojectDoubleBuffer
        self.staticDirect = staticDirect
        self.asyncSort = asyncSort
        self.asyncGeometry = asyncGeometry
        self.instanceBuffers = instanceBuffers
        self.frameLatency = frameLatency
    }
}

public final class GsplatUIKitSurfaceRenderer {
    private var renderer: OpaquePointer?
    private let lock = NSLock()

    public init(
        view: UIView,
        viewController: UIViewController,
        datasetPath: String,
        width: UInt32,
        height: UInt32,
        options: GsplatSurfaceOptions = GsplatSurfaceOptions()
    ) throws {
        try GsplatKitVersion.requireSupported()

        var handle: OpaquePointer?
        let viewPointer = Unmanaged.passUnretained(view).toOpaque()
        let controllerPointer = Unmanaged.passUnretained(viewController).toOpaque()
        try datasetPath.withCString { pathPointer in
            try check(
                gsplat_surface_renderer_create_uikit(
                    viewPointer,
                    controllerPointer,
                    pathPointer,
                    width,
                    height,
                    &handle
                ),
                operation: "gsplat_surface_renderer_create_uikit"
            )
        }
        guard let handle else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "gsplat_surface_renderer_create_uikit",
                detail: "native renderer was nil after a successful create call"
            )
        }

        do {
            try Self.apply(options: options, to: handle)
            renderer = handle
        } catch {
            gsplat_surface_renderer_destroy(handle)
            throw error
        }
    }

    deinit {
        close()
    }

    public func close() {
        lock.lock()
        defer { lock.unlock() }

        if let renderer {
            gsplat_surface_renderer_destroy(renderer)
            self.renderer = nil
        }
    }

    public func resize(width: UInt32, height: UInt32) throws {
        try withRenderer(operation: "gsplat_surface_renderer_resize") { renderer in
            try check(
                gsplat_surface_renderer_resize(renderer, width, height),
                operation: "gsplat_surface_renderer_resize"
            )
        }
    }

    public func resetCamera() throws {
        try withRenderer(operation: "gsplat_surface_renderer_reset_camera") { renderer in
            try check(
                gsplat_surface_renderer_reset_camera(renderer),
                operation: "gsplat_surface_renderer_reset_camera"
            )
        }
    }

    public func orbit(yawRadians: Float, pitchRadians: Float) throws {
        try withRenderer(operation: "gsplat_surface_renderer_orbit") { renderer in
            try check(
                gsplat_surface_renderer_orbit(renderer, yawRadians, pitchRadians),
                operation: "gsplat_surface_renderer_orbit"
            )
        }
    }

    public func zoom(distanceScale: Float) throws {
        try withRenderer(operation: "gsplat_surface_renderer_zoom") { renderer in
            try check(
                gsplat_surface_renderer_zoom(renderer, distanceScale),
                operation: "gsplat_surface_renderer_zoom"
            )
        }
    }

    public func pan(normalizedDeltaX: Float, normalizedDeltaY: Float) throws {
        try withRenderer(operation: "gsplat_surface_renderer_pan") { renderer in
            try check(
                gsplat_surface_renderer_pan(renderer, normalizedDeltaX, normalizedDeltaY),
                operation: "gsplat_surface_renderer_pan"
            )
        }
    }

    public func renderFrame() throws {
        try withRenderer(operation: "gsplat_surface_renderer_render_frame") { renderer in
            try check(
                gsplat_surface_renderer_render_frame(renderer),
                operation: "gsplat_surface_renderer_render_frame"
            )
        }
    }

    public func stats() throws -> GsplatFrameStats {
        try withRenderer(operation: "gsplat_surface_renderer_get_stats") { renderer in
            var stats = GsplatStats()
            try check(
                gsplat_surface_renderer_get_stats(renderer, &stats),
                operation: "gsplat_surface_renderer_get_stats"
            )
            return GsplatFrameStats(stats)
        }
    }

    private static func apply(options: GsplatSurfaceOptions, to renderer: OpaquePointer) throws {
        guard options.sortInterval > 0 else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "GsplatSurfaceOptions",
                detail: "sortInterval must be positive"
            )
        }
        guard options.instanceBuffers > 0 else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "GsplatSurfaceOptions",
                detail: "instanceBuffers must be positive"
            )
        }
        guard options.frameLatency > 0 else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "GsplatSurfaceOptions",
                detail: "frameLatency must be positive"
            )
        }

        let steps: [(String, Int32)] = [
            (
                "gsplat_surface_renderer_set_sort_interval",
                gsplat_surface_renderer_set_sort_interval(renderer, options.sortInterval)
            ),
            (
                "gsplat_surface_renderer_set_gpu_preproject",
                gsplat_surface_renderer_set_gpu_preproject(renderer, options.gpuPreproject ? 1 : 0)
            ),
            (
                "gsplat_surface_renderer_set_gpu_preproject_double_buffer",
                gsplat_surface_renderer_set_gpu_preproject_double_buffer(
                    renderer,
                    options.gpuPreprojectDoubleBuffer ? 1 : 0
                )
            ),
            (
                "gsplat_surface_renderer_set_static_direct",
                gsplat_surface_renderer_set_static_direct(renderer, options.staticDirect ? 1 : 0)
            ),
            (
                "gsplat_surface_renderer_set_async_sort",
                gsplat_surface_renderer_set_async_sort(renderer, options.asyncSort ? 1 : 0)
            ),
            (
                "gsplat_surface_renderer_set_async_geometry",
                gsplat_surface_renderer_set_async_geometry(renderer, options.asyncGeometry ? 1 : 0)
            ),
            (
                "gsplat_surface_renderer_set_instance_buffer_count",
                gsplat_surface_renderer_set_instance_buffer_count(renderer, options.instanceBuffers)
            ),
            (
                "gsplat_surface_renderer_set_frame_latency",
                gsplat_surface_renderer_set_frame_latency(renderer, options.frameLatency)
            ),
        ]

        for (operation, rc) in steps {
            try check(rc, operation: operation)
        }
    }

    private func withRenderer<Result>(
        operation: String,
        _ body: (OpaquePointer) throws -> Result
    ) throws -> Result {
        lock.lock()
        defer { lock.unlock() }

        guard let renderer else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: operation,
                detail: "surface renderer is closed"
            )
        }
        return try body(renderer)
    }
}
#endif

private func check(_ code: Int32, operation: String) throws {
    guard code == gsplatOk else {
        let detail = String(cString: gsplat_last_error_message())
        throw GsplatKitError(
            code: code,
            operation: operation,
            detail: detail == "ok" ? nil : detail
        )
    }
}
