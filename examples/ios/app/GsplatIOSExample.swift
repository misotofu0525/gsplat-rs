import Darwin
import Foundation
import QuartzCore
import UIKit
import UniformTypeIdentifiers

private let bundleDatasetName = "showcase"
private let bundleDatasetExtension = "ply"
private let bundleDatasetLabelExtension = "name"
private let importedPlyName = "imported_scene.ply"
private let minimalPlyName = "minimal_ascii.ply"
private let maxSurfaceSidePixels = 1600
private let orbitRadiansPerScreen: Float = 3.2
private let touchEpsilon: Float = 0.0001
private let zoomEpsilon: Float = 0.003
private let targetFrameIntervalSeconds = 1.0 / 60.0
private let showcaseText = UIColor(red: 0.96, green: 0.95, blue: 0.91, alpha: 1)
private let showcaseMuted = UIColor(red: 0.72, green: 0.70, blue: 0.66, alpha: 1)
private let showcaseAccent = UIColor(red: 0.83, green: 0.96, blue: 0.45, alpha: 1)

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

private struct BenchmarkConfig {
    var enabled = false
    var frames = 120
    var warmupFrames = 10
    var yawStepRadians: Float = 0.001
    var sortInterval: UInt32 = 2
    var asyncSort = false
    var frameLatency: UInt32 = 2

    static func fromArguments(_ arguments: [String]) -> BenchmarkConfig {
        let args = LaunchArguments(arguments)
        var config = BenchmarkConfig()
        config.enabled = args.bool("gsplat_benchmark", default: false)
        config.frames = max(1, args.int("gsplat_benchmark_frames", default: config.frames))
        config.warmupFrames = max(0, args.int("gsplat_benchmark_warmup_frames", default: config.warmupFrames))
        let yawStep = args.float("gsplat_benchmark_yaw_step", default: config.yawStepRadians)
        config.yawStepRadians = yawStep.isFinite && yawStep != 0 ? yawStep : config.yawStepRadians
        config.sortInterval = UInt32(max(1, args.int("gsplat_surface_sort_interval", default: Int(config.sortInterval))))
        config.asyncSort = args.bool("gsplat_surface_async_sort", default: config.asyncSort)
        config.frameLatency = UInt32(
            min(max(1, args.int("gsplat_surface_frame_latency", default: Int(config.frameLatency))), 4)
        )
        return config
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

    func float(_ key: String, default defaultValue: Float) -> Float {
        values[normalize(key)].flatMap(Float.init) ?? defaultValue
    }

    private func normalize(_ key: String) -> String {
        key.replacingOccurrences(of: "-", with: "_")
    }
}

private final class SurfaceBenchmark {
    let config: BenchmarkConfig
    private(set) var complete = false
    private var observedFrames = 0
    private var samples = 0
    private var totalCallMs: Double = 0
    private var totalFrameMs: Double = 0
    private var totalPreprocessMs: Double = 0
    private var totalSortMs: Double = 0
    private var totalRasterMs: Double = 0
    private var totalVisible: UInt64 = 0
    private var totalDrawn: UInt64 = 0

    init(config: BenchmarkConfig) {
        self.config = config
    }

    func record(stats: GsplatStats, renderCallNs: UInt64) {
        guard config.enabled, !complete else {
            return
        }

        observedFrames += 1
        if observedFrames <= config.warmupFrames {
            return
        }

        samples += 1
        totalCallMs += Double(renderCallNs) / 1_000_000.0
        totalFrameMs += Double(stats.frame_ms)
        totalPreprocessMs += Double(stats.preprocess_ms)
        totalSortMs += Double(stats.sort_ms)
        totalRasterMs += Double(stats.raster_ms)
        totalVisible += UInt64(stats.visible_count)
        totalDrawn += UInt64(stats.drawn_count)
        complete = samples >= config.frames
    }

    func resultLine(datasetLabel: String) -> String {
        let safeSamples = max(samples, 1)
        return [
            "BENCHMARK_RESULT",
            "dataset=\(datasetLabel)",
            "samples=\(samples)",
            "warmup=\(config.warmupFrames)",
            "sort_interval=\(config.sortInterval)",
            "async_sort=\(config.asyncSort)",
            "geometry_pipeline=sorted_index_direct",
            "frame_latency=\(config.frameLatency)",
            "avg_call_ms=\(format(totalCallMs / Double(safeSamples)))",
            "avg_frame_ms=\(format(totalFrameMs / Double(safeSamples)))",
            "avg_preprocess_ms=\(format(totalPreprocessMs / Double(safeSamples)))",
            "avg_sort_ms=\(format(totalSortMs / Double(safeSamples)))",
            "avg_raster_ms=\(format(totalRasterMs / Double(safeSamples)))",
            "avg_visible=\(totalVisible / UInt64(safeSamples))",
            "avg_drawn=\(totalDrawn / UInt64(safeSamples))",
        ].joined(separator: " ")
    }

