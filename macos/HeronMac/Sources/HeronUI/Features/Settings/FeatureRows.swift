import Foundation

/// The rows of Settings › Features. B4 adds none: each feature's `a` unit appends its own.
public enum FeatureRows {
    public static let all: [FeatureRow] = [
        // F1: the panel, its hotkey, the Services item and the App Intent. No macOS permission (TCC: none).
        FeatureRow(key: "quick_entry", title: "Quick entry", kind: .app(defaultsKey: QuickEntrySettings.defaultsKey)),
    ]
}
