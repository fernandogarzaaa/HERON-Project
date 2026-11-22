import Foundation

/// The rows of Settings › Features. B4 adds none: each feature's `a` unit appends its own.
public enum FeatureRows {
    public static let all: [FeatureRow] = [
        // F1: the panel, its hotkey, and the Services item. The "Ask Heron" App Intent is deferred to v1.1 and has no row. No macOS permission (TCC: none).
        FeatureRow(key: "quick_entry", title: "Quick entry", kind: .app(defaultsKey: QuickEntrySettings.defaultsKey)),
        // F9: the "Look at a window" button in F1's panel. TCC: Screen Recording. The row names it and its pane but registers
        // no request: the panel explains it and asks on first use (QF3), then reports a refusal back so the row shows it.
        FeatureRow(key: "screen_text", title: "Look at a window", kind: .app(defaultsKey: ScreenTextSettings.defaultsKey),
                   permission: ScreenTextSettings.permission, settingsPane: ScreenRecordingPane.url),
    ]
}
