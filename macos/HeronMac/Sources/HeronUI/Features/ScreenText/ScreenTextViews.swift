import SwiftUI
import AppKit
import CoreGraphics

/// The System Settings pane where Screen Recording is granted.
enum ScreenRecordingPane {
    static let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")!
}

/// F1's panel button for F9. It is the only thing in the app that starts a window capture.
struct LookAtWindowButton: View {
    let screenText: ScreenTextModel
    let action: () -> Void

    var body: some View {
        Button(screenText.isBusy ? "Looking…" : "Look at a window", action: action)
            .buttonStyle(HButtonStyle(.secondary))
            .disabled(screenText.isBusy)
            .accessibilityHint("Opens the macOS window picker. Heron reads the text of the window you pick, once.")
    }
}

/// What the panel says when the capture needs Screen Recording (explained before macOS asks) or when it failed.
struct ScreenTextNotice: View {
    let screenText: ScreenTextModel
    @Environment(\.heronTheme) private var theme

    var body: some View {
        switch screenText.status {
        case .needsScreenRecording:
            VStack(alignment: .leading, spacing: Spacing.s2) {
                Text("macOS needs your OK before Heron can read a window")
                    .font(HeronFont.callout.weight(.semibold)).foregroundStyle(theme.ink)
                Text("Heron captures only the one window you pick, once, and reads its text on this Mac. The picture is thrown away right after. It never records, and it only looks when you press the button. Allow Screen Recording, then press the button again.")
                    .font(HeronFont.callout).foregroundStyle(theme.ink2).fixedSize(horizontal: false, vertical: true)
                HStack(spacing: Spacing.s2) {
                    Button("Allow Screen Recording") {
                        screenText.allowScreenRecording()
                    }
                    .buttonStyle(HButtonStyle(.primary))
                    Button("Open System Settings") { NSWorkspace.shared.open(ScreenRecordingPane.url) }
                        .buttonStyle(HButtonStyle(.secondary))
                    Button("Not now") { screenText.dismissNotice() }
                        .buttonStyle(HButtonStyle(.link))
                }
            }
            .accessibilityElement(children: .contain)
        case .failed(let message):
            HStack(spacing: Spacing.s2) {
                Text(message).font(HeronFont.foot).foregroundStyle(theme.danger).lineLimit(2)
                Spacer(minLength: 0)
                Button(action: { screenText.dismissNotice() }) { Image(systemName: "xmark.circle.fill") }
                    .buttonStyle(.plain).foregroundStyle(theme.ink3)
                    .accessibilityLabel("Dismiss")
            }
        case .idle, .picking, .reading:
            EmptyView()
        }
    }
}
