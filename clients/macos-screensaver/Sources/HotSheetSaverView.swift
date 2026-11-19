import AppKit
import ScreenSaver
import WebKit

/// Where the Hot Sheet server leaves the standby snapshot (HS2-MZM988). The saver host is sandboxed
/// and holds no server secret, so it reads one credential-free JSON file from its own container and
/// never opens a network connection. `NSHomeDirectory()` is that container's `Data` directory.
enum SnapshotLocation {
    static let directoryName = "HotSheet"
    static let fileName = "standby-snapshot.json"
    /// Only the test harness changes this; the saver itself always reads its own container.
    nonisolated(unsafe) static var home: String = NSHomeDirectory()

    static func directory() -> URL {
        URL(fileURLWithPath: home, isDirectory: true)
            .appendingPathComponent("Library/Application Support", isDirectory: true)
            .appendingPathComponent(directoryName, isDirectory: true)
    }

    static func file() -> URL {
        directory().appendingPathComponent(fileName)
    }
}

/// Where the bundled standby page lives. Only the test harness changes `override`.
enum PageLocation {
    nonisolated(unsafe) static var override: URL?

    static func root(for bundleClass: AnyClass) -> URL {
        override ?? Bundle(for: bundleClass).resourceURL!.appendingPathComponent("standby", isDirectory: true)
    }
}

/// Reads the snapshot when it changes. It watches the containing directory, because the writer
/// replaces the file atomically (a rename), which a watch on the file itself would miss. Nothing is
/// polled: an idle machine wakes the saver for nothing.
final class SnapshotWatcher {
    private let directory: URL
    private let file: URL
    private let onSnapshot: (String) -> Void
    private var source: DispatchSourceFileSystemObject?
    private var descriptor: Int32 = -1

    init(directory: URL, file: URL, onSnapshot: @escaping (String) -> Void) {
        self.directory = directory
        self.file = file
        self.onSnapshot = onSnapshot
    }

    func start() {
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        descriptor = open(directory.path, O_EVTONLY)
        guard descriptor >= 0 else { return }
        let source = DispatchSource.makeFileSystemObjectSource(
            fileDescriptor: descriptor, eventMask: [.write, .rename, .extend], queue: .main)
        source.setEventHandler { [weak self] in self?.read() }
        source.setCancelHandler { [descriptor] in close(descriptor) }
        source.resume()
        self.source = source
        read()
    }

    func stop() {
        source?.cancel()
        source = nil
        descriptor = -1
    }

    /// Hands over the file only when it is valid JSON, so a half-written or foreign file is ignored
    /// and the page never evaluates arbitrary text.
    func read() {
        guard let data = try? Data(contentsOf: file),
            (try? JSONSerialization.jsonObject(with: data)) != nil,
            let text = String(data: data, encoding: .utf8)
        else { return }
        onSnapshot(text)
    }
}

/// The Hot Sheet screen saver: the real standby screen, rendered by the bundled web page, fed by the
/// snapshot file (HS2-MZM988, docs/34-glance-surfaces.md §34.5). The page does the drawing and the
/// model derivation, so the saver cannot drift from standby.
@objc(HotSheetSaverView)
public final class HotSheetSaverView: ScreenSaverView {
    private(set) var webView: WKWebView?
    private var watcher: SnapshotWatcher?
    private var pageReady = false
    private var pending: String?

    public override init?(frame: NSRect, isPreview: Bool) {
        super.init(frame: frame, isPreview: isPreview)
        configure()
    }

    public required init?(coder: NSCoder) {
        super.init(coder: coder)
        configure()
    }

    private func configure() {
        wantsLayer = true
        layer?.backgroundColor = NSColor.black.cgColor
        // The page draws everything itself; the engine's per-frame callback is not needed.
        animationTimeInterval = 1
        let configuration = WKWebViewConfiguration()
        let view = WKWebView(frame: bounds, configuration: configuration)
        view.autoresizingMask = [.width, .height]
        view.setValue(false, forKey: "drawsBackground")
        addSubview(view)
        webView = view
        watcher = SnapshotWatcher(directory: SnapshotLocation.directory(), file: SnapshotLocation.file()) {
            [weak self] text in self?.deliver(text)
        }
    }

    public override func startAnimation() {
        super.startAnimation()
        loadPage()
        watcher?.start()
    }

    public override func stopAnimation() {
        super.stopAnimation()
        watcher?.stop()
    }

    public override func animateOneFrame() {}

    public override var hasConfigureSheet: Bool { false }

    private func loadPage() {
        guard let webView, !pageReady else { return }
        let root = PageLocation.root(for: HotSheetSaverView.self)
        webView.navigationDelegate = nil
        webView.loadFileURL(root.appendingPathComponent("index.html"), allowingReadAccessTo: root)
        pageReady = true
    }

    private func deliver(_ text: String) {
        guard let webView else { return }
        // `text` is validated JSON, so it is a valid expression and carries nothing executable.
        webView.evaluateJavaScript("window.hotsheetSaver && window.hotsheetSaver.update(\(text))", completionHandler: nil)
    }
}
