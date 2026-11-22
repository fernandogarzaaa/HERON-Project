import Foundation

/// The rows of Settings › Features. B4 adds none: each feature's `a` unit appends its own.
public enum FeatureRows {
    public static let all: [FeatureRow] = [
        // F8: `draft_email` opens a filled-in compose window and never sends. It needs no macOS permission (TCC: none).
        FeatureRow(key: "email_drafts", title: "Email drafts", kind: .core(key: "email_drafts")),
    ]
}
