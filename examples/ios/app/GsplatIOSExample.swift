import Darwin
import Foundation
import QuartzCore
import UIKit
import UniformTypeIdentifiers

// Example-only qualification symbol. It is deliberately not part of gsplat.h
// or the stable v0.1 C ABI.
@_silgen_name("gsplat_benchmark_set_surface_camera_trace_frame_with_display_policy")
private func gsplatBenchmarkSetSurfaceCameraTraceFrameWithDisplayPolicy(
    _ renderer: OpaquePointer?,
    _ tracePath: UnsafePointer<CChar>?,
    _ frameIndex: UInt32,
    _ requireTraceDisplayMatch: UInt32
) -> Int32

private let bundleDatasetName = "showcase"
private let bundleDatasetExtension = "ply"
private let bundleDatasetLabelExtension = "name"
private let importedPlyName = "imported_scene.ply"
private let minimalPlyName = "minimal_ascii.ply"
private let orbitRadiansPerScreen: Float = 3.2
private let touchEpsilon: Float = 0.0001
private let zoomEpsilon: Float = 0.003
private let targetFrameIntervalSeconds = 1.0 / 60.0
private let currentStatsUISampleInterval = 15
private let firstProjectedDrawTicket: UInt64 = 1 << 52
private let maximumJavaScriptSafeInteger: UInt64 = (1 << 53) - 1
private let showcaseText = UIColor(red: 0.96, green: 0.95, blue: 0.91, alpha: 1)
private let showcaseMuted = UIColor(red: 0.72, green: 0.70, blue: 0.66, alpha: 1)
private let showcaseAccent = UIColor(red: 0.83, green: 0.96, blue: 0.45, alpha: 1)

private func validProjectedV1Header(size: UInt32, version: UInt32, expected: Int) -> Bool {
    size == UInt32(expected) && version == 1
}

private func isProjectedTicket(_ ticket: UInt64) -> Bool {
    (firstProjectedDrawTicket...maximumJavaScriptSafeInteger).contains(ticket)
}

private struct RenderCommand {
    var resize: (width: Int, height: Int)?
    var reset: Bool
    var orbitYaw: Float
    var orbitPitch: Float
    var zoomScale: Float
    var panX: Float
    var panY: Float
}

private struct DatasetSelection {
    var path: String
    var label: String
}

struct BenchmarkConfig {
    var enabled = false
    var frames = 120
    var warmupFrames = 10
    var yawStepRadians: Float = 0.001
    var sortInterval: UInt32 = 1
    var asyncSort = false
    var frameLatency: UInt32 = 2
    var orderBackend = "adaptive"
    var projectedPolicy = "adaptive"
    /// Complete resident production path by default; Direct and Paged remain explicit A/B knobs.
    var geometryPath = "packed"
    var cameraTracePath: String?
    var cameraTraceFrame = 0
    var cameraTraceSequence = false
    var cameraTraceFrameIndices: [Int] = []
    var cameraTraceLoops = 1
    var requireTraceDisplayMatch = true
    var cameraTraceMetadata: CameraTraceMetadata?

    var measuredSampleCount: Int {
        cameraTraceSequence ? frames * cameraTraceLoops : frames
    }

    static func fromArguments(_ arguments: [String]) -> BenchmarkConfig {
        let args = LaunchArguments(arguments)
        var config = BenchmarkConfig()
        config.enabled = args.bool("gsplat_benchmark", default: false)
        let yawStep = args.float("gsplat_benchmark_yaw_step", default: config.yawStepRadians)
        config.yawStepRadians = yawStep.isFinite ? yawStep : config.yawStepRadians
        config.asyncSort = args.bool("gsplat_surface_async_sort", default: config.asyncSort)
        config.frameLatency = UInt32(
            min(max(1, args.int("gsplat_surface_frame_latency", default: Int(config.frameLatency))), 4)
        )
        let geometryPath = args.string("gsplat_geometry_path", default: config.geometryPath).lowercased()
        config.geometryPath = ["packed", "paged"].contains(geometryPath) ? geometryPath : "direct"
        let orderBackend = args.string("gsplat_surface_order_backend", default: config.orderBackend).lowercased()
        precondition(["cpu", "gpu", "adaptive"].contains(orderBackend), "invalid gsplat_surface_order_backend")
        config.orderBackend = orderBackend
        let projectedPolicy = args.string(
            "gsplat_surface_projected_policy",
            default: config.projectedPolicy
        ).lowercased()
        precondition(
            ["candidate", "compact", "adaptive"].contains(projectedPolicy),
            "invalid gsplat_surface_projected_policy"
        )
        config.projectedPolicy = projectedPolicy
        let tracePath = args.string("gsplat_camera_trace", default: "").trimmingCharacters(in: .whitespacesAndNewlines)
        if !tracePath.isEmpty {
            config.cameraTracePath = tracePath.hasPrefix("/")
                ? tracePath
                : (Bundle.main.path(forResource: tracePath, ofType: nil) ?? tracePath)
            config.cameraTraceMetadata = try? CameraTraceMetadata.load(path: config.cameraTracePath!)
            precondition(config.cameraTraceMetadata != nil, "invalid camera trace at \(config.cameraTracePath!)")
        }
        config.cameraTraceSequence = args.bool("gsplat_camera_trace_sequence", default: false)
        precondition(!config.cameraTraceSequence || config.cameraTraceMetadata != nil,
                     "gsplat_camera_trace_sequence requires gsplat_camera_trace")
        precondition(!config.cameraTraceSequence || !args.contains("gsplat_camera_trace_frame"),
                     "gsplat_camera_trace_frame cannot be combined with sequence playback")
        let frameIndicesText = args.string("gsplat_camera_frame_indices", default: "")
        if config.cameraTraceSequence {
            config.cameraTraceFrameIndices = frameIndicesText.isEmpty
                ? Array(config.cameraTraceMetadata!.timestamps.indices)
                : parseCameraTraceFrameIndices(frameIndicesText)
            precondition(config.cameraTraceFrameIndices.count >= 2,
                         "camera trace sequence requires at least two frame indices")
            precondition(
                config.cameraTraceFrameIndices.allSatisfy(config.cameraTraceMetadata!.timestamps.indices.contains),
                "camera trace frame index is out of range"
            )
        } else {
            precondition(frameIndicesText.isEmpty && !args.contains("gsplat_camera_trace_loops"),
                         "camera trace sequence options require gsplat_camera_trace_sequence")
        }
        let defaultFrames = config.cameraTraceSequence ? config.cameraTraceFrameIndices.count : config.frames
        config.frames = max(1, args.int("gsplat_benchmark_frames", default: defaultFrames))
        let defaultWarmup = config.cameraTraceSequence ? 0 : config.warmupFrames
        config.warmupFrames = max(0, args.int("gsplat_benchmark_warmup_frames", default: defaultWarmup))
        let defaultSortInterval = config.cameraTraceSequence ? 1 : Int(config.sortInterval)
        config.sortInterval = UInt32(max(1, args.int("gsplat_surface_sort_interval", default: defaultSortInterval)))
        precondition(!config.cameraTraceSequence || config.sortInterval == 1,
                     "camera trace sequence requires gsplat_surface_sort_interval=1")
        config.cameraTraceLoops = max(1, args.int("gsplat_camera_trace_loops", default: 1))
        config.requireTraceDisplayMatch = args.bool("gsplat_require_trace_display_match", default: true)
        config.cameraTraceFrame = max(0, args.int("gsplat_camera_trace_frame", default: 0))
        if !config.cameraTraceSequence, let metadata = config.cameraTraceMetadata {
            precondition(metadata.timestamps.indices.contains(config.cameraTraceFrame),
                         "camera trace frame index is out of range")
        }
        return config
    }
}

private func parseCameraTraceFrameIndices(_ value: String) -> [Int] {
    let indices = value.split(separator: ",", omittingEmptySubsequences: false).map { part -> Int in
        guard let index = Int(part.trimmingCharacters(in: .whitespaces)), index >= 0 else {
            preconditionFailure("gsplat_camera_frame_indices must contain non-negative integers")
        }
        return index
    }
    precondition(!indices.isEmpty && Set(indices).count == indices.count,
                 "gsplat_camera_frame_indices must be non-empty and unique")
    return indices
}

struct CameraTraceMetadata {
    let id: String
    let sha256: String
    let width: Int
    let height: Int
    let timestamps: [UInt64]

    static func load(path: String) throws -> CameraTraceMetadata {
        let data = try Data(contentsOf: URL(fileURLWithPath: path))
        guard let root = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              root["schema"] as? String == "gsplat-camera-trace/v1",
              let id = root["trace_id"] as? String, !id.isEmpty,
              let sha256 = root["content_sha256"] as? String,
              sha256.range(of: "^[0-9a-f]{64}$", options: .regularExpression) != nil,
              let display = root["display"] as? [String: Any],
              let width = display["width"] as? Int, width > 0,
              let height = display["height"] as? Int, height > 0,
              let frames = root["frames"] as? [[String: Any]], !frames.isEmpty else {
            throw CameraTraceMetadataError.invalid
        }
        var timestamps: [UInt64] = []
        for (index, frame) in frames.enumerated() {
            guard frame["frame_index"] as? Int == index,
                  let timestamp = (frame["timestamp_ns"] as? NSNumber)?.uint64Value,
                  timestamps.last.map({ timestamp > $0 }) ?? true else {
                throw CameraTraceMetadataError.invalid
            }
            timestamps.append(timestamp)
        }
        return CameraTraceMetadata(id: id, sha256: sha256, width: width, height: height, timestamps: timestamps)
    }
}

private enum CameraTraceMetadataError: Error { case invalid }

struct CameraTraceStep {
    let phase: String
    let loopIndex: Int
    let phaseFrameIndex: Int
    let measuredSampleIndex: Int?
    let traceFrameIndex: Int
    let timestampNs: UInt64
}

/// Maps the experimental geometry-path label to the `GsplatGeometryPath` FFI value.
func geometryPathValue(_ label: String) -> UInt32 {
    switch label {
    case "packed": return 1
    case "paged": return 2
    default: return 0
    }
}

/// Maps the experimental geometry-path label to the artifact `renderer.path` name.
func geometryPipelineName(_ label: String) -> String {
    switch label {
    case "packed": return "packed_atlas"
    case "paged": return "paged_active_atlas"
    default: return "sorted_index_direct"
    }
}

func orderBackendValue(_ label: String) -> UInt32 {
    switch label {
    case "gpu": return 1
    case "adaptive": return 2
    default: return 0
    }
}

func projectedPolicyValue(_ label: String) -> UInt32 {
    switch label {
    case "candidate": return 1
    case "compact": return 2
    default: return 3
    }
}

