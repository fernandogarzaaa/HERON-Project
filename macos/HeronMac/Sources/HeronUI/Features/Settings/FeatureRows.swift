import Foundation

/// The rows of Settings › Features. B4 added none: each feature's `a` unit appends its own.
public enum FeatureRows {
    public static let all: [FeatureRow] = [
        AskReasonsFeature.row,  // F2
    ]
}
