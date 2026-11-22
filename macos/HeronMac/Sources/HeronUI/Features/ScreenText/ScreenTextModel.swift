import Foundation
import CoreGraphics
import Observation
import ScreenCaptureKit
import HeronCore

/// The app setting behind F9's row in Settings › Features. On once the feature is built (QF3), so it is registered
/// as a default: it reads `true` until the owner turns it off.
public enum ScreenTextSettings {
    public static let defaultsKey = "heron.screenText.enabled"

    public static func registerDefaults(_ defaults: UserDefaults = .standard) {
        defaults.register(defaults: [defaultsKey: true])
    }

    /// On unless the owner stored `false` (same reading as `QuickEntrySettings.isEnabled`).
    public static func isEnabled(_ defaults: UserDefaults = .standard) -> Bool {
        (defaults.object(forKey: defaultsKey) as? Bool) ?? true
    }
}

/// A window the owner chose in macOS's picker. `filter` is what ScreenCaptureKit captures; it is `nil` only in tests.
public struct PickedWindow: @unchecked Sendable {
    public var appName: String
    public var filter: SCContentFilter?

    public init(appName: String, filter: SCContentFilter? = nil) {
        self.appName = appName
        self.filter = filter
    }
}

/// One captured frame. A class so that "the image is discarded right after its text is read" can be checked:
/// once the last reference goes, the pixels go with it.
public final class CapturedImage: @unchecked Sendable {
    public let cgImage: CGImage
    public init(_ cgImage: CGImage) { self.cgImage = cgImage }
}

public enum ScreenTextError: Error, Equatable, Sendable {
    /// macOS refused the capture for lack of Screen Recording permission.
    case permissionNeeded
    /// The picked window had no readable text.
    case noText
    case failed(String)
}

/// Opens macOS's window picker. `nil` means the owner closed it without picking.
public protocol WindowPicking: Sendable {
    func pickWindow() async throws -> PickedWindow?
}

/// One ScreenCaptureKit capture of the picked window.
public protocol WindowCapturing: Sendable {
    func capture(_ window: PickedWindow) async throws -> CapturedImage
}

/// Reads the text in an image on the Mac.
public protocol TextRecognizing: Sendable {
    func recognizeText(in image: CapturedImage) async throws -> String
}

/// What "Look at a window" produced: text and the name of the app it came from. Never an image.
public struct ScreenTextResult: Equatable, Sendable {
    public var app: String
    public var text: String
}

/// F9's flow: pick one window, capture it once, read its text, drop the image.
///
/// Only `lookAtWindow()` starts it, and only the "Look at a window" button calls that. Nothing here runs on a timer,
/// in the background, or at the model's request. The text is handed back to the caller (the quick-entry panel), which
/// attaches it as a chip; nothing is sent from here.
@MainActor
@Observable
public final class ScreenTextModel {
    public enum Status: Equatable, Sendable {
        case idle
        /// The picker is open.
        case picking
        /// The window was picked and its text is being read.
        case reading
        /// macOS wants Screen Recording permission; the panel explains why before asking.
        case needsScreenRecording
        case failed(String)
    }

    public private(set) var status: Status = .idle

    @ObservationIgnored private let picker: any WindowPicking
    @ObservationIgnored private let capturer: any WindowCapturing
    @ObservationIgnored private let recognizer: any TextRecognizing
    @ObservationIgnored private let defaults: UserDefaults

    public init(picker: any WindowPicking = SystemWindowPicker(),
                capturer: any WindowCapturing = ScreenCaptureWindowCapturer(),
                recognizer: any TextRecognizing = VisionTextRecognizer(),
                defaults: UserDefaults = .standard) {
        self.picker = picker
        self.capturer = capturer
        self.recognizer = recognizer
        self.defaults = defaults
    }

    public var isEnabled: Bool { ScreenTextSettings.isEnabled(defaults) }
    public var isBusy: Bool { status == .picking || status == .reading }

    /// Opens the picker, captures the chosen window once and reads its text. Returns `nil` when the feature is off,
    /// the owner cancelled, or nothing could be read (`status` says which).
    public func lookAtWindow() async -> ScreenTextResult? {
        guard isEnabled, !isBusy else { return nil }
        status = .picking
        do {
            guard let window = try await picker.pickWindow() else {
                status = .idle
                return nil
            }
            status = .reading
            let text = try await readText(of: window)
            status = .idle
            return ScreenTextResult(app: window.appName, text: text)
        } catch ScreenTextError.permissionNeeded {
            status = .needsScreenRecording
            return nil
        } catch ScreenTextError.noText {
            status = .failed("Heron found no text in that window.")
            return nil
        } catch {
            status = .failed(describe(error))
            return nil
        }
    }

    /// The image lives only inside this function: it is captured, read, and goes out of scope before the text is returned.
    private func readText(of window: PickedWindow) async throws -> String {
        let text: String
        do {
            let image = try await capturer.capture(window)
            text = try await recognizer.recognizeText(in: image)
        }
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { throw ScreenTextError.noText }
        // The core refuses a context item over 20,000 characters (`MAX_CONTEXT_CHARS`), so a long window is cut here.
        return String(trimmed.prefix(Self.maxCharacters))
    }

    /// Mirrors the core's per-item limit on chat context text.
    public static let maxCharacters = 20_000

    /// Clears a shown explanation or error (the owner dismissed it).
    public func dismissNotice() {
        if !isBusy { status = .idle }
    }
}
