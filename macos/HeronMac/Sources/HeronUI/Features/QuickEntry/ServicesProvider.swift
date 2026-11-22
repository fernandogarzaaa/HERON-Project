import AppKit

/// The "Ask Heron" item of the Services menu (`NSServices` in Info.plist: `NSMessage` = `askHeron`).
///
/// macOS hands over the selected text on a pasteboard. The app that had the selection is still frontmost at
/// that moment, so its name is read here and not later. The provider only attaches the text to the panel as a
/// chip; the owner decides what to ask and presses Return.
@MainActor
public final class ServicesProvider: NSObject {
    private let frontmostAppName: @MainActor () -> String?
    private let isEnabled: @MainActor () -> Bool
    private let onSelection: @MainActor (_ text: String, _ app: String) -> Void

    /// The name shown when macOS can't say which app had the selection.
    public static let unknownApp = "another app"

    /// - Parameters:
    ///   - frontmostAppName: the source app's name; reads `NSWorkspace.shared.frontmostApplication` in the app.
    ///   - isEnabled: whether "Quick entry" is on in Settings › Features.
    ///   - onSelection: opens the panel with the text attached.
    public init(frontmostAppName: @escaping @MainActor () -> String? = { NSWorkspace.shared.frontmostApplication?.localizedName },
                isEnabled: @escaping @MainActor () -> Bool = { QuickEntrySettings.isEnabled() },
                onSelection: @escaping @MainActor (_ text: String, _ app: String) -> Void) {
        self.frontmostAppName = frontmostAppName
        self.isEnabled = isEnabled
        self.onSelection = onSelection
    }

    /// The Services entry point: `askHeron(_:userData:error:)`.
    @objc public func askHeron(_ pasteboard: NSPasteboard, userData: String, error: AutoreleasingUnsafeMutablePointer<NSString>) {
        guard isEnabled() else {
            error.pointee = "Quick entry is off. Turn it on in Heron's Settings › Features." as NSString
            return
        }
        guard let text = pasteboard.string(forType: .string),
              !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            error.pointee = "Heron got no text from that selection." as NSString
            return
        }
        let app = frontmostAppName().flatMap { $0.isEmpty ? nil : $0 } ?? Self.unknownApp
        onSelection(text, app)
    }

    /// Registers this provider with the app and refreshes the Services menu.
    public func register() {
        NSApplication.shared.servicesProvider = self
        NSUpdateDynamicServices()
    }
}
