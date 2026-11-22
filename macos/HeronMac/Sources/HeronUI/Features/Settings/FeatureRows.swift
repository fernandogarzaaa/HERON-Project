import Foundation

/// The rows of Settings › Features. B4 adds none: each feature's `a` unit appends its own.
public enum FeatureRows {
    public static let all: [FeatureRow] = [
        // F1: the panel, its hotkey, and the Services item. The "Ask Heron" App Intent is deferred to v1.1 and has no row. No macOS permission (TCC: none).
        FeatureRow(key: "quick_entry", title: "Quick entry", kind: .app(defaultsKey: QuickEntrySettings.defaultsKey)),
        // F9: the "Look at a window" button in F1's panel. TCC: Screen Recording only if the window picker turns out to need it
        // (UNVERIFIED), and then the panel explains it before macOS asks, so the row names no permission.
        FeatureRow(key: "screen_text", title: "Look at a window", kind: .app(defaultsKey: ScreenTextSettings.defaultsKey)),
    ]
}
