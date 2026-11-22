import Foundation
import Observation

/// Display preferences, kept in `UserDefaults` under `avatarStyle`, `theme` and `accentColor`
/// (heron-v1.md item 40), plus `appearance` and `textSize` from Settings › Appearance.
/// Unknown stored values fall back to the defaults in tokens.json.
@MainActor
@Observable
public final class DisplayPreferences {
    public enum Key {
        public static let avatarStyle = "avatarStyle"
        public static let theme = "theme"
        public static let accentColor = "accentColor"
        public static let appearance = "appearance"
        public static let textSize = "textSize"
        public static let showMenuBarExtra = "showMenuBarExtra"
    }

    @ObservationIgnored private let defaults: UserDefaults

    public var avatarStyle: AvatarStyle { didSet { defaults.set(avatarStyle.rawValue, forKey: Key.avatarStyle) } }
    public var theme: ThemeName { didSet { defaults.set(theme.rawValue, forKey: Key.theme) } }
    public var accentColor: AccentName { didSet { defaults.set(accentColor.rawValue, forKey: Key.accentColor) } }
    public var appearance: AppearanceChoice { didSet { defaults.set(appearance.rawValue, forKey: Key.appearance) } }
    public var textSize: TextSizeChoice { didSet { defaults.set(textSize.rawValue, forKey: Key.textSize) } }
    public var showMenuBarExtra: Bool { didSet { defaults.set(showMenuBarExtra, forKey: Key.showMenuBarExtra) } }

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        let d = GeneratedTokens.defaults
        avatarStyle = AvatarStyle(rawValue: defaults.string(forKey: Key.avatarStyle) ?? "") ?? AvatarStyle(rawValue: d["avatarStyle"]!)!
        theme = ThemeName(rawValue: defaults.string(forKey: Key.theme) ?? "") ?? ThemeName(rawValue: d["theme"]!)!
        accentColor = AccentName(rawValue: defaults.string(forKey: Key.accentColor) ?? "") ?? AccentName(rawValue: d["accentColor"]!)!
        appearance = AppearanceChoice(rawValue: defaults.string(forKey: Key.appearance) ?? "") ?? AppearanceChoice(rawValue: d["appearance"]!)!
        textSize = TextSizeChoice(rawValue: defaults.string(forKey: Key.textSize) ?? "") ?? .m
        showMenuBarExtra = defaults.object(forKey: Key.showMenuBarExtra) as? Bool ?? true
    }

    /// `~/Library/Application Support/Heron/avatar.png`, the picture for the `image` avatar style.
    public static var avatarImageURL: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Heron", isDirectory: true).appendingPathComponent("avatar.png")
    }
}
