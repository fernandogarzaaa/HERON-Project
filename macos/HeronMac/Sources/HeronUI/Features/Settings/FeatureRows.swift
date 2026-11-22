import Foundation

/// The rows of Settings › Features. Each feature's `a` unit appends its own.
public enum FeatureRows {
    public static let all: [FeatureRow] = [
        ShortcutsFeature.row,  // F5
    ]
}