    private func format(_ value: Double) -> String {
        String(format: "%.3f", value)
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
        createRendererIfNeeded()
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        resizeRendererIfNeeded()
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

        var handle: OpaquePointer?
        let viewPointer = Unmanaged.passUnretained(surfaceView).toOpaque()
        let controllerPointer = Unmanaged.passUnretained(self).toOpaque()
        let rc = datasetPath.withCString { path in
            gsplat_surface_renderer_create_uikit(
                viewPointer,
                controllerPointer,
                path,
                UInt32(size.width),
                UInt32(size.height),
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

        renderer = handle
        currentSurfaceSize = size
        setStatus("state=rendering")
        print("IOS_SURFACE_CREATE_OK dataset=\(datasetLabel) size=\(size.width)x\(size.height)")
        fflush(stdout)
        startRenderLoop(handle)
    }

    private func configureRenderer(_ handle: OpaquePointer) -> Int32 {
        let steps: [(String, Int32)] = [
            ("sort_interval", gsplat_surface_renderer_set_sort_interval(handle, benchmarkConfig.sortInterval)),
            ("async_sort", gsplat_surface_renderer_set_async_sort(handle, benchmarkConfig.asyncSort ? 1 : 0)),
            ("frame_latency", gsplat_surface_renderer_set_frame_latency(handle, benchmarkConfig.frameLatency)),
        ]

        for (name, rc) in steps where rc != 0 {
            print("IOS_SURFACE_CONFIG_FAILED option=\(name) rc=\(rc) error=\(errorMessage(rc))")
            fflush(stdout)
            return rc
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
            var frameIndex = 0
            while self.isRenderLoopActive() {
                let renderStartNs = DispatchTime.now().uptimeNanoseconds
                var rc: Int32 = 0
                if benchmark.config.enabled {
                    rc = gsplat_surface_renderer_orbit(renderer, benchmark.config.yawStepRadians, 0)
                } else {
                    rc = self.applyPendingCommand(renderer)
                }

                if rc == 0 {
                    rc = gsplat_surface_renderer_render_frame(renderer)
                }
                let renderCallNs = DispatchTime.now().uptimeNanoseconds - renderStartNs

                if rc != 0 {
                    self.setStatus("state=render_failed rc=\(rc) error=\(self.errorMessage(rc))")
                    print("IOS_SURFACE_RENDER_FAILED rc=\(rc) error=\(self.errorMessage(rc))")
                    fflush(stdout)
                    break
                }

                frameIndex += 1
                var stats = GsplatStats()
                let statsRc = gsplat_surface_renderer_get_stats(renderer, &stats)
                if statsRc == 0 {
                    if benchmark.config.enabled {
                        benchmark.record(stats: stats, renderCallNs: renderCallNs)
                        if benchmark.complete {
                            let result = benchmark.resultLine(datasetLabel: self.datasetLabel)
                            print(result)
                            fflush(stdout)
                            self.setStatus("state=benchmark_complete \(result)")
                            break
                        }
                    }
                    if frameIndex % 15 == 0 {
                        self.updateStats(stats, frameIndex: frameIndex)
                    }
                } else {
                    self.setStatus("state=stats_failed rc=\(statsRc) error=\(self.errorMessage(statsRc))")
                    break
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
                }
            }
        }
    }

    private func stopRenderer() {
        setRenderLoopActive(false)
        renderer = nil
        currentSurfaceSize = nil
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
        var width = max(1, Int((surfaceView.bounds.width * screenScale).rounded()))
        var height = max(1, Int((surfaceView.bounds.height * screenScale).rounded()))
        let maxSide = max(width, height)
        if maxSide > maxSurfaceSidePixels {
            let scale = CGFloat(maxSurfaceSidePixels) / CGFloat(maxSide)
            width = max(1, Int((CGFloat(width) * scale).rounded()))
            height = max(1, Int((CGFloat(height) * scale).rounded()))
        }

        if let layer = surfaceView.layer as? CAMetalLayer {
            layer.contentsScale = screenScale
            layer.drawableSize = CGSize(width: width, height: height)
            layer.isOpaque = true
            layer.framebufferOnly = true
        }

        return (width, height)
    }

    private func updateStats(_ stats: GsplatStats, frameIndex: Int) {
        let state = String(
            format: "state=rendering drawn=%u/%u frame_ms=%.2f",
            stats.drawn_count,
            stats.visible_count,
            stats.frame_ms
        )
        setStatus(state)
        if frameIndex % 60 == 0 {
            print(
                String(
                    format: "IOS_SURFACE_FRAME frame=%d drawn=%u visible=%u frame_ms=%.2f",
                    frameIndex,
                    stats.drawn_count,
                    stats.visible_count,
                    stats.frame_ms
                )
            )
            fflush(stdout)
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
        ]
        if benchmarkConfig.enabled {
            lines.append("benchmark=orbit frames=\(benchmarkConfig.frames) warmup=\(benchmarkConfig.warmupFrames)")
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
