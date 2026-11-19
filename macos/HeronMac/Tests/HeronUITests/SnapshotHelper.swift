import XCTest
import SwiftUI
import AppKit
@testable import HeronUI

/// A small in-package snapshot helper (heron-v1.md item 41). It renders a view with `ImageRenderer` at a
/// fixed size and scale and compares the pixels against a PNG in `__Snapshots__/`, with a 1% tolerance.
///
/// - A missing reference is recorded, and the test fails once, so the picture must be reviewed.
/// - References record the macOS major version in their name (`name.macos27.png`). If only references for
///   another major exist, the comparison is skipped.
enum Snapshot {
    static let scale: CGFloat = 1
    /// A pixel counts as different when any channel moves by more than this (anti-aliasing noise is smaller).
    static let channelTolerance = 10
    /// The share of pixels that may differ.
    static let pixelTolerance = 0.01

    static var directory: URL {
        URL(fileURLWithPath: #filePath).deletingLastPathComponent().appendingPathComponent("__Snapshots__", isDirectory: true)
    }

    static var macOSMajor: Int { ProcessInfo.processInfo.operatingSystemVersion.majorVersion }

    static func referenceURL(_ name: String, major: Int = macOSMajor) -> URL {
        directory.appendingPathComponent("\(name).macos\(major).png")
    }

    enum Outcome: Equatable {
        case matched(difference: Double)
        case recorded(URL)
        case skippedOtherMacOS
        case mismatch(difference: Double, sizeChanged: Bool)
    }

    /// Renders `view` in a light or dark Dusk theme with the default accent, and returns the outcome.
    @MainActor
    static func check<V: View>(_ view: V, name: String, size: CGSize, dark: Bool, record: Bool = false) throws -> Outcome {
        let theme = HeronTheme.resolve(theme: .dusk, accent: .system, isDark: dark)
        let content = view
            .environment(\.colorScheme, dark ? .dark : .light)
            .environment(\.heronTheme, theme)
            .environment(\.heronSnapshotMode, true)
            .environment(\.heronFlatScroll, true)
            .frame(width: size.width, height: size.height > 0 ? size.height : nil, alignment: .topLeading)
            .background(theme.window)
        let image = try render(content, width: size.width, height: size.height)
        return try compare(image, name: name, forceRecord: record)
    }

    /// Like `check`, but renders through an `NSHostingView` so scroll views are real: `ImageRenderer` can't draw
    /// them. Used by the overflow snapshots (heron-v1.md item 41b), which must show where content scrolls.
    /// The view is drawn at exactly `size`, light or dark, 1 pt = 1 pixel.
    @MainActor
    static func checkScrolling<V: View>(_ view: V, name: String, size: CGSize, dark: Bool) throws -> Outcome {
        let theme = HeronTheme.resolve(theme: .dusk, accent: .system, isDark: dark)
        let content = view
            .environment(\.colorScheme, dark ? .dark : .light)
            .environment(\.heronTheme, theme)
            .environment(\.heronSnapshotMode, true)
            .environment(\.heronFlatScroll, false)
            .frame(width: size.width, height: size.height, alignment: .topLeading)
            .background(theme.window)
        let image = try renderHosted(content, size: size, dark: dark)
        return try compare(image, name: name)
    }

    /// An `NSHostingView` for a view, sized to `size` and put in an off-screen window, so it lays out and draws.
    /// Keep the result alive while using the view: the window goes away with it.
    struct Hosted<V: View> {
        let view: NSHostingView<V>
        let window: NSWindow
    }

    @MainActor
    static func makeHosted<V: View>(_ content: V, size: CGSize, dark: Bool = false) -> Hosted<V> {
        _ = NSApplication.shared
        let hosting = NSHostingView(rootView: content)
        hosting.frame = CGRect(origin: .zero, size: size)
        let window = NSWindow(contentRect: hosting.frame, styleMask: [.borderless], backing: .buffered, defer: false)
        window.appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
        window.contentView = hosting
        return Hosted(view: hosting, window: window)
    }

    /// Measured heights (the risk box) and scroll views settle over a few turns of the run loop.
    @MainActor
    static func settle(_ hostedView: NSView) {
        for _ in 0..<8 {
            hostedView.layoutSubtreeIfNeeded()
            hostedView.displayIfNeeded()
            RunLoop.current.run(until: Date().addingTimeInterval(0.05))
        }
        hostedView.layoutSubtreeIfNeeded()
    }

    @MainActor
    static func renderHosted<V: View>(_ content: V, size: CGSize, dark: Bool) throws -> CGImage {
        let hosted = makeHosted(content, size: size, dark: dark)
        let hostedView = hosted.view
        settle(hostedView)
        defer { withExtendedLifetime(hosted) {} }
        let rep = try XCTUnwrap(hostedView.bitmapImageRepForCachingDisplay(in: hostedView.bounds), "the hosting view can't be drawn")
        hostedView.cacheDisplay(in: hostedView.bounds, to: rep)
        let drawn = try XCTUnwrap(rep.cgImage, "the hosting view drew nothing")
        // Whatever the screen's scale, keep 1 pt = 1 pixel, like `ImageRenderer` at scale 1.
        let w = Int(size.width), h = Int(size.height)
        let ctx = try XCTUnwrap(CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0,
                                          space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                          bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        ctx.interpolationQuality = .high
        ctx.draw(drawn, in: CGRect(x: 0, y: 0, width: w, height: h))
        return try XCTUnwrap(ctx.makeImage())
    }

    @MainActor
    static func render<V: View>(_ view: V, width: CGFloat, height: CGFloat) throws -> CGImage {
        let renderer = ImageRenderer(content: view)
        renderer.scale = scale
        renderer.proposedSize = ProposedViewSize(width: width, height: height > 0 ? height : nil)
        return try XCTUnwrap(renderer.cgImage, "the view did not render")
    }

    static func compare(_ image: CGImage, name: String, forceRecord: Bool = false) throws -> Outcome {
        let fm = FileManager.default
        let url = referenceURL(name)
        if fm.fileExists(atPath: url.path) && !forceRecord {
            let reference = try loadPNG(url)
            let d = difference(image, reference)
            if d.ratio <= pixelTolerance && !d.sizeChanged { return .matched(difference: d.ratio) }
            // Keep the failing picture outside the package so it is never committed.
            let actual = URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent("\(name).macos\(macOSMajor).actual.png")
            try? pngData(image).write(to: actual)
            return .mismatch(difference: d.ratio, sizeChanged: d.sizeChanged)
        }
        let others = (try? fm.contentsOfDirectory(atPath: directory.path))?.contains { $0.hasPrefix("\(name).macos") && $0.hasSuffix(".png") } ?? false
        if others && !forceRecord { return .skippedOtherMacOS }
        try fm.createDirectory(at: directory, withIntermediateDirectories: true)
        try pngData(image).write(to: url)
        return .recorded(url)
    }

    /// Fraction of pixels that differ by more than `channelTolerance`. Different sizes count as fully different.
    static func difference(_ a: CGImage, _ b: CGImage) -> (ratio: Double, sizeChanged: Bool) {
        guard a.width == b.width, a.height == b.height else { return (1, true) }
        let pa = pixels(a), pb = pixels(b)
        var differing = 0
        var i = 0
        while i < pa.count {
            var moved = false
            for c in 0..<4 where abs(Int(pa[i + c]) - Int(pb[i + c])) > channelTolerance { moved = true }
            if moved { differing += 1 }
            i += 4
        }
        let total = max(1, a.width * a.height)
        return (Double(differing) / Double(total), false)
    }

    /// RGBA bytes in sRGB, premultiplied, so two images compare the same however they were encoded.
    static func pixels(_ image: CGImage) -> [UInt8] {
        let w = image.width, h = image.height
        var data = [UInt8](repeating: 0, count: w * h * 4)
        data.withUnsafeMutableBytes { raw in
            let ctx = CGContext(data: raw.baseAddress, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                                space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
            ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        }
        return data
    }

    static func pngData(_ image: CGImage) throws -> Data {
        try XCTUnwrap(NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]))
    }

    static func loadPNG(_ url: URL) throws -> CGImage {
        let data = try Data(contentsOf: url)
        let source = try XCTUnwrap(CGImageSourceCreateWithData(data as CFData, nil))
        return try XCTUnwrap(CGImageSourceCreateImageAtIndex(source, 0, nil))
    }
}

extension XCTestCase {
    /// Asserts a snapshot, following the record-and-fail-once rule.
    @MainActor
    func assertSnapshot<V: View>(_ view: V, name: String, size: CGSize, dark: Bool, file: StaticString = #filePath, line: UInt = #line) throws {
        try reportSnapshot(try Snapshot.check(view, name: name, size: size, dark: dark), name: name, file: file, line: line)
    }

    /// Asserts a snapshot rendered with real scroll views (`Snapshot.checkScrolling`), with the same rules.
    @MainActor
    func assertScrollingSnapshot<V: View>(_ view: V, name: String, size: CGSize, dark: Bool = false, file: StaticString = #filePath, line: UInt = #line) throws {
        try reportSnapshot(try Snapshot.checkScrolling(view, name: name, size: size, dark: dark), name: name, file: file, line: line)
    }

    private func reportSnapshot(_ outcome: Snapshot.Outcome, name: String, file: StaticString, line: UInt) throws {
        switch outcome {
        case .matched:
            break
        case .recorded(let url):
            XCTFail("Recorded a new reference at \(url.lastPathComponent). Review the picture, then run the tests again.", file: file, line: line)
        case .skippedOtherMacOS:
            throw XCTSkip("References for \(name) were recorded on another macOS major version; this is macOS \(Snapshot.macOSMajor).")
        case .mismatch(let d, let sizeChanged):
            XCTFail(sizeChanged ? "\(name): the image size changed" : "\(name): \(String(format: "%.2f", d * 100))% of pixels differ (limit 1%). The new picture is in \(NSTemporaryDirectory()).",
                    file: file, line: line)
        }
    }
}
