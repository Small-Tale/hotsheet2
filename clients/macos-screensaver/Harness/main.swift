import AppKit
import WebKit

// Hosting check for HotSheet.saver (HS2-MZM988): renders the saver view in a window at several sizes
// and proves the page loads from the bundle resources and that a snapshot file written afterwards
// reaches the page, without launching the real screen saver engine (which takes over the display).
// Usage: harness <standby-page-dir> <snapshot-home-dir> <snapshot-fixture.json> [capture-prefix]
// With a capture prefix it also writes a PNG of the rendered page at each size, for visual review.

let arguments = CommandLine.arguments
guard arguments.count == 4 || arguments.count == 5 else {
    FileHandle.standardError.write(Data("usage: harness <standby-page-dir> <home-dir> <snapshot.json> [capture-prefix]\n".utf8))
    exit(2)
}
SnapshotLocation.home = arguments[2]
PageLocation.override = URL(fileURLWithPath: arguments[1], isDirectory: true)

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data("FAIL: \(message)\n".utf8))
    exit(1)
}

/// Run the main loop until `condition` holds or `seconds` pass.
func waitFor(_ seconds: Double, _ condition: () -> Bool) -> Bool {
    let deadline = Date().addingTimeInterval(seconds)
    while Date() < deadline {
        if condition() { return true }
        RunLoop.main.run(until: Date().addingTimeInterval(0.05))
    }
    return condition()
}

func evaluate(_ view: WKWebView, _ script: String) -> String? {
    var result: String?
    var done = false
    view.evaluateJavaScript(script) { value, _ in
        result = value.map { "\($0)" }
        done = true
    }
    _ = waitFor(5) { done }
    return result
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)

let saver = HotSheetSaverView(frame: NSRect(x: 0, y: 0, width: 1280, height: 800), isPreview: false)!
let window = NSWindow(
    contentRect: saver.frame, styleMask: [.titled], backing: .buffered, defer: false)
window.contentView = saver
window.orderBack(nil)

// The harness points the page lookup at the built bundle's resources; the bundle itself is checked by
// check.sh, which loads the principal class from it.
saver.startAnimation()
guard let web = saver.webView else { fail("the saver has no web view") }
func capture(_ name: String) {
    guard arguments.count == 5, let web = saver.webView else { return }
    var done = false
    web.takeSnapshot(with: nil) { image, _ in
        if let image, let tiff = image.tiffRepresentation, let rep = NSBitmapImageRep(data: tiff),
            let png = rep.representation(using: .png, properties: [:])
        {
            try? png.write(to: URL(fileURLWithPath: "\(arguments[4])-\(name).png"))
        }
        done = true
    }
    _ = waitFor(5) { done }
}

func number(_ script: String) -> Int? { evaluate(web, script).flatMap { Int($0) } }
func text(_ script: String) -> String? { evaluate(web, script) }

guard waitFor(15, { text("document.readyState") == "complete" && text("typeof window.hotsheetSaver") == "object" })
else { fail("the standby page did not load from the bundle resources") }
// The real standby screen is on the page, empty and not live, before any snapshot exists.
guard number("document.querySelectorAll('[data-component=\"standby-screen\"]').length") == 1
else { fail("the page did not render the standby screen") }
guard number("document.querySelectorAll('[data-role=\"standby-row\"]').length") == 0
else { fail("rows showed before any snapshot") }
guard text("document.querySelector('[data-role=\"standby-freshness\"]').dataset.live") == "false"
else { fail("the freshness line must not claim live before a snapshot") }

// A snapshot arrives as the server writes it: atomically, by rename.
let directory = SnapshotLocation.directory()
let temporary = directory.appendingPathComponent("standby-snapshot.json.tmp")
let fixture = try! Data(contentsOf: URL(fileURLWithPath: arguments[3]))
try! fixture.write(to: temporary)
_ = try! FileManager.default.replaceItemAt(SnapshotLocation.file(), withItemAt: temporary)
guard waitFor(10, { number("document.querySelectorAll('[data-role=\"standby-row\"]').length") == 2 })
else { fail("the snapshot written by rename never reached the page") }
guard number("document.querySelectorAll('[data-role=\"standby-alert\"]').length") == 2
else { fail("the stalled and waiting stores did not raise alerts") }
guard text("document.querySelector('[data-role=\"standby-freshness\"]').dataset.live") == "true"
else { fail("a fresh snapshot must read as live") }
guard text("document.querySelector('[data-role=\"standby-scope\"]').textContent") == "All projects"
else { fail("the scope label is not the standby one") }

// Text that is not JSON never reaches the page, and valid JSON that is not a snapshot leaves the last good one.
try! Data("alert(1)".utf8).write(to: SnapshotLocation.file())
RunLoop.main.run(until: Date().addingTimeInterval(0.5))
try! Data(#"{"unrelated":true}"#.utf8).write(to: SnapshotLocation.file())
RunLoop.main.run(until: Date().addingTimeInterval(0.5))
guard number("document.querySelectorAll('[data-role=\"standby-row\"]').length") == 2
else { fail("an invalid snapshot replaced the last good one") }

// The screen fills the view it is given, wide to narrow to wide. A window cannot outgrow its display, so the
// widest size is the display's own.
let display = NSScreen.main?.visibleFrame.size ?? NSSize(width: 1440, height: 900)
for size in [display, NSSize(width: 800, height: 500), NSSize(width: 1280, height: 800)] {
    window.setContentSize(size)
    _ = waitFor(2) { false }
    let width = Int(saver.bounds.width), height = Int(saver.bounds.height)
    guard width > 0, height > 0 else { fail("the saver view has no size at \(Int(size.width)) points") }
    guard number("Math.round(document.querySelector('.standby-screen').getBoundingClientRect().width)") == width,
        number("Math.round(document.querySelector('.standby-screen').getBoundingClientRect().height)") == height
    else {
        let actual = text("(r=>r.width+'x'+r.height)(document.querySelector('.standby-screen').getBoundingClientRect())") ?? "?"
        fail("the standby screen is \(actual), not the \(width)x\(height) view")
    }
    // Every region, down to the last alert callout, project row, and the freshness line, lies inside the
    // view: a short display must not clip them under the page's hidden overflow (HS2-D9F3CA).
    let clipped = text("""
        (() => { const screen = document.querySelector('.standby-screen').getBoundingClientRect();
          return [...document.querySelectorAll('.standby-screen__top, .standby-screen__gauge, .standby-screen__count, \
        [data-role="standby-alert"], [data-role="standby-row"], [data-role="standby-freshness"]')]
            .filter(node => { const r = node.getBoundingClientRect();
              return r.height === 0 || r.top < screen.top || r.left < screen.left || r.bottom > screen.bottom || r.right > screen.right; })
            .map(node => (node.dataset.role || node.className) + '@' + Math.round(node.getBoundingClientRect().bottom)).join(' '); })()
        """) ?? "?"
    guard clipped.isEmpty else { fail("regions fall outside the \(width)x\(height) view: \(clipped)") }
    capture("\(width)x\(height)")
}
saver.stopAnimation()
print("OK")
exit(0)
