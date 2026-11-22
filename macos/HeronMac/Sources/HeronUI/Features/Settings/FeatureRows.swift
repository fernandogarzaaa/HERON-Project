import Foundation

/// The rows of Settings › Features. B4 adds none: each feature's `a` unit appends its own.
public enum FeatureRows {
    public static let all: [FeatureRow] = [
        // F3: the Runs screen, the menu bar badge and the app's notices. On unless turned off (QF3).
        FeatureRow(key: "runs", title: "Runs and notifications", kind: .app(defaultsKey: RunsSetting.defaultsKey),
                   permission: FeatureRow.notificationsPermission,
                   settingsPane: URL(string: "x-apple.systempreferences:com.apple.Notifications-Settings.extension")),
    ]
}
