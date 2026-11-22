import Foundation

/// One row of Settings › Features: an app setting held in `UserDefaults`, or a core key held in `config.toml`
/// and switched through `POST /api/features/{key}`. Each feature's `a` unit adds its own row to `FeatureRows.all`.
public struct FeatureRow: Identifiable, Equatable, Sendable {
    public enum Kind: Equatable, Sendable {
        /// An app setting, stored under this `UserDefaults` key.
        case app(defaultsKey: String)
        /// A core key from `GET /api/features`.
        case core(key: String)
    }

    public let key: String
    public let title: String
    public let kind: Kind
    /// The macOS permission the row needs, in the words the row shows ("Notifications"). `nil` when it needs none.
    /// The app asks for it when the owner turns the row on.
    public let permission: String?
    /// The System Settings pane where that permission is granted, shown when it is denied.
    public let settingsPane: URL?

    public var id: String { key }

    public init(key: String, title: String, kind: Kind, permission: String? = nil, settingsPane: URL? = nil) {
        self.key = key
        self.title = title
        self.kind = kind
        self.permission = permission
        self.settingsPane = settingsPane
    }

    /// The `permission` value that `FeaturesSettingsStore` asks for through its `NotificationPoster`.
    public static let notificationsPermission = "Notifications"
}
