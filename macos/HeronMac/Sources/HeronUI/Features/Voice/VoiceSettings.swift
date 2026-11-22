import Foundation

/// The app setting behind F7's row ("Speak replies") in Settings › Features. On once the feature is built (QF3),
/// so it is registered as a default, the same way as `QuickEntrySettings`.
public enum VoiceSettings {
    public static let defaultsKey = "heron.voice.speakReplies"

    public static func registerDefaults(_ defaults: UserDefaults = .standard) {
        defaults.register(defaults: [defaultsKey: true])
    }

    /// On unless the owner stored `false`.
    public static func isEnabled(_ defaults: UserDefaults = .standard) -> Bool {
        (defaults.object(forKey: defaultsKey) as? Bool) ?? true
    }
}
