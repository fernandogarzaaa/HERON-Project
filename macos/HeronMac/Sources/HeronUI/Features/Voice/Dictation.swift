import AppKit
import Observation
import SwiftUI

/// The microphone button's logic. Heron never touches the microphone: the button sends macOS's own "Start
/// Dictation" action (`startDictation:`) to the field being typed in, and macOS does the rest.
///
/// UNVERIFIED (heron-v1-features.md, F7): whether a text field's editor answers that action when it is sent from a
/// button. The answer is read from the responder itself (`sendAction` returns false when nothing handled it), so
/// when it doesn't, the button shows the owner's dictation shortcut instead of doing nothing.
@MainActor
@Observable
public final class DictationButtonModel {
    /// Sends `startDictation:` to the focused field. True when the field handled it.
    @ObservationIgnored private let send: @MainActor () -> Bool
    @ObservationIgnored private let shortcut: @MainActor () -> String

    /// The line shown when the button couldn't start dictation: "Press … to dictate."
    public private(set) var hint: String?

    public init(send: @escaping @MainActor () -> Bool = DictationButtonModel.sendStartDictation,
                shortcut: @escaping @MainActor () -> String = { DictationShortcut.current() }) {
        self.send = send
        self.shortcut = shortcut
    }

    /// Starts dictation, or falls back to the shortcut hint.
    public func start() {
        hint = send() ? nil : "Press \(shortcut()) to dictate."
    }

    public func clearHint() { hint = nil }

    /// Sends `startDictation:` to the key window's text field editor, the object macOS dictation types into.
    /// Nothing is sent when no text field has the focus.
    public static func sendStartDictation() -> Bool {
        guard let app = NSApp, let responder = app.keyWindow?.firstResponder, responder is NSText else { return false }
        return app.sendAction(Selector(("startDictation:")), to: responder, from: nil)
    }
}

/// The owner's dictation shortcut, in words. macOS keeps it in System Settings › Keyboard › Dictation, and it is
/// "Press 🎤 / fn twice / Control twice / a custom key" depending on the Mac. Heron doesn't read it from the
/// system preferences (their format isn't documented), so the hint points to where it is set.
public enum DictationShortcut {
    public static func current() -> String {
        "your dictation shortcut (set in System Settings › Keyboard › Dictation)"
    }
}

/// A microphone button for a text field. When dictation can't be started it shows the shortcut hint beside itself
/// for a few seconds.
struct DictationButton: View {
    @State private var model = DictationButtonModel()
    @Environment(\.heronTheme) private var theme

    var body: some View {
        HStack(spacing: Spacing.s2) {
            if let hint = model.hint {
                Text(hint).font(HeronFont.foot).foregroundStyle(theme.ink3).lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)
                    .transition(.opacity)
                    .task(id: hint) {
                        try? await Task.sleep(for: .seconds(8))
                        model.clearHint()
                    }
            }
            Button { model.start() } label: { Image(systemName: "mic").font(.title3) }
                .buttonStyle(.plain).foregroundStyle(theme.ink2)
                .accessibilityLabel("Dictate")
                .accessibilityHint("Starts macOS dictation in the field you are typing in")
                .help("Dictate (macOS dictation)")
        }
    }
}