private struct LaunchArguments {
    private var values: [String: String] = [:]

    init(_ arguments: [String]) {
        var index = 1
        while index < arguments.count {
            let argument = arguments[index]
            guard argument.hasPrefix("--") else {
                index += 1
                continue
            }

            let raw = String(argument.dropFirst(2))
            if let equalIndex = raw.firstIndex(of: "=") {
                let key = normalize(String(raw[..<equalIndex]))
                let value = String(raw[raw.index(after: equalIndex)...])
                values[key] = value
            } else {
                let key = normalize(raw)
                if index + 1 < arguments.count && !arguments[index + 1].hasPrefix("--") {
                    values[key] = arguments[index + 1]
                    index += 1
                } else {
                    values[key] = "true"
                }
            }
            index += 1
        }
    }

    func bool(_ key: String, default defaultValue: Bool) -> Bool {
        guard let value = values[normalize(key)]?.lowercased() else {
            return defaultValue
        }
        if ["1", "true", "yes", "y", "on"].contains(value) {
            return true
        }
        if ["0", "false", "no", "n", "off"].contains(value) {
            return false
        }
        return defaultValue
    }

    func int(_ key: String, default defaultValue: Int) -> Int {
        values[normalize(key)].flatMap(Int.init) ?? defaultValue
    }

    func contains(_ key: String) -> Bool {
        values[normalize(key)] != nil
    }

    func float(_ key: String, default defaultValue: Float) -> Float {
        values[normalize(key)].flatMap(Float.init) ?? defaultValue
    }

    func string(_ key: String, default defaultValue: String) -> String {
        values[normalize(key)] ?? defaultValue
    }

    private func normalize(_ key: String) -> String {
        key.replacingOccurrences(of: "-", with: "_")
    }
}

@main
final class AppDelegate: UIResponder, UIApplicationDelegate {
    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
    ) -> Bool {
        true
    }

    func application(
        _ application: UIApplication,
        configurationForConnecting connectingSceneSession: UISceneSession,
        options: UIScene.ConnectionOptions
    ) -> UISceneConfiguration {
        let configuration = UISceneConfiguration(
            name: "Default",
            sessionRole: connectingSceneSession.role
        )
        configuration.delegateClass = SceneDelegate.self
        return configuration
    }
}

final class SceneDelegate: UIResponder, UIWindowSceneDelegate {
    var window: UIWindow?

    func scene(
        _ scene: UIScene,
        willConnectTo session: UISceneSession,
        options connectionOptions: UIScene.ConnectionOptions
    ) {
        guard let windowScene = scene as? UIWindowScene else {
            return
        }

        let window = UIWindow(windowScene: windowScene)
        window.rootViewController = ExampleViewController()
        window.makeKeyAndVisible()
        self.window = window
    }
}

final class MetalSurfaceView: UIView {
    override class var layerClass: AnyClass {
        CAMetalLayer.self
    }
}

final class ExampleViewController: UIViewController, UIGestureRecognizerDelegate, UIDocumentPickerDelegate {
    private let surfaceView = MetalSurfaceView()
    private let statusLabel = UILabel()
    private let importButton = UIButton(type: .system)
    private let studioButton = UIButton(type: .system)
    private let studioPanel = UIVisualEffectView(effect: UIBlurEffect(style: .systemUltraThinMaterialDark))
    private let sceneTitleLabel = UILabel()
    private let sceneMetaLabel = UILabel()
    private let renderQueue = DispatchQueue(label: "com.gsplat.example.ios.render")
    private let commandLock = NSLock()
    private let renderStateLock = NSLock()
    private var benchmarkConfig = BenchmarkConfig.fromArguments(ProcessInfo.processInfo.arguments)
    private var renderer: OpaquePointer?
    private var currentSurfaceSize: (width: Int, height: Int)?
    private var surfaceExactness: GsplatSurfaceExactness?
    private var datasetPath = ""
    private var datasetLabel = "pending"
    private var latestState = "state=launching"
    private var cameraState = "camera=auto"
    private var renderLoopActive = false
    private var pendingResize: (width: Int, height: Int)?
    private var pendingResetCamera = false
    private var pendingOrbitYaw: Float = 0
    private var pendingOrbitPitch: Float = 0
    private var pendingZoomScale: Float = 1
    private var pendingPanX: Float = 0
    private var pendingPanY: Float = 0
    private var lastAdaptiveGpuFailureReason: UInt32?

    override var supportedInterfaceOrientations: UIInterfaceOrientationMask {
        benchmarkConfig.enabled ? .landscape : .allButUpsideDown
    }

