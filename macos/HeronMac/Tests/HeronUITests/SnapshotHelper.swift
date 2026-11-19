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
            .frame(width: size.width, height: size.height > 0 ? size.height : nil, alignment: .topLeading)
            .background(theme.window)
        let image = try render(content, width: size.width, height: size.height)
        return try compare(image, name: name, forceRecord: record)
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
        switch try Snapshot.check(view, name: name, size: size, dark: dark) {
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
