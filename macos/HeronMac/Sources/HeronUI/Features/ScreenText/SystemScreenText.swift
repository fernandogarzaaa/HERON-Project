import Foundation
import CoreGraphics
import ScreenCaptureKit
import Vision

/// macOS's own window picker, `SCContentSharingPicker`, limited to one window. The owner chooses; Heron never
/// lists or enumerates windows itself.
public final class SystemWindowPicker: NSObject, WindowPicking, SCContentSharingPickerObserver, @unchecked Sendable {
    private let lock = NSLock()
    private var pending: CheckedContinuation<PickedWindow?, Error>?

    public override init() { super.init() }

    /// One window, and no switching to a display or an app once picked.
    public static func configuration() -> SCContentSharingPickerConfiguration {
        var configuration = SCContentSharingPickerConfiguration()
        configuration.allowedPickerModes = [.singleWindow]
        configuration.allowsChangingSelectedContent = false
        return configuration
    }

    public func pickWindow() async throws -> PickedWindow? {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<PickedWindow?, Error>) in
            let busy: Bool = lock.withLock {
                if pending != nil { return true }
                pending = continuation
                return false
            }
            if busy { continuation.resume(returning: nil); return }
            DispatchQueue.main.async {
                let picker = SCContentSharingPicker.shared
                picker.add(self)
                picker.defaultConfiguration = Self.configuration()
                picker.isActive = true
                picker.present()
            }
        }
    }

    private func finish(_ result: Result<PickedWindow?, Error>) {
        let continuation: CheckedContinuation<PickedWindow?, Error>? = lock.withLock {
            let held = pending
            pending = nil
            return held
        }
        DispatchQueue.main.async {
            let picker = SCContentSharingPicker.shared
            picker.remove(self)
            picker.isActive = false
        }
        continuation?.resume(with: result)
    }

    // MARK: SCContentSharingPickerObserver

    public func contentSharingPicker(_ picker: SCContentSharingPicker, didCancelFor stream: SCStream?) {
        finish(.success(nil))
    }

    public func contentSharingPicker(_ picker: SCContentSharingPicker, didUpdateWith filter: SCContentFilter, for stream: SCStream?) {
        finish(.success(PickedWindow(appName: Self.appName(of: filter), filter: filter)))
    }

    public func contentSharingPickerStartDidFailWithError(_ error: Error) {
        finish(.failure(error))
    }

    /// The owning app of the picked window, for the chip's "From <app>" label.
    static func appName(of filter: SCContentFilter) -> String {
        if #available(macOS 15.2, *) {
            if let name = filter.includedWindows.first?.owningApplication?.applicationName, !name.isEmpty { return name }
            if let name = filter.includedApplications.first?.applicationName, !name.isEmpty { return name }
        }
        return "a window"
    }
}

/// The real permission check and request.
public struct SystemScreenRecordingAccess: ScreenRecordingAccessing {
    public init() {}
    public func isGranted() -> Bool { CGPreflightScreenCaptureAccess() }
    public func requestAccess() -> Bool { CGRequestScreenCaptureAccess() }
}

/// One ScreenCaptureKit screenshot of the picked window.
public struct ScreenCaptureWindowCapturer: WindowCapturing {
    public init() {}

    public func capture(_ window: PickedWindow) async throws -> CapturedImage {
        guard let filter = window.filter else { throw ScreenTextError.failed("No window was picked.") }
        let configuration = SCStreamConfiguration()
        let scale = CGFloat(filter.pointPixelScale)
        configuration.width = max(1, Int(filter.contentRect.width * scale))
        configuration.height = max(1, Int(filter.contentRect.height * scale))
        configuration.showsCursor = false
        do {
            let image = try await SCScreenshotManager.captureImage(contentFilter: filter, configuration: configuration)
            return CapturedImage(image)
        } catch {
            if (error as NSError).code == SCStreamError.Code.userDeclined.rawValue { throw ScreenTextError.permissionNeeded }
            throw error
        }
    }
}

/// Vision's text recognition, run on the Mac. Nothing leaves the machine here.
public struct VisionTextRecognizer: TextRecognizing {
    public init() {}

    public func recognizeText(in image: CapturedImage) async throws -> String {
        let cgImage = image.cgImage
        return try await Task.detached(priority: .userInitiated) {
            let request = VNRecognizeTextRequest()
            request.recognitionLevel = .accurate
            request.usesLanguageCorrection = true
            let handler = VNImageRequestHandler(cgImage: cgImage, options: [:])
            try handler.perform([request])
            let lines = (request.results ?? []).compactMap { $0.topCandidates(1).first?.string }
            return lines.joined(separator: "\n")
        }.value
    }
}