    override var preferredInterfaceOrientationForPresentation: UIInterfaceOrientation {
        benchmarkConfig.enabled ? .landscapeRight : .portrait
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .black
        setDataset(resolveInitialDataset())
        configureSurfaceView()
        configureImportButton()
        configureStatusLabel()
        configureGestures()
        setStatus("state=waiting_for_surface")
    }

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        requestBenchmarkLandscapeOrientation()
        createRendererIfNeeded()
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        if benchmarkConfig.enabled, renderer == nil, view.window != nil {
            createRendererIfNeeded()
        }
        resizeRendererIfNeeded()
    }

    private func requestBenchmarkLandscapeOrientation() {
        guard benchmarkConfig.enabled else { return }
        setNeedsUpdateOfSupportedInterfaceOrientations()
        view.window?.windowScene?.requestGeometryUpdate(
            .iOS(interfaceOrientations: .landscape)
        ) { error in
            print("IOS_BENCHMARK_ORIENTATION_FAILED error=\(error.localizedDescription)")
            fflush(stdout)
        }
    }

    override func viewWillDisappear(_ animated: Bool) {
        stopRenderer()
        super.viewWillDisappear(animated)
    }

    private func configureSurfaceView() {
        surfaceView.translatesAutoresizingMaskIntoConstraints = false
        surfaceView.backgroundColor = .black
        surfaceView.isMultipleTouchEnabled = true
        view.addSubview(surfaceView)
        NSLayoutConstraint.activate([
            surfaceView.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            surfaceView.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            surfaceView.topAnchor.constraint(equalTo: view.topAnchor),
            surfaceView.bottomAnchor.constraint(equalTo: view.bottomAnchor),
        ])
    }

    private func configureStatusLabel() {
        let brandLabel = UILabel()
        brandLabel.translatesAutoresizingMaskIntoConstraints = false
        brandLabel.text = "gsplat.rs   /   METAL + WGPU"
        brandLabel.textColor = showcaseAccent
        brandLabel.font = .systemFont(ofSize: 11, weight: .bold)
        brandLabel.adjustsFontForContentSizeCategory = true

        let heroLabel = UILabel()
        heroLabel.translatesAutoresizingMaskIntoConstraints = false
        heroLabel.numberOfLines = 2
        heroLabel.text = "Captured light.\nStill alive."
        heroLabel.textColor = showcaseText
        heroLabel.font = .systemFont(ofSize: 42, weight: .bold)
        heroLabel.adjustsFontSizeToFitWidth = true
        heroLabel.minimumScaleFactor = 0.78

        let subtitleLabel = UILabel()
        subtitleLabel.translatesAutoresizingMaskIntoConstraints = false
        subtitleLabel.numberOfLines = 2
        subtitleLabel.text = "A living Gaussian splat, rendered natively\nby Rust on your phone."
        subtitleLabel.textColor = showcaseMuted
        subtitleLabel.font = .systemFont(ofSize: 14, weight: .regular)

        sceneTitleLabel.textColor = showcaseText
        sceneTitleLabel.font = .systemFont(ofSize: 15, weight: .semibold)
        sceneTitleLabel.adjustsFontSizeToFitWidth = true
        sceneTitleLabel.minimumScaleFactor = 0.8
        sceneMetaLabel.textColor = showcaseMuted
        sceneMetaLabel.font = .monospacedSystemFont(ofSize: 10, weight: .medium)
        sceneMetaLabel.adjustsFontSizeToFitWidth = true
        sceneMetaLabel.minimumScaleFactor = 0.72

        let sceneStack = UIStackView(arrangedSubviews: [sceneTitleLabel, sceneMetaLabel])
        sceneStack.translatesAutoresizingMaskIntoConstraints = false
        sceneStack.axis = .vertical
        sceneStack.spacing = 5

        let sceneCard = UIVisualEffectView(effect: UIBlurEffect(style: .systemUltraThinMaterialDark))
        sceneCard.translatesAutoresizingMaskIntoConstraints = false
        sceneCard.layer.cornerRadius = 14
        sceneCard.layer.cornerCurve = .continuous
        sceneCard.layer.masksToBounds = true
        sceneCard.layer.borderWidth = 0.5
        sceneCard.layer.borderColor = UIColor.white.withAlphaComponent(0.25).cgColor
        sceneCard.isUserInteractionEnabled = false
        sceneCard.contentView.addSubview(sceneStack)

        studioButton.translatesAutoresizingMaskIntoConstraints = false
        var studioConfiguration = UIButton.Configuration.plain()
        studioConfiguration.title = "Studio"
        studioConfiguration.baseForegroundColor = showcaseText
        studioConfiguration.contentInsets = NSDirectionalEdgeInsets(top: 9, leading: 15, bottom: 9, trailing: 15)
        var studioBackground = UIBackgroundConfiguration.clear()
        studioBackground.backgroundColor = UIColor.black.withAlphaComponent(0.62)
        studioBackground.cornerRadius = 20
        studioBackground.strokeColor = UIColor.white.withAlphaComponent(0.25)
        studioBackground.strokeWidth = 0.5
        studioConfiguration.background = studioBackground
        studioButton.configuration = studioConfiguration
        studioButton.accessibilityLabel = "Toggle live diagnostics"
        studioButton.addTarget(self, action: #selector(toggleStudioPanel), for: .touchUpInside)

        studioPanel.translatesAutoresizingMaskIntoConstraints = false
        studioPanel.layer.cornerRadius = 14
        studioPanel.layer.cornerCurve = .continuous
        studioPanel.layer.masksToBounds = true
        studioPanel.layer.borderWidth = 0.5
        studioPanel.layer.borderColor = UIColor.white.withAlphaComponent(0.25).cgColor
        studioPanel.isHidden = true
        studioPanel.accessibilityIdentifier = "studioDiagnosticsPanel"

        let studioLabel = UILabel()
        studioLabel.translatesAutoresizingMaskIntoConstraints = false
        studioLabel.text = "STUDIO / LIVE DIAGNOSTICS"
        studioLabel.textColor = showcaseAccent
        studioLabel.font = .systemFont(ofSize: 10, weight: .bold)

        statusLabel.translatesAutoresizingMaskIntoConstraints = false
        statusLabel.numberOfLines = 0
        statusLabel.textColor = showcaseText
        statusLabel.font = .monospacedSystemFont(ofSize: 11, weight: .regular)
        statusLabel.isUserInteractionEnabled = false
        statusLabel.text = buildStatusText()
        statusLabel.accessibilityIdentifier = "liveDiagnostics"

        studioPanel.contentView.addSubview(studioLabel)
        studioPanel.contentView.addSubview(statusLabel)
        view.addSubview(brandLabel)
        view.addSubview(heroLabel)
        view.addSubview(subtitleLabel)
        view.addSubview(sceneCard)
        view.addSubview(studioPanel)
        view.addSubview(studioButton)
        NSLayoutConstraint.activate([
            brandLabel.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor, constant: 22),
            brandLabel.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor, constant: 18),
            studioButton.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor, constant: -18),
            studioButton.centerYAnchor.constraint(equalTo: brandLabel.centerYAnchor),

            heroLabel.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor, constant: 22),
            heroLabel.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor, constant: -26),
            heroLabel.topAnchor.constraint(equalTo: brandLabel.bottomAnchor, constant: 30),
            subtitleLabel.leadingAnchor.constraint(equalTo: heroLabel.leadingAnchor, constant: 2),
            subtitleLabel.trailingAnchor.constraint(equalTo: heroLabel.trailingAnchor),
            subtitleLabel.topAnchor.constraint(equalTo: heroLabel.bottomAnchor, constant: 14),

            sceneStack.leadingAnchor.constraint(equalTo: sceneCard.contentView.leadingAnchor, constant: 16),
            sceneStack.trailingAnchor.constraint(equalTo: sceneCard.contentView.trailingAnchor, constant: -16),
            sceneStack.topAnchor.constraint(equalTo: sceneCard.contentView.topAnchor, constant: 13),
            sceneStack.bottomAnchor.constraint(equalTo: sceneCard.contentView.bottomAnchor, constant: -13),
            sceneCard.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor, constant: 18),
            sceneCard.bottomAnchor.constraint(equalTo: view.safeAreaLayoutGuide.bottomAnchor, constant: -18),
            sceneCard.trailingAnchor.constraint(lessThanOrEqualTo: importButton.leadingAnchor, constant: -10),
            sceneCard.widthAnchor.constraint(lessThanOrEqualToConstant: 230),

            studioPanel.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor, constant: 18),
            studioPanel.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor, constant: -18),
            studioPanel.topAnchor.constraint(equalTo: brandLabel.bottomAnchor, constant: 24),
            studioLabel.leadingAnchor.constraint(equalTo: studioPanel.contentView.leadingAnchor, constant: 16),
            studioLabel.trailingAnchor.constraint(equalTo: studioPanel.contentView.trailingAnchor, constant: -16),
            studioLabel.topAnchor.constraint(equalTo: studioPanel.contentView.topAnchor, constant: 15),
            statusLabel.leadingAnchor.constraint(equalTo: studioPanel.contentView.leadingAnchor, constant: 16),
            statusLabel.trailingAnchor.constraint(equalTo: studioPanel.contentView.trailingAnchor, constant: -16),
            statusLabel.topAnchor.constraint(equalTo: studioLabel.bottomAnchor, constant: 10),
            statusLabel.bottomAnchor.constraint(equalTo: studioPanel.contentView.bottomAnchor, constant: -16),
        ])
        updateShowcaseOverlay()
    }

    private func configureImportButton() {
        importButton.translatesAutoresizingMaskIntoConstraints = false
        var background = UIBackgroundConfiguration.clear()
        background.backgroundColor = showcaseText
        background.cornerRadius = 22
        var configuration = UIButton.Configuration.plain()
        configuration.title = "Open PLY  +"
        configuration.baseForegroundColor = .black
        configuration.contentInsets = NSDirectionalEdgeInsets(top: 11, leading: 17, bottom: 11, trailing: 17)
        configuration.background = background
        importButton.configuration = configuration
        importButton.addTarget(self, action: #selector(openPlyPicker), for: .touchUpInside)
        view.addSubview(importButton)
        NSLayoutConstraint.activate([
            importButton.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor, constant: -18),
            importButton.bottomAnchor.constraint(equalTo: view.safeAreaLayoutGuide.bottomAnchor, constant: -18),
        ])
    }

    private func configureGestures() {
        let orbitPan = UIPanGestureRecognizer(target: self, action: #selector(handleOrbitPan(_:)))
        orbitPan.minimumNumberOfTouches = 1
        orbitPan.maximumNumberOfTouches = 1
        orbitPan.delegate = self
        surfaceView.addGestureRecognizer(orbitPan)

        let transformPan = UIPanGestureRecognizer(target: self, action: #selector(handleTransformPan(_:)))
        transformPan.minimumNumberOfTouches = 2
        transformPan.maximumNumberOfTouches = 2
        transformPan.delegate = self
        surfaceView.addGestureRecognizer(transformPan)

        let pinch = UIPinchGestureRecognizer(target: self, action: #selector(handlePinch(_:)))
        pinch.delegate = self
        surfaceView.addGestureRecognizer(pinch)

        let doubleTap = UITapGestureRecognizer(target: self, action: #selector(handleDoubleTap(_:)))
        doubleTap.numberOfTapsRequired = 2
        doubleTap.delegate = self
        surfaceView.addGestureRecognizer(doubleTap)
    }

    func gestureRecognizer(
        _ gestureRecognizer: UIGestureRecognizer,
        shouldRecognizeSimultaneouslyWith otherGestureRecognizer: UIGestureRecognizer
    ) -> Bool {
        true
    }

    private func createRendererIfNeeded() {
        guard renderer == nil else {
            return
        }
        guard !datasetPath.isEmpty else {
            setStatus("state=dataset_missing")
            return
        }
        guard let size = configureDrawableSize() else {
            setStatus("state=surface_not_ready")
            return
        }
        guard !benchmarkConfig.enabled || size.width > size.height else {
            setStatus("state=waiting_for_landscape_surface size=\(size.width)x\(size.height)")
            return
        }

        var handle: OpaquePointer?
        let viewPointer = Unmanaged.passUnretained(surfaceView).toOpaque()
        let controllerPointer = Unmanaged.passUnretained(self).toOpaque()
        let rc = datasetPath.withCString { path in
            gsplat_surface_renderer_create_uikit_with_geometry_path(
                viewPointer,
                controllerPointer,
                path,
                UInt32(size.width),
                UInt32(size.height),
                geometryPathValue(benchmarkConfig.geometryPath),
                &handle
            )
        }
        guard rc == 0, let handle else {
            setStatus("state=create_failed rc=\(rc) error=\(errorMessage(rc))")
            print("IOS_SURFACE_CREATE_FAILED rc=\(rc) error=\(errorMessage(rc))")
            fflush(stdout)
            return
        }

        let configRc = configureRenderer(handle)
        guard configRc == 0 else {
            gsplat_surface_renderer_destroy(handle)
            setStatus("state=create_failed rc=\(configRc) error=\(errorMessage(configRc))")
            return
        }

        var exactness = GsplatSurfaceExactness()
        let exactnessRc = gsplat_surface_renderer_get_exactness(handle, &exactness)
        guard exactnessRc == 0 else {
            gsplat_surface_renderer_destroy(handle)
            print(
                "IOS_SURFACE_EXACTNESS_FAILED rc=\(exactnessRc) " +
                "error=\(errorMessage(exactnessRc))"
            )
            fflush(stdout)
            setStatus("state=create_failed rc=\(exactnessRc) error=\(errorMessage(exactnessRc))")
            return
        }
        let fullQualityFlags: UInt32 = 0b1_1111
        if benchmarkConfig.geometryPath != "paged" &&
            exactness.quality_flags & fullQualityFlags != fullQualityFlags {
            gsplat_surface_renderer_destroy(handle)
            print("IOS_SURFACE_EXACTNESS_REJECTED flags=\(exactness.quality_flags)")
            fflush(stdout)
            setStatus("state=exactness_rejected")
            return
        }
        print(
            "SURFACE_EXACTNESS source=\(exactness.source_splat_count) " +
            "decoded=\(exactness.decoded_splat_count) encoded=\(exactness.encoded_splat_count) " +
            "resident=\(exactness.resident_splat_count) " +
            "addressable=\(exactness.addressable_splat_count) " +
            "source_sh=\(exactness.source_sh_degree) resident_sh=\(exactness.resident_sh_degree) " +
            "flags=\(exactness.quality_flags) " +
            "max_storage_buffers_per_shader_stage=" +
            "\(exactness.max_storage_buffers_per_shader_stage) " +
            "max_storage_buffer_binding_size=\(exactness.max_storage_buffer_binding_size)"
        )
        fflush(stdout)

        renderer = handle
        surfaceExactness = exactness
        currentSurfaceSize = size
        setStatus("state=rendering")
        print("IOS_SURFACE_CREATE_OK dataset=\(datasetLabel) size=\(size.width)x\(size.height)")
        fflush(stdout)
        startRenderLoop(handle)
    }

    private func configureRenderer(_ handle: OpaquePointer) -> Int32 {
        let steps: [(String, Int32)] = [
            ("sort_interval", gsplat_surface_renderer_set_sort_interval(handle, benchmarkConfig.sortInterval)),
            ("order_backend", gsplat_surface_renderer_set_order_backend(
                handle,
                orderBackendValue(benchmarkConfig.orderBackend)
            )),
            ("async_sort", gsplat_surface_renderer_set_async_sort(handle, benchmarkConfig.asyncSort ? 1 : 0)),
            ("frame_latency", gsplat_surface_renderer_set_frame_latency(handle, benchmarkConfig.frameLatency)),
            ("projected_policy_v1", gsplat_surface_renderer_set_projected_policy_v1(
                handle,
                projectedPolicyValue(benchmarkConfig.projectedPolicy)
            )),
        ]

        for (name, rc) in steps where rc != 0 {
            print("IOS_SURFACE_CONFIG_FAILED option=\(name) rc=\(rc) error=\(errorMessage(rc))")
            fflush(stdout)
            return rc
        }

        if let tracePath = benchmarkConfig.cameraTracePath {
            let initialFrame = benchmarkConfig.cameraTraceSequence
                ? benchmarkConfig.cameraTraceFrameIndices.last!
                : benchmarkConfig.cameraTraceFrame
            let traceRc = tracePath.withCString { path in
                gsplatBenchmarkSetSurfaceCameraTraceFrameWithDisplayPolicy(
                    handle,
                    path,
                    UInt32(initialFrame),
                    benchmarkConfig.requireTraceDisplayMatch ? 1 : 0
                )
            }
            if traceRc != 0 {
                print("IOS_SURFACE_CONFIG_FAILED option=camera_trace rc=\(traceRc) error=\(errorMessage(traceRc))")
                fflush(stdout)
                return traceRc
            }
            let metadata = benchmarkConfig.cameraTraceMetadata!
            let mode = benchmarkConfig.cameraTraceSequence ? "trace_sequence" : "fixed_frame"
            let selected = benchmarkConfig.cameraTraceSequence
                ? benchmarkConfig.cameraTraceFrameIndices.map(String.init).joined(separator: ",")
                : String(benchmarkConfig.cameraTraceFrame)
            let receiptFrame = benchmarkConfig.cameraTraceSequence
                ? benchmarkConfig.cameraTraceFrameIndices.first!
                : benchmarkConfig.cameraTraceFrame
            setCameraState("camera=trace mode=\(mode) frames=\(selected)")
            print(
                "CAMERA_TRACE trace_id=\(metadata.id) trace_sha256=\(metadata.sha256) " +
                "mode=\(mode) frame_indices=\(selected) frame_index=\(receiptFrame) " +
                "timestamp_ns=\(metadata.timestamps[receiptFrame]) " +
                "requested_backend=\(benchmarkConfig.orderBackend)"
            )
            fflush(stdout)
        }

        return 0
    }

    private func startRenderLoop(_ renderer: OpaquePointer) {
        setRenderLoopActive(true)
        renderQueue.async { [weak self] in
            guard let self else {
                gsplat_surface_renderer_destroy(renderer)
                return
            }

            let benchmark = SurfaceBenchmark(config: self.benchmarkConfig)
            var currentStatsConsumer = GsplatCurrentStatsConsumer()
            var frameIndex = 0
            while self.isRenderLoopActive() {
                let traceStep = benchmark.nextTraceStep()
                let frameStartNs = DispatchTime.now().uptimeNanoseconds
                var rc: Int32 = 0
                if let traceStep, let tracePath = benchmark.config.cameraTracePath {
                    rc = tracePath.withCString { path in
                        gsplatBenchmarkSetSurfaceCameraTraceFrameWithDisplayPolicy(
                            renderer,
                            path,
                            UInt32(traceStep.traceFrameIndex),
                            benchmark.config.requireTraceDisplayMatch ? 1 : 0
                        )
                    }
                } else if benchmark.config.enabled && benchmark.config.cameraTracePath == nil {
                    rc = gsplat_surface_renderer_orbit(renderer, benchmark.config.yawStepRadians, 0)
                } else {
                    rc = self.applyPendingCommand(renderer)
                }

                let strictCurrentStats = benchmark.nextFrameRequiresCurrentStats
                let requestUIStats = !benchmark.config.enabled &&
                    (frameIndex + 1).isMultiple(of: currentStatsUISampleInterval)
                var currentStatsAdmission: GsplatCurrentStatsRequestStatus?
                if rc == 0 && (strictCurrentStats || requestUIStats) {
                    currentStatsAdmission = self.requestCurrentStats(renderer)
                    if strictCurrentStats && currentStatsAdmission != .requested {
                        let reason = currentStatsAdmission.map(self.currentStatsRequestName)
                            ?? "ffi_error"
                        benchmark.recordCurrentStatsError(
                            "measured current-stats request was not admitted: \(reason)"
                        )
                        self.setStatus("state=benchmark_current_stats_request_error")
                        break
                    }
                }

                let renderCallStartNs = DispatchTime.now().uptimeNanoseconds
                if rc == 0 {
                    rc = gsplat_surface_renderer_render_frame(renderer)
                }
                let renderCallNs = DispatchTime.now().uptimeNanoseconds - renderCallStartNs

                if rc != 0 {
                    self.setStatus("state=render_failed rc=\(rc) error=\(self.errorMessage(rc))")
                    print("IOS_SURFACE_RENDER_FAILED rc=\(rc) error=\(self.errorMessage(rc))")
                    fflush(stdout)
                    break
                }

                frameIndex += 1
                guard let currentSubmission = self.readCurrentStatsSubmission(renderer) else {
                    if benchmark.config.enabled {
                        benchmark.recordCurrentStatsError("current-stats submission getter failed")
                        self.setStatus("state=benchmark_current_stats_submission_error")
                        break
                    }
                    self.updateCurrentStats(
                        receipt: nil,
                        unavailableReason: "submission_error",
                        renderCallNs: renderCallNs,
                        frameIndex: frameIndex
                    )
                    continue
                }
                let submissionEvent = currentStatsConsumer.observe(currentSubmission)
                var measuredCurrentSubmission: GsplatCurrentStatsIssuedSubmission?
                if strictCurrentStats {
                    guard case .pending(let issued) = submissionEvent else {
                        benchmark.recordCurrentStatsError(
                            "measured frame did not publish one new Issued current-stats submission"
                        )
                        self.setStatus("state=benchmark_current_stats_submission_error")
                        break
                    }
                    benchmark.recordCurrentStatsSubmission(issued, traceStep: traceStep)
                    measuredCurrentSubmission = issued
                }

                guard let currentPoll = self.pollCurrentStats(renderer) else {
                    if benchmark.config.enabled {
                        benchmark.recordCurrentStatsError("current-stats poll failed")
                        self.setStatus("state=benchmark_current_stats_poll_error")
                        break
                    }
                    self.updateCurrentStats(
                        receipt: nil,
                        unavailableReason: "poll_error",
                        renderCallNs: renderCallNs,
                        frameIndex: frameIndex
                    )
                    continue
                }
                let currentEvent = currentStatsConsumer.consume(currentPoll)

                if benchmark.config.enabled {
                    benchmark.recordCurrentStatsEvent(currentEvent)
                    if let error = benchmark.currentStatsProtocolError {
                        print("BENCHMARK_CURRENT_STATS_ERROR \(error)")
                        fflush(stdout)
                        self.setStatus("state=benchmark_current_stats_error")
                        break
                    }
                    guard let orderFrame = self.readOrderFrameState(renderer) else {
                        self.setStatus("state=order_status_error")
                        break
                    }
                    benchmark.recordOrderSubmission(orderFrame.submission)
                    benchmark.recordProjectedSubmission(
                        orderFrame.projectedSubmission,
                        orderSubmission: orderFrame.submission
                    )
                    self.logAdaptiveGpuStatus(orderFrame.stats)
                    if !self.logCompletedProjectedMeasurements(
                        renderer,
                        benchmark: benchmark
                    ) {
                        self.setStatus("state=benchmark_projected_measurement_error")
                        break
                    }
                    if !self.logCompletedOrderMeasurements(renderer, benchmark: benchmark) {
                        self.setStatus("state=benchmark_measurement_error")
                        break
                    }
                    if let traceStep {
                        let metadata = benchmark.config.cameraTraceMetadata!
                        print(
                            "CAMERA_TRACE_FRAME trace_id=\(metadata.id) trace_sha256=\(metadata.sha256) " +
                            "phase=\(traceStep.phase) loop=\(traceStep.loopIndex) " +
                            "phase_frame=\(traceStep.phaseFrameIndex) " +
                            "frame_index=\(traceStep.traceFrameIndex) " +
                            "timestamp_ns=\(traceStep.timestampNs) " +
                            "requested_backend=\(benchmark.config.orderBackend)"
                        )
                        fflush(stdout)
                    }
                    if strictCurrentStats {
                        guard let measuredCurrentSubmission else {
                            benchmark.recordCurrentStatsError(
                                "measured sample lost its Issued current-stats submission"
                            )
                            break
                        }
                        benchmark.record(
                            sortStats: orderFrame.stats,
                            submission: orderFrame.submission,
                            projectedSubmission: orderFrame.projectedSubmission,
                            currentStatsSubmission: measuredCurrentSubmission,
                            renderCallNs: renderCallNs,
                            frameStartNs: frameStartNs,
                            traceStep: traceStep
                        )
                    } else {
                        benchmark.recordWarmupFrame(frameStartNs: frameStartNs)
                    }
                    if benchmark.complete {
                        guard self.flushTerminalLedgers(
                            renderer,
                            benchmark: benchmark,
                            currentStatsConsumer: &currentStatsConsumer
                        ) else {
                            self.setStatus("state=benchmark_terminal_ledger_error")
                            break
                        }
                        let size = self.currentSurfaceSize ?? (width: 1, height: 1)
                        guard let exactness = self.surfaceExactness else {
                            self.setStatus("state=benchmark_exactness_missing")
                            print("BENCHMARK_ARTIFACT_ERROR native exactness receipt is missing")
                            fflush(stdout)
                            break
                        }
                        guard let presentation = self.readSurfacePresentation(renderer) else {
                            self.setStatus("state=benchmark_presentation_missing")
                            break
                        }
                        guard benchmark.emitArtifacts(
                            datasetPath: self.datasetPath,
                            datasetLabel: self.datasetLabel,
                            width: size.width,
                            height: size.height,
                            exactness: exactness,
                            presentation: presentation
                        ) else {
                            self.setStatus("state=benchmark_artifact_error")
                            break
                        }
                        let result = benchmark.resultLine(datasetLabel: self.datasetLabel)
                        print(result)
                        fflush(stdout)
                        self.setStatus("state=benchmark_complete \(result)")
                        break
                    }
                } else {
                    let receipt: GsplatCurrentStatsReceipt?
                    if case .ready(let ready) = currentEvent {
                        receipt = ready
                    } else {
                        receipt = nil
                    }
                    self.updateCurrentStats(
                        receipt: receipt,
                        unavailableReason: self.currentStatsUnavailableReason(
                            admission: currentStatsAdmission,
                            submissionEvent: submissionEvent,
                            pollEvent: currentEvent,
                            pendingCount: currentStatsConsumer.pendingCount
                        ),
                        renderCallNs: renderCallNs,
                        frameIndex: frameIndex
                    )
                }

                if !benchmark.config.enabled {
                    Thread.sleep(forTimeInterval: targetFrameIntervalSeconds)
                }
            }

            self.setRenderLoopActive(false)
            gsplat_surface_renderer_destroy(renderer)
            DispatchQueue.main.async { [weak self] in
                if self?.renderer == renderer {
                    self?.renderer = nil
                    self?.surfaceExactness = nil
                }
            }
        }
    }

    private func requestCurrentStats(
        _ renderer: OpaquePointer
    ) -> GsplatCurrentStatsRequestStatus? {
        var native = GsplatSurfaceCurrentStatsRequestV1()
        native.struct_size = UInt32(MemoryLayout<GsplatSurfaceCurrentStatsRequestV1>.size)
        native.version = 1
        let rc = gsplat_surface_renderer_request_current_stats_v1(renderer, &native)
        guard rc == 0 else {
            print("IOS_CURRENT_STATS_REQUEST_FAILED rc=\(rc) error=\(errorMessage(rc))")
            fflush(stdout)
            return nil
        }
        do {
            return try currentStatsRequestStatus(native: native)
        } catch {
            print("IOS_CURRENT_STATS_REQUEST_FAILED invalid V1 payload error=\(error)")
            fflush(stdout)
            return nil
        }
    }

    private func readCurrentStatsSubmission(
        _ renderer: OpaquePointer
    ) -> GsplatCurrentStatsSubmission? {
        var native = GsplatSurfaceCurrentStatsSubmissionV1()
        native.struct_size = UInt32(
            MemoryLayout<GsplatSurfaceCurrentStatsSubmissionV1>.size
        )
        native.version = 1
        let rc = gsplat_surface_renderer_get_current_stats_submission_v1(
            renderer,
            &native
        )
        guard rc == 0 else {
            print("IOS_CURRENT_STATS_SUBMISSION_FAILED rc=\(rc) error=\(errorMessage(rc))")
            fflush(stdout)
            return nil
        }
        do {
            return try GsplatCurrentStatsSubmission(native: native)
        } catch {
            print("IOS_CURRENT_STATS_SUBMISSION_FAILED invalid V1 payload error=\(error)")
            fflush(stdout)
            return nil
        }
    }

    private func pollCurrentStats(
        _ renderer: OpaquePointer
    ) -> GsplatCurrentStatsPoll? {
        var native = GsplatSurfaceCurrentStatsPollV1()
        native.struct_size = UInt32(MemoryLayout<GsplatSurfaceCurrentStatsPollV1>.size)
        native.version = 1
        let rc = gsplat_surface_renderer_poll_current_stats_v1(renderer, &native)
        guard rc == 0 else {
            print("IOS_CURRENT_STATS_POLL_FAILED rc=\(rc) error=\(errorMessage(rc))")
            fflush(stdout)
            return nil
        }
        do {
            return try GsplatCurrentStatsPoll(native: native)
        } catch {
            print("IOS_CURRENT_STATS_POLL_FAILED invalid V1 payload error=\(error)")
            fflush(stdout)
            return nil
        }
    }

    private func logCompletedProjectedMeasurements(
        _ renderer: OpaquePointer,
        benchmark: SurfaceBenchmark? = nil
    ) -> Bool {
        while true {
            var measurement = GsplatSurfaceProjectedMeasurementV1()
            measurement.struct_size = UInt32(
                MemoryLayout<GsplatSurfaceProjectedMeasurementV1>.size
            )
            measurement.version = 1
            var available: UInt32 = 0
            let rc = gsplat_surface_renderer_poll_projected_measurement_v1(
                renderer,
                &measurement,
                &available
            )
            guard rc == 0,
                  available <= 1,
                  validProjectedV1Header(
                      size: measurement.struct_size,
                      version: measurement.version,
                      expected: MemoryLayout<GsplatSurfaceProjectedMeasurementV1>.size
                  ) else {
                print(
                    "IOS_PROJECTED_MEASUREMENT_FAILED rc=\(rc) available=\(available) " +
                        "error=\(errorMessage(rc))"
                )
                fflush(stdout)
                return false
            }
            if available == 0 { break }

            // The V/C/D ledger is bounded. Take the matching counts before any
            // other poll or render call can progress another completion.
            guard let counts = takeProjectedCounts(
                renderer,
                ticket: measurement.ticket,
                cameraRevision: measurement.camera_revision
            ) else { return false }
            guard isProjectedTicket(measurement.ticket),
                  measurement.frame_complete_ms.isFinite,
                  measurement.frame_complete_ms >= 0,
                  (1...2).contains(measurement.execution),
                  measurement.order_backend <= 1,
                  measurement.flags & ~UInt32(0b1111) == 0,
                  measurement.flags & 1 != 0,
                  measurement.flags & (1 << 1) == 0,
                  counts.flags & ~UInt32(0b1) == 0,
                  counts.contributor_count <= counts.visible_count,
                  (measurement.execution == 1
                    ? (measurement.flags & (1 << 2) == 0 &&
                        counts.flags & 1 == 0 &&
                        counts.drawn_count == counts.visible_count)
                    : (measurement.flags & (1 << 2) != 0 &&
                        counts.flags & 1 != 0 &&
                        counts.drawn_count == counts.contributor_count)) else {
                print("IOS_PROJECTED_MEASUREMENT_FAILED invalid identity or V/C/D contract")
                fflush(stdout)
                return false
            }
            print(
                "PROJECTED_MEASUREMENT ticket=\(measurement.ticket) " +
                    "camera_revision=\(measurement.camera_revision) " +
                    "projection_generation=\(measurement.projection_generation) " +
                    "probe_generation=\(measurement.probe_generation) " +
                    "execution=\(measurement.execution) " +
                    "order_backend=\(measurement.order_backend) " +
                    "frame_complete_ms=\(measurement.frame_complete_ms) " +
                    "visible=\(counts.visible_count) contributor=\(counts.contributor_count) " +
                    "drawn=\(counts.drawn_count) flags=\(measurement.flags)"
            )
            fflush(stdout)
            benchmark?.recordProjectedMeasurement(measurement, counts: counts)
            if measurement.flags & (1 << 3) != 0 {
                print("IOS_PROJECTED_MEASUREMENT_FAILED dropped_prior=true")
                fflush(stdout)
                return false
            }
        }

        while true {
            var failure = GsplatSurfaceProjectedFailureV1()
            failure.struct_size = UInt32(MemoryLayout<GsplatSurfaceProjectedFailureV1>.size)
            failure.version = 1
            var available: UInt32 = 0
            let rc = gsplat_surface_renderer_poll_projected_failure_v1(
                renderer,
                &failure,
                &available
            )
            guard rc == 0,
                  available <= 1,
                  validProjectedV1Header(
                      size: failure.struct_size,
                      version: failure.version,
                      expected: MemoryLayout<GsplatSurfaceProjectedFailureV1>.size
                  ) else {
                print(
                    "IOS_PROJECTED_FAILURE_POLL_FAILED rc=\(rc) available=\(available) " +
                        "error=\(errorMessage(rc))"
                )
                fflush(stdout)
                return false
            }
            if available == 0 { return true }
            guard isProjectedTicket(failure.ticket),
                  (1...3).contains(failure.reason),
                  (1...2).contains(failure.execution),
                  failure.order_backend <= 1,
                  failure.flags & ~UInt32(0b1) == 0 else {
                print("IOS_PROJECTED_FAILURE_POLL_FAILED invalid terminal identity")
                fflush(stdout)
                return false
            }
            benchmark?.recordProjectedMeasurementFailure(failure)
            print(
                "PROJECTED_MEASUREMENT_FAILURE ticket=\(failure.ticket) " +
                    "camera_revision=\(failure.camera_revision) " +
                    "projection_generation=\(failure.projection_generation) " +
                    "probe_generation=\(failure.probe_generation) " +
                    "reason=\(failure.reason) execution=\(failure.execution) " +
                    "order_backend=\(failure.order_backend) flags=\(failure.flags)"
            )
            fflush(stdout)
            return false
        }
    }

    private func takeProjectedCounts(
        _ renderer: OpaquePointer,
        ticket: UInt64,
        cameraRevision: UInt64
    ) -> GsplatSurfaceProjectedCountsV1? {
        var counts = GsplatSurfaceProjectedCountsV1()
        counts.struct_size = UInt32(MemoryLayout<GsplatSurfaceProjectedCountsV1>.size)
        counts.version = 1
        var available: UInt32 = 0
        let rc = gsplat_surface_renderer_take_projected_counts_v1(
            renderer,
            ticket,
            &counts,
            &available
        )
        guard rc == 0,
              available == 1,
              validProjectedV1Header(
                  size: counts.struct_size,
                  version: counts.version,
                  expected: MemoryLayout<GsplatSurfaceProjectedCountsV1>.size
              ),
              counts.ticket == ticket,
              counts.camera_revision == cameraRevision else {
            print(
                "IOS_PROJECTED_COUNTS_FAILED rc=\(rc) available=\(available) " +
                    "ticket=\(ticket) revision=\(cameraRevision) error=\(errorMessage(rc))"
            )
            fflush(stdout)
            return nil
        }
        return counts
    }

    private func logCompletedOrderMeasurements(
        _ renderer: OpaquePointer,
        benchmark: SurfaceBenchmark? = nil
    ) -> Bool {
        while true {
            var measurement = GsplatSurfaceCpuOrderMeasurement()
            var available: UInt32 = 0
            let rc = gsplat_surface_renderer_poll_cpu_order_measurement(
                renderer,
                &measurement,
                &available
            )
            if rc != 0 {
                print("IOS_CPU_ORDER_MEASUREMENT_FAILED rc=\(rc) error=\(errorMessage(rc))")
                fflush(stdout)
                return false
            }
            if available == 0 { break }
            guard let counts = takeOrderCounts(
                renderer,
                ticket: measurement.ticket,
                cameraRevision: measurement.camera_revision
            ) else { return false }
            print(
                "CPU_ORDER_MEASUREMENT ticket=\(measurement.ticket) " +
                "camera_revision=\(measurement.camera_revision) " +
                "requested_backend=\(measurement.requested_backend) " +
                "actual_backend=\(measurement.actual_backend) " +
                "adaptive_state=\(measurement.adaptive_state) " +
                "preprocess_ms=\(measurement.preprocess_ms) sort_ms=\(measurement.sort_ms) " +
                "frame_complete_ms=\(measurement.frame_complete_ms) " +
                "count_semantics=candidate_visible_contributor_issued_v1 " +
                "visible=\(counts.visible_count) contributor=\(counts.contributor_count) " +
                "drawn=\(counts.drawn_count) " +
                "exact_contributor_compaction=\(counts.flags & 1 != 0) " +
                "flags=\(measurement.flags)"
            )
            fflush(stdout)
            benchmark?.recordCpuOrderMeasurement(measurement, counts: counts)
        }

        while true {
            var measurement = GsplatSurfaceOrderMeasurement()
            var available: UInt32 = 0
            let rc = gsplat_surface_renderer_poll_order_measurement(
                renderer,
                &measurement,
                &available
            )
            if rc != 0 {
                print("IOS_ORDER_MEASUREMENT_FAILED rc=\(rc) error=\(errorMessage(rc))")
                fflush(stdout)
                return false
            }
            if available == 0 {
                break
            }
            guard let counts = takeOrderCounts(
                renderer,
                ticket: measurement.ticket,
                cameraRevision: measurement.camera_revision
            ) else { return false }

            func optionalValue(_ value: Float, _ bit: UInt32) -> String {
                (measurement.flags & (1 << bit)) != 0 ? String(value) : "null"
            }
            let timingSource: String
            switch measurement.timing_source {
            case 1: timingSource = "timestamp_query"
            case 2: timingSource = "completion"
            default: timingSource = "unknown"
            }
            print(
                "ORDER_MEASUREMENT ticket=\(measurement.ticket) " +
                "camera_revision=\(measurement.camera_revision) timing_source=\(timingSource) " +
                "requested_backend=\(measurement.requested_backend) " +
                "actual_backend=\(measurement.actual_backend) adaptive_state=\(measurement.adaptive_state) " +
                "gpu_preprocess_ms=\(optionalValue(measurement.gpu_preprocess_ms, 0)) " +
                "gpu_radix_ms=\(optionalValue(measurement.gpu_radix_ms, 1)) " +
                "gpu_order_ms=\(optionalValue(measurement.gpu_order_ms, 2)) " +
                "gpu_complete_ms=\(measurement.gpu_complete_ms) " +
                "timestamp_period_ns=\(optionalValue(measurement.timestamp_period_ns, 3)) " +
                "count_semantics=candidate_visible_contributor_issued_v1 " +
                "visible=\(counts.visible_count) contributor=\(counts.contributor_count) " +
                "drawn=\(counts.drawn_count) " +
                "exact_contributor_compaction=\(counts.flags & 1 != 0) " +
                "flags=\(measurement.flags)"
            )
            fflush(stdout)
            benchmark?.recordOrderMeasurement(measurement, counts: counts)
        }

        var sawFailure = false
        while true {
            var failure = GsplatSurfaceOrderMeasurementFailure()
            var available: UInt32 = 0
            let rc = gsplat_surface_renderer_poll_order_measurement_failure(
                renderer,
                &failure,
                &available
            )
            if rc != 0 {
                print("IOS_ORDER_MEASUREMENT_FAILURE_POLL_FAILED rc=\(rc) error=\(errorMessage(rc))")
                fflush(stdout)
                return false
            }
            if available == 0 {
                return !sawFailure
            }
            sawFailure = true
            benchmark?.recordOrderMeasurementFailure(failure)
            let reason: String
            switch failure.reason {
            case 1: reason = "readback_map"
            case 2: reason = "generation_invalidated"
            default: reason = "unknown_\(failure.reason)"
            }
            print(
                "ORDER_MEASUREMENT_FAILURE ticket=\(failure.ticket) " +
                "camera_revision=\(failure.camera_revision) reason=\(reason) " +
                "requested_backend=\(failure.requested_backend) " +
                "actual_backend=\(failure.actual_backend) " +
                "adaptive_state=\(failure.adaptive_state) flags=\(failure.flags)"
            )
            fflush(stdout)
        }
    }

    private func takeOrderCounts(
        _ renderer: OpaquePointer,
        ticket: UInt64,
        cameraRevision: UInt64
    ) -> GsplatSurfaceOrderCounts? {
        var counts = GsplatSurfaceOrderCounts()
        var available: UInt32 = 0
        let rc = gsplat_surface_renderer_take_order_counts(
            renderer,
            ticket,
            &counts,
            &available
        )
        guard rc == 0, available != 0,
              counts.ticket == ticket,
              counts.camera_revision == cameraRevision else {
            print(
                "IOS_ORDER_COUNTS_FAILED rc=\(rc) available=\(available) " +
                "ticket=\(ticket) revision=\(cameraRevision) error=\(errorMessage(rc))"
            )
            fflush(stdout)
            return nil
        }
        return counts
    }

    private func readOrderFrameState(
        _ renderer: OpaquePointer
    ) -> (
        stats: GsplatSurfaceSortStats,
        submission: GsplatSurfaceOrderSubmission,
        projectedSubmission: GsplatSurfaceProjectedSubmissionV1
    )? {
        var stats = GsplatSurfaceSortStats()
        let statsRc = gsplat_surface_renderer_get_sort_stats(renderer, &stats)
        guard statsRc == 0 else {
            print("IOS_ORDER_STATUS_FAILED rc=\(statsRc) error=\(errorMessage(statsRc))")
            fflush(stdout)
            return nil
        }
        var submission = GsplatSurfaceOrderSubmission()
        let submissionRc = gsplat_surface_renderer_get_order_submission(renderer, &submission)
        guard submissionRc == 0 else {
            print(
                "IOS_ORDER_SUBMISSION_FAILED rc=\(submissionRc) " +
                "error=\(errorMessage(submissionRc))"
            )
            fflush(stdout)
            return nil
        }
        guard let projectedSubmission = readProjectedSubmission(renderer) else {
            return nil
        }
        guard projectedSubmission.camera_revision == submission.camera_revision,
              projectedSubmission.order_backend == submission.actual_backend else {
            print(
                "IOS_PROJECTED_SUBMISSION_FAILED projected/order frame identity mismatch"
            )
            fflush(stdout)
            return nil
        }
        if projectedSubmission.flags & 1 != 0 && submission.flags & (1 << 1) != 0 {
            print("IOS_PROJECTED_SUBMISSION_FAILED frame issued order and projected tickets")
            fflush(stdout)
            return nil
        }
        return (stats, submission, projectedSubmission)
    }

    private func readProjectedSubmission(
        _ renderer: OpaquePointer
    ) -> GsplatSurfaceProjectedSubmissionV1? {
        var submission = GsplatSurfaceProjectedSubmissionV1()
        submission.struct_size = UInt32(MemoryLayout<GsplatSurfaceProjectedSubmissionV1>.size)
        submission.version = 1
        let rc = gsplat_surface_renderer_get_projected_submission_v1(renderer, &submission)
        guard rc == 0,
              validProjectedV1Header(
                  size: submission.struct_size,
                  version: submission.version,
                  expected: MemoryLayout<GsplatSurfaceProjectedSubmissionV1>.size
              ),
              submission.requested_policy == projectedPolicyValue(benchmarkConfig.projectedPolicy),
              (1...2).contains(submission.actual_execution),
              submission.order_backend <= 1,
              submission.adaptive_state <= 7,
              submission.flags & ~UInt32(0b111) == 0,
              submission.reserved == 0 else {
            print(
                "IOS_PROJECTED_SUBMISSION_FAILED rc=\(rc) error=\(errorMessage(rc))"
            )
            fflush(stdout)
            return nil
        }
        let ticketIssued = submission.flags & 1 != 0
        let ringBusy = submission.flags & (1 << 1) != 0
        let surfaceUnavailable = submission.flags & (1 << 2) != 0
        guard !(ringBusy && surfaceUnavailable),
              ticketIssued
                ? (isProjectedTicket(submission.ticket) && !ringBusy && !surfaceUnavailable)
                : submission.ticket == 0 else {
            print("IOS_PROJECTED_SUBMISSION_FAILED invalid ticket/unsampled flags")
            fflush(stdout)
            return nil
        }
        if submission.requested_policy != 3 {
            guard submission.actual_execution == submission.requested_policy,
                  submission.adaptive_state == 0,
                  !ticketIssued,
                  !ringBusy,
                  !surfaceUnavailable else {
                print("IOS_PROJECTED_SUBMISSION_FAILED forced policy fabricated telemetry")
                fflush(stdout)
                return nil
            }
        }
        return submission
    }

    private func logAdaptiveGpuStatus(_ status: GsplatSurfaceSortStats) {
        let reason = (status.flags & (1 << 15)) != 0
            ? (status.flags >> 16) & 0b111
            : nil
        if reason != lastAdaptiveGpuFailureReason {
            lastAdaptiveGpuFailureReason = reason
            let label: String
            switch reason {
            case 1: label = "unsupported"
            case 2: label = "initialization"
            case 3: label = "out_of_memory"
            case 4: label = "validation"
            case nil: label = "none"
            default: label = "unknown_\(reason!)"
            }
            print(
                "ADAPTIVE_GPU_STATUS failure=\(label) " +
                "actual_backend=\((status.flags >> 9) & 0b11) " +
                "adaptive_state=\((status.flags >> 12) & 0b111)"
            )
            fflush(stdout)
        }
    }

    private func flushTerminalLedgers(
        _ renderer: OpaquePointer,
        benchmark: SurfaceBenchmark,
        currentStatsConsumer: inout GsplatCurrentStatsConsumer,
        maxFrames: Int = 120
    ) -> Bool {
        if benchmark.terminalLedgersWaitFinished {
            if let error = benchmark.currentStatsLedgerError {
                print("BENCHMARK_CURRENT_STATS_TERMINAL_LEDGER_ERROR \(error)")
                fflush(stdout)
                return false
            }
            if let error = benchmark.orderLedgerError {
                print("BENCHMARK_TERMINAL_LEDGER_ERROR \(error)")
                fflush(stdout)
                return false
            }
            if let error = benchmark.projectedLedgerError {
                print("BENCHMARK_PROJECTED_TERMINAL_LEDGER_ERROR \(error)")
                fflush(stdout)
                return false
            }
            return true
        }
        for _ in 0..<maxFrames {
            let rc = gsplat_surface_renderer_render_frame(renderer)
            guard rc == 0 else {
                print("BENCHMARK_TERMINAL_FLUSH_RENDER_FAILED rc=\(rc) error=\(errorMessage(rc))")
                fflush(stdout)
                return false
            }
            guard let submission = readCurrentStatsSubmission(renderer),
                  let poll = pollCurrentStats(renderer) else {
                benchmark.recordCurrentStatsError(
                    "current-stats submission or poll failed during terminal flush"
                )
                return false
            }
            let submissionEvent = currentStatsConsumer.observe(submission)
            if case .rejected = submissionEvent {
                benchmark.recordCurrentStatsError(
                    "current-stats submission identity drifted during terminal flush"
                )
                return false
            }
            benchmark.recordCurrentStatsEvent(currentStatsConsumer.consume(poll))
            if let error = benchmark.currentStatsProtocolError {
                print("BENCHMARK_CURRENT_STATS_TERMINAL_LEDGER_ERROR \(error)")
                fflush(stdout)
                return false
            }
            guard let orderFrame = readOrderFrameState(renderer) else { return false }
            benchmark.recordOrderSubmission(orderFrame.submission)
            benchmark.recordProjectedSubmission(
                orderFrame.projectedSubmission,
                orderSubmission: orderFrame.submission
            )
            logAdaptiveGpuStatus(orderFrame.stats)
            if !logCompletedProjectedMeasurements(renderer, benchmark: benchmark) {
                return false
            }
            if !logCompletedOrderMeasurements(renderer, benchmark: benchmark) {
                return false
            }
            if benchmark.terminalLedgersWaitFinished { break }
        }
        guard benchmark.terminalLedgersWaitFinished else {
            print("BENCHMARK_TERMINAL_LEDGER_TIMEOUT current/order/projected tickets remain pending")
            fflush(stdout)
            return false
        }
        if let error = benchmark.currentStatsLedgerError {
            print("BENCHMARK_CURRENT_STATS_TERMINAL_LEDGER_ERROR \(error)")
            fflush(stdout)
            return false
        }
        if let error = benchmark.orderLedgerError {
            print("BENCHMARK_TERMINAL_LEDGER_ERROR \(error)")
            fflush(stdout)
            return false
        }
        if let error = benchmark.projectedLedgerError {
            print("BENCHMARK_PROJECTED_TERMINAL_LEDGER_ERROR \(error)")
            fflush(stdout)
            return false
        }
        return true
    }

    private func readSurfacePresentation(
        _ renderer: OpaquePointer
    ) -> GsplatSurfacePresentation? {
        var presentation = GsplatSurfacePresentation()
        let rc = gsplat_surface_renderer_get_presentation(renderer, &presentation)
        guard rc == 0 else {
            print(
                "BENCHMARK_PRESENTATION_ERROR rc=\(rc) error=\(errorMessage(rc))"
            )
            fflush(stdout)
            return nil
        }
        return presentation
    }

    private func stopRenderer() {
        setRenderLoopActive(false)
        renderer = nil
        currentSurfaceSize = nil
        surfaceExactness = nil
        lastAdaptiveGpuFailureReason = nil
    }

    private func restartRendererForDataset() {
        clearPendingCameraCommands()
        setCameraState("camera=auto")
        stopRenderer()
        setStatus("state=dataset_ready")
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { [weak self] in
            self?.createRendererIfNeeded()
        }
    }

    private func resizeRendererIfNeeded() {
        guard let size = configureDrawableSize() else {
            return
        }
        guard currentSurfaceSize?.width != size.width || currentSurfaceSize?.height != size.height else {
            return
        }

        currentSurfaceSize = size
        if renderer != nil {
            queueResize(size)
            setStatus("state=resize_pending size=\(size.width)x\(size.height)")
        }
    }

    private func configureDrawableSize() -> (width: Int, height: Int)? {
        guard surfaceView.bounds.width > 0, surfaceView.bounds.height > 0 else {
            return nil
        }

        let screenScale = surfaceView.window?.screen.scale ?? UIScreen.main.scale
        // Camera traces define camera motion and a reference viewport, not an
        // internal render resolution. Always render at the real CAMetalLayer
        // pixel size so a low-resolution trace cannot silently upscale.
        let width = max(1, Int((surfaceView.bounds.width * screenScale).rounded()))
        let height = max(1, Int((surfaceView.bounds.height * screenScale).rounded()))

        if let layer = surfaceView.layer as? CAMetalLayer {
            layer.contentsScale = screenScale
            layer.drawableSize = CGSize(width: width, height: height)
            layer.isOpaque = true
            layer.framebufferOnly = true
        }

        return (width, height)
    }

    private func updateCurrentStats(
        receipt: GsplatCurrentStatsReceipt?,
        unavailableReason: String,
        renderCallNs: UInt64,
        frameIndex: Int
    ) {
        let callMs = String(format: "%.2f", Double(renderCallNs) / 1_000_000.0)
        let state: String
        if let receipt {
            state = "state=rendering stats=ready ticket=\(receipt.ticket) " +
                "camera=\(receipt.identity.cameraRevision) " +
                "presentation=\(receipt.identity.presentationSequence) " +
                "plan=\(currentStatsPlanName(receipt.identity.executedPlan)) " +
                "S=\(receipt.sourceCount) V=\(receipt.visibleCount) " +
                "C=\(receipt.contributorCount) D=\(receipt.drawnCount) " +
                "call_ms=\(callMs)"
        } else {
            state = "state=rendering stats=unavailable reason=\(unavailableReason) " +
                "call_ms=\(callMs)"
        }
        setStatus(state)
        if frameIndex % 60 == 0 {
            print("IOS_SURFACE_FRAME frame=\(frameIndex) \(state)")
            fflush(stdout)
        }
    }

    private func currentStatsUnavailableReason(
        admission: GsplatCurrentStatsRequestStatus?,
        submissionEvent: GsplatCurrentStatsConsumerEvent,
        pollEvent: GsplatCurrentStatsConsumerEvent,
        pendingCount: Int
    ) -> String {
        switch pollEvent {
        case .unsampled(let status): return currentStatsRequestName(status)
        case .failure(let failure):
            return "terminal_\(currentStatsFailureName(failure.reason))"
        case .rejected: return "identity_mismatch"
        default: break
        }
        if let admission, admission != .requested {
            return currentStatsRequestName(admission)
        }
        if pendingCount > 0 { return "pending" }
        switch submissionEvent {
        case .pending: return "pending"
        case .settled: return "settled"
        case .rejected: return "identity_mismatch"
        default: return "empty"
        }
    }

    private func currentStatsRequestName(
        _ status: GsplatCurrentStatsRequestStatus
    ) -> String {
        switch status {
        case .requested: return "requested"
        case .busy: return "busy"
        case .gpuUnavailable: return "gpu_unavailable"
        case .resourceUnavailable: return "resource_unavailable"
        case .ticketExhausted: return "ticket_exhausted"
        }
    }

    private func currentStatsFailureName(
        _ reason: GsplatCurrentStatsFailureReason
    ) -> String {
        switch reason {
        case .mapFailure: return "map_failure"
        case .generationInvalidated: return "generation_invalidated"
        case .expired: return "expired"
        case .dropped: return "dropped"
        }
    }

    private func currentStatsPlanName(_ plan: GsplatCurrentStatsPlan) -> String {
        switch plan {
        case .cpuPostSort: return "cpu_post_sort"
        case .gpuPostSort: return "gpu_post_sort"
        case .gpuPreproject: return "gpu_preproject"
        }
    }

    @objc private func openPlyPicker() {
        let plyType = UTType(filenameExtension: "ply") ?? .data
        let picker = UIDocumentPickerViewController(forOpeningContentTypes: [plyType, .data], asCopy: true)
        picker.delegate = self
        picker.allowsMultipleSelection = false
        present(picker, animated: true)
    }

    func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
        setStatus("state=import_cancelled")
    }

    func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
        guard let url = urls.first else {
            setStatus("state=import_cancelled")
            return
        }
        importPly(from: url)
    }

    private func importPly(from url: URL) {
        setStatus("state=importing")
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            guard let self else {
                return
            }

            let result = Result {
                try self.copyPlyIntoDocuments(from: url)
            }

            DispatchQueue.main.async { [weak self] in
                guard let self else {
                    return
                }
                switch result {
                case .success(let selection):
                    self.setDataset(selection)
                    self.setStatus("state=imported")
                    self.restartRendererForDataset()
                case .failure(let error):
                    self.setStatus("state=import_failed error=\(self.compactMessage(error))")
                }
            }
        }
    }

    private func copyPlyIntoDocuments(from sourceURL: URL) throws -> DatasetSelection {
        let didStartSecurityScope = sourceURL.startAccessingSecurityScopedResource()
        defer {
            if didStartSecurityScope {
                sourceURL.stopAccessingSecurityScopedResource()
            }
        }

        let fileManager = FileManager.default
        let destination = documentsDirectory().appendingPathComponent(importedPlyName)
        let temp = documentsDirectory().appendingPathComponent("\(importedPlyName).tmp")
        if fileManager.fileExists(atPath: temp.path) {
            try fileManager.removeItem(at: temp)
        }
        try fileManager.copyItem(at: sourceURL, to: temp)
        if fileManager.fileExists(atPath: destination.path) {
            try fileManager.removeItem(at: destination)
        }
        try fileManager.moveItem(at: temp, to: destination)

        let displayName = sourceURL.lastPathComponent.isEmpty ? importedPlyName : sourceURL.lastPathComponent
        return DatasetSelection(path: destination.path, label: "imported:\(displayName)")
    }

    private func resolveInitialDataset() -> DatasetSelection {
        let importedDataset = documentsDirectory().appendingPathComponent(importedPlyName)
        if FileManager.default.fileExists(atPath: importedDataset.path) {
            return DatasetSelection(path: importedDataset.path, label: importedDataset.lastPathComponent)
        }

        if let bundleURL = Bundle.main.url(forResource: bundleDatasetName, withExtension: bundleDatasetExtension) {
            let labelURL = Bundle.main.url(
                forResource: bundleDatasetName,
                withExtension: bundleDatasetLabelExtension
            )
            let sourceLabel = labelURL
                .flatMap { try? String(contentsOf: $0, encoding: .utf8) }
                .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
                .flatMap { $0.isEmpty ? nil : $0 }
            return DatasetSelection(path: bundleURL.path, label: sourceLabel ?? bundleURL.lastPathComponent)
        }

        let minimalURL = documentsDirectory().appendingPathComponent(minimalPlyName)
        if !FileManager.default.fileExists(atPath: minimalURL.path) {
            writeMinimalDataset(to: minimalURL)
        }
        return DatasetSelection(path: minimalURL.path, label: minimalURL.lastPathComponent)
    }

    private func documentsDirectory() -> URL {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
    }

    private func setDataset(_ selection: DatasetSelection) {
        datasetPath = selection.path
        datasetLabel = selection.label
    }

    private func writeMinimalDataset(to url: URL) {
        try? minimalPly.write(to: url, atomically: true, encoding: .utf8)
    }

    @objc private func handleOrbitPan(_ gesture: UIPanGestureRecognizer) {
        guard gesture.state == .began || gesture.state == .changed else {
            return
        }

        let translation = gesture.translation(in: surfaceView)
        gesture.setTranslation(.zero, in: surfaceView)
        let size = max(min(surfaceView.bounds.width, surfaceView.bounds.height), 1)
        let dx = Float(translation.x / size)
        let dy = Float(translation.y / size)
        guard abs(dx) > touchEpsilon || abs(dy) > touchEpsilon else {
            return
        }

        queueCameraOrbit(deltaYawRadians: -dx * orbitRadiansPerScreen, deltaPitchRadians: -dy * orbitRadiansPerScreen)
    }

    @objc private func handleTransformPan(_ gesture: UIPanGestureRecognizer) {
        guard gesture.state == .began || gesture.state == .changed else {
            return
        }

        let translation = gesture.translation(in: surfaceView)
        gesture.setTranslation(.zero, in: surfaceView)
        let width = max(surfaceView.bounds.width, 1)
        let height = max(surfaceView.bounds.height, 1)
        let dx = Float(translation.x / width)
        let dy = Float(translation.y / height)
        guard abs(dx) > touchEpsilon || abs(dy) > touchEpsilon else {
            return
        }

        queueCameraPan(normalizedDeltaX: dx, normalizedDeltaY: dy)
    }

    @objc private func handlePinch(_ gesture: UIPinchGestureRecognizer) {
        guard gesture.state == .began || gesture.state == .changed else {
            return
        }

        let scale = Float(1.0 / gesture.scale).clamped(to: 0.5...2.0)
        gesture.scale = 1.0
        guard abs(scale - 1.0) > zoomEpsilon else {
            return
        }

        queueCameraZoom(distanceScale: scale)
    }

    @objc private func handleDoubleTap(_ gesture: UITapGestureRecognizer) {
        guard gesture.state == .recognized else {
            return
        }
        queueCameraReset()
    }

    private func queueResize(_ size: (width: Int, height: Int)) {
        withCommandLock {
            pendingResize = size
        }
    }

    private func queueCameraReset() {
        withCommandLock {
            pendingResetCamera = true
            pendingOrbitYaw = 0
            pendingOrbitPitch = 0
            pendingZoomScale = 1
            pendingPanX = 0
            pendingPanY = 0
        }
        setCameraState("camera=reset")
    }

    private func queueCameraOrbit(deltaYawRadians: Float, deltaPitchRadians: Float) {
        withCommandLock {
            pendingOrbitYaw += deltaYawRadians
            pendingOrbitPitch += deltaPitchRadians
        }
        setCameraState("camera=orbit")
    }

    private func queueCameraZoom(distanceScale: Float) {
        withCommandLock {
            pendingZoomScale = (pendingZoomScale * distanceScale).clamped(to: 0.001...1000.0)
        }
        setCameraState("camera=zoom")
    }

    private func queueCameraPan(normalizedDeltaX: Float, normalizedDeltaY: Float) {
        withCommandLock {
            pendingPanX += normalizedDeltaX
            pendingPanY += normalizedDeltaY
        }
        setCameraState("camera=pan")
    }

    private func clearPendingCameraCommands() {
        withCommandLock {
            pendingResize = nil
            pendingResetCamera = false
            pendingOrbitYaw = 0
            pendingOrbitPitch = 0
            pendingZoomScale = 1
            pendingPanX = 0
            pendingPanY = 0
        }
    }

    private func applyPendingCommand(_ renderer: OpaquePointer) -> Int32 {
        let command = withCommandLock { () -> RenderCommand? in
            let hasCommand = pendingResize != nil ||
                pendingResetCamera ||
                abs(pendingOrbitYaw) > touchEpsilon ||
                abs(pendingOrbitPitch) > touchEpsilon ||
                abs(pendingZoomScale - 1) > zoomEpsilon ||
                abs(pendingPanX) > touchEpsilon ||
                abs(pendingPanY) > touchEpsilon
            guard hasCommand else {
                return nil
            }

            let command = RenderCommand(
                resize: pendingResize,
                reset: pendingResetCamera,
                orbitYaw: pendingOrbitYaw,
                orbitPitch: pendingOrbitPitch,
                zoomScale: pendingZoomScale,
                panX: pendingPanX,
                panY: pendingPanY
            )
            pendingResize = nil
            pendingResetCamera = false
            pendingOrbitYaw = 0
            pendingOrbitPitch = 0
            pendingZoomScale = 1
            pendingPanX = 0
            pendingPanY = 0
            return command
        }

        guard let command else {
            return 0
        }

        var appliedCameraState: String?
        if let resize = command.resize {
            let rc = gsplat_surface_renderer_resize(renderer, UInt32(resize.width), UInt32(resize.height))
            if rc != 0 {
                return rc
            }
        }
        if command.reset {
            let rc = gsplat_surface_renderer_reset_camera(renderer)
            if rc != 0 {
                setCameraState("camera=reset_error rc=\(rc)")
                return rc
            }
            appliedCameraState = "camera=reset"
        }
        if abs(command.orbitYaw) > touchEpsilon || abs(command.orbitPitch) > touchEpsilon {
            let rc = gsplat_surface_renderer_orbit(renderer, command.orbitYaw, command.orbitPitch)
            if rc != 0 {
                setCameraState("camera=orbit_error rc=\(rc)")
                return rc
            }
            appliedCameraState = "camera=orbit"
        }
        if abs(command.zoomScale - 1) > zoomEpsilon {
            let rc = gsplat_surface_renderer_zoom(renderer, command.zoomScale)
            if rc != 0 {
                setCameraState("camera=zoom_error rc=\(rc)")
                return rc
            }
            appliedCameraState = "camera=zoom"
        }
        if abs(command.panX) > touchEpsilon || abs(command.panY) > touchEpsilon {
            let rc = gsplat_surface_renderer_pan(renderer, command.panX, command.panY)
            if rc != 0 {
                setCameraState("camera=pan_error rc=\(rc)")
                return rc
            }
            if appliedCameraState != "camera=zoom" {
                appliedCameraState = "camera=pan"
            }
        }
        if let appliedCameraState {
            print("IOS_SURFACE_CAMERA \(appliedCameraState)")
            fflush(stdout)
            setCameraState(appliedCameraState)
        }

        return 0
    }

    private func setRenderLoopActive(_ active: Bool) {
        withRenderStateLock {
            renderLoopActive = active
        }
    }

    private func isRenderLoopActive() -> Bool {
        withRenderStateLock {
            renderLoopActive
        }
    }

    private func setStatus(_ state: String) {
        DispatchQueue.main.async { [weak self] in
            guard let self else {
                return
            }
            self.latestState = state
            self.updateShowcaseOverlay()
        }
    }

    private func setCameraState(_ state: String) {
        DispatchQueue.main.async { [weak self] in
            guard let self else {
                return
            }
            self.cameraState = state
            self.updateShowcaseOverlay()
        }
    }

    private func updateShowcaseOverlay() {
        statusLabel.text = buildStatusText()
        sceneTitleLabel.text = sceneTitle()
        sceneMetaLabel.text = compactSceneStatus()
    }

    private func sceneTitle() -> String {
        if datasetLabel.hasPrefix("imported:") {
            return "Imported memory"
        }
        if datasetLabel.localizedCaseInsensitiveContains("showcase") ||
            datasetLabel.localizedCaseInsensitiveContains("kitune") {
            return "Kitsune shrine"
        }
        if datasetLabel.localizedCaseInsensitiveContains("flower") {
            return "Flowers / NVIDIA"
        }
        return "Gaussian scene"
    }

    private func compactSceneStatus() -> String {
        if latestState.hasPrefix("state=rendering") {
            let drawn = statusValue("drawn")?.split(separator: "/").first.map(String.init)
            let frame = statusValue("frame_ms")
            return [
                "LIVE",
                drawn.map { "\($0) SPLATS" },
                frame.map { "\($0) MS" },
            ].compactMap { $0 }.joined(separator: "  ·  ")
        }
        if latestState.contains("failed") || latestState.contains("error") {
            return "ATTENTION  ·  OPEN STUDIO"
        }
        return "LOADING  ·  DRAG TO ORBIT"
    }

    private func statusValue(_ key: String) -> String? {
        latestState
            .split(separator: " ")
            .first { $0.hasPrefix("\(key)=") }
            .map { String($0.dropFirst(key.count + 1)) }
    }

    @objc private func toggleStudioPanel() {
        let opening = studioPanel.isHidden
        if opening {
            studioPanel.alpha = 0
            studioPanel.isHidden = false
        }
        studioButton.configuration?.title = opening ? "Close" : "Studio"
        UIView.animate(withDuration: 0.2, animations: {
            self.studioPanel.alpha = opening ? 1 : 0
        }, completion: { _ in
            if !opening {
                self.studioPanel.isHidden = true
            }
        })
    }

    private func buildStatusText() -> String {
        var lines = [
            "gsplat ios example",
            "abi=\(gsplat_version_major()).\(gsplat_version_minor())",
            "surface=wgpu realtime \(surfaceSizeLabel())",
            latestState,
            cameraState,
            "geometry_pipeline=\(geometryPipelineName(benchmarkConfig.geometryPath))",
        ]
        if benchmarkConfig.enabled {
            let cameraMode: String
            if benchmarkConfig.cameraTraceSequence {
                cameraMode = "trace_sequence"
            } else if benchmarkConfig.cameraTracePath == nil {
                cameraMode = "orbit"
            } else {
                cameraMode = "fixed_camera"
            }
            lines.append(
                "benchmark=\(cameraMode) frames=\(benchmarkConfig.frames) " +
                "warmup=\(benchmarkConfig.warmupFrames) loops=\(benchmarkConfig.cameraTraceLoops)"
            )
        }
        lines.append("dataset=\(datasetLabel)")
        lines.append("path=\(datasetPath)")
        return lines.joined(separator: "\n")
    }

    private func surfaceSizeLabel() -> String {
        guard let currentSurfaceSize else {
            return "pending"
        }
        return "\(currentSurfaceSize.width)x\(currentSurfaceSize.height)"
    }

    private func errorMessage(_ code: Int32) -> String {
        if let detail = gsplat_last_error_message() {
            let message = String(cString: detail)
            if !message.isEmpty && message != "ok" {
                return message
            }
        }
        guard let message = gsplat_error_message(code) else {
            return "unknown"
        }
        return String(cString: message)
    }

    private func compactMessage(_ error: Error) -> String {
        String(describing: error)
            .replacingOccurrences(of: "\n", with: " ")
            .prefix(160)
            .description
    }

    private func withCommandLock<T>(_ body: () -> T) -> T {
        commandLock.lock()
        defer {
            commandLock.unlock()
        }
        return body()
    }

    private func withRenderStateLock<T>(_ body: () -> T) -> T {
        renderStateLock.lock()
        defer {
            renderStateLock.unlock()
        }
        return body()
    }
}

private extension Comparable {
    func clamped(to range: ClosedRange<Self>) -> Self {
        min(max(self, range.lowerBound), range.upperBound)
    }
}

private let minimalPly = """
ply
format ascii 1.0
element vertex 1
property float x
property float y
property float z
property float opacity
property float scale_0
property float scale_1
property float scale_2
property float rot_0
property float rot_1
property float rot_2
property float rot_3
property float f_dc_0
property float f_dc_1
property float f_dc_2
end_header
0.0 0.0 0.5 0.9 1.0 1.0 1.0 1.0 0.0 0.0 0.0 0.9 0.2 0.1
"""
