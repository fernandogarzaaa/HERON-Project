import SwiftUI
import AppKit

// MARK: Preference values (UserDefaults keys `avatarStyle`, `theme`, `accentColor`)

public enum ThemeName: String, CaseIterable, Sendable, Identifiable {
    case dusk, shore, graphite
    public var id: String { rawValue }
    public var title: String { rawValue.capitalized }
}

public enum AccentName: String, CaseIterable, Sendable, Identifiable {
    case system, slate, lagoon, reed, rose
    public var id: String { rawValue }
    public var title: String { rawValue.capitalized }
}

public enum AppearanceChoice: String, CaseIterable, Sendable, Identifiable {
    case system, light, dark
    public var id: String { rawValue }
    public var title: String { rawValue.capitalized }
    public var colorScheme: ColorScheme? {
        switch self { case .system: nil; case .light: .light; case .dark: .dark }
    }
}

public enum AvatarStyle: String, CaseIterable, Sendable, Identifiable {
    case ripple, heron, monogram, image
    public var id: String { rawValue }
    public var title: String { rawValue.capitalized }
}

/// Settings › Appearance › Text size (S, M, L, XL).
public enum TextSizeChoice: String, CaseIterable, Sendable, Identifiable {
    case s, m, l, xl
    public var id: String { rawValue }
    public var title: String { rawValue.uppercased() }
    /// Multiplier for the reading font, which scales explicitly in case `dynamicTypeSize` does nothing on macOS.
    public var scale: CGFloat {
        switch self { case .s: 0.92; case .m: 1.0; case .l: 1.12; case .xl: 1.26 }
    }
    public var dynamicTypeSize: DynamicTypeSize {
        switch self { case .s: .small; case .m: .large; case .l: .xLarge; case .xl: .xxLarge }
    }
    /// One step up, for ⌘+ in a conversation. Stops at XL.
    public func larger() -> TextSizeChoice {
        let all = Self.allCases
        return all[min(all.count - 1, all.firstIndex(of: self)! + 1)]
    }
    /// One step down, for ⌘−. Stops at S.
    public func smaller() -> TextSizeChoice {
        let all = Self.allCases
        return all[max(0, all.firstIndex(of: self)! - 1)]
    }
}

// MARK: Spacing, radii, motion

public enum Spacing {
    public static let s1: CGFloat = 4, s2: CGFloat = 8, s3: CGFloat = 12, s4: CGFloat = 16
    public static let s5: CGFloat = 20, s6: CGFloat = 24, s8: CGFloat = 32, s12: CGFloat = 48
    public static let chatColumn: CGFloat = 680
}

public enum Radius {
    public static let tag: CGFloat = 4, control: CGFloat = 6, card: CGFloat = 10, sheet: CGFloat = 14
}

public enum Motion {
    public static let fast: Animation = .easeOut(duration: 0.12)
    public static let std: Animation = .timingCurve(0.2, 0.8, 0.2, 1, duration: 0.22)
    public static let slow: Animation = .timingCurve(0.2, 0.8, 0.2, 1, duration: 0.36)
    /// The 120 ms crossfade every state change uses under Reduce Motion.
    public static let crossfade: Animation = .easeOut(duration: 0.12)

    public static func animation(_ base: Animation, reduceMotion: Bool) -> Animation { reduceMotion ? crossfade : base }
}

// MARK: Colour

public extension Color {
    /// sRGB from `0xRRGGBB`.
    init(hex: UInt32) {
        self.init(.sRGB, red: Double((hex >> 16) & 0xFF) / 255, green: Double((hex >> 8) & 0xFF) / 255,
                  blue: Double(hex & 0xFF) / 255, opacity: 1)
    }
}

/// Where `accentColor == system` gets its colour.
public enum SystemAccent: Sendable, Equatable {
    /// The mockups' default blue, from tokens.json. Deterministic: used by snapshots and tests.
    case palette
    /// The live macOS accent. `reserved` is true for graphite, gold and purple, which read as "waiting"
    /// or "outside content" or lose selection, so the theme falls back to `slate`.
    case live(reserved: Bool)

    @MainActor public static func current() -> SystemAccent {
        let c = NSColor.controlAccentColor.usingColorSpace(.sRGB) ?? .systemBlue
        var h: CGFloat = 0, s: CGFloat = 0, b: CGFloat = 0, a: CGFloat = 0
        c.getHue(&h, saturation: &s, brightness: &b, alpha: &a)
        let hue = h * 360
        let graphite = s < 0.15
        let gold = hue >= 35 && hue <= 70
        let purple = hue >= 250 && hue <= 310
        return .live(reserved: graphite || gold || purple)
    }
}

/// Every colour of one theme, in one mode, with one accent. Built from the tokens in `GeneratedTokens`.
public struct HeronTheme: Equatable, Sendable {
    public var name: ThemeName
    public var accentName: AccentName
    public var isDark: Bool
    public var increasedContrast: Bool

    public var window, sidebar, surface, raised, hairline, ink, ink2, ink3: Color
    public var accent, accentSoft, onAccent: Color
    public var waiting, waitingSoft, waitingMark, outside, outsideSoft, danger, dangerSoft, ok, okSoft: Color

    /// Width of the border drawn on cards and sheets: 1 pt `ink` under Increase Contrast, else the hairline.
    public var cardBorder: Color { increasedContrast ? ink : hairline }
    public var cardBorderWidth: CGFloat { 1 }

    public static func resolve(theme: ThemeName, accent: AccentName, isDark: Bool,
                               increasedContrast: Bool = false, systemAccent: SystemAccent = .palette) -> HeronTheme {
        let mode = isDark ? "dark" : "light"
        let n = GeneratedTokens.neutrals[theme.rawValue]![mode]!
        func role(_ name: String) -> UInt32 { n[GeneratedTokens.neutralRoles.firstIndex(of: name)!] }
        let st = GeneratedTokens.status[mode]!

        var accentKey = accent.rawValue
        var accentIsLive = false
        if accent == .system, case .live(let reserved) = systemAccent {
            if reserved { accentKey = "slate" } else { accentIsLive = true }
        }
        let a = GeneratedTokens.accents[accentKey]!
        let accentColor = accentIsLive ? Color.accentColor : Color(hex: a[mode]!)

        let ink = Color(hex: role("ink"))
        let hair = Color(hex: role("hairline"))
        return HeronTheme(
            name: theme, accentName: accent, isDark: isDark, increasedContrast: increasedContrast,
            window: Color(hex: role("window")), sidebar: Color(hex: role("sidebar")), surface: Color(hex: role("surface")),
            raised: Color(hex: role("raised")),
            // Increase Contrast: hairlines get stronger and ink2/ink3 become ink.
            hairline: increasedContrast ? ink.opacity(0.4) : hair,
            ink: ink,
            ink2: increasedContrast ? ink : Color(hex: role("ink2")),
            ink3: increasedContrast ? ink : Color(hex: role("ink3")),
            accent: accentColor, accentSoft: Color(hex: a[isDark ? "darkSoft" : "lightSoft"]!),
            onAccent: Color(hex: a[isDark ? "onDark" : "onLight"]!),
            waiting: Color(hex: st["waiting"]!), waitingSoft: Color(hex: st["waitingSoft"]!), waitingMark: Color(hex: st["waitingMark"]!),
            outside: Color(hex: st["outside"]!), outsideSoft: Color(hex: st["outsideSoft"]!),
            danger: Color(hex: st["danger"]!), dangerSoft: Color(hex: st["dangerSoft"]!),
            ok: Color(hex: st["ok"]!), okSoft: Color(hex: st["okSoft"]!))
    }

    public static let `default` = HeronTheme.resolve(theme: .dusk, accent: .system, isDark: false)
}

private struct HeronThemeKey: EnvironmentKey { static let defaultValue = HeronTheme.default }
private struct TextScaleKey: EnvironmentKey { static let defaultValue: CGFloat = 1 }
/// Snapshot renders draw avatars still, because there is no clock.
private struct SnapshotModeKey: EnvironmentKey { static let defaultValue = false }
/// `ImageRenderer` can't render a `ScrollView`, so its snapshots draw scroll views flat. The overflow
/// snapshots render through an `NSHostingView` instead, which draws real scroll views, and leave this off.
private struct FlatScrollKey: EnvironmentKey { static let defaultValue = false }

public extension EnvironmentValues {
    var heronTheme: HeronTheme {
        get { self[HeronThemeKey.self] }
        set { self[HeronThemeKey.self] = newValue }
    }
    var heronTextScale: CGFloat {
        get { self[TextScaleKey.self] }
        set { self[TextScaleKey.self] = newValue }
    }
    var heronSnapshotMode: Bool {
        get { self[SnapshotModeKey.self] }
        set { self[SnapshotModeKey.self] = newValue }
    }
    var heronFlatScroll: Bool {
        get { self[FlatScrollKey.self] }
        set { self[FlatScrollKey.self] = newValue }
    }
}

/// Installs the theme, text size and accent from the preferences, following the system colour scheme
/// unless the user picked one.
public struct HeronThemed<Content: View>: View {
    let preferences: DisplayPreferences
    let content: Content
    @Environment(\.colorScheme) private var scheme
    @Environment(\.colorSchemeContrast) private var contrast

    public init(preferences: DisplayPreferences, @ViewBuilder content: () -> Content) {
        self.preferences = preferences
        self.content = content()
    }

    public var body: some View {
        let effective = preferences.appearance.colorScheme ?? scheme
        let theme = HeronTheme.resolve(theme: preferences.theme, accent: preferences.accentColor, isDark: effective == .dark,
                                       increasedContrast: contrast == .increased, systemAccent: SystemAccent.current())
        content
            .environment(\.heronTheme, theme)
            .environment(\.heronTextScale, preferences.textSize.scale)
            .dynamicTypeSize(preferences.textSize.dynamicTypeSize)
            .preferredColorScheme(preferences.appearance.colorScheme)
            .tint(theme.accent)
    }
}

// MARK: Type

/// The type tokens of heron-mac-visual.md. SF Pro through text styles; the serif (New York) only for the
/// Today greeting, first-run headings and empty-state titles.
public enum HeronFont {
    public static let large = Font.system(.largeTitle, design: .serif, weight: .semibold)
    public static let title = Font.system(.title, design: .serif, weight: .semibold)
    public static let title2 = Font.system(.title2, weight: .semibold)
    /// Empty-state titles are `title2` in the serif.
    public static let emptyTitle = Font.system(.title2, design: .serif, weight: .semibold)
    public static let title3 = Font.system(.title3, weight: .semibold)
    public static let body = Font.body
    public static let callout = Font.callout
    public static let sub = Font.subheadline
    public static let subLabel = Font.subheadline.weight(.semibold)
    public static let foot = Font.footnote
    public static let mono = Font.body.monospaced()

    /// `reading`: 14 pt at the standard size, scaled by the Text size control.
    public static func reading(scale: CGFloat) -> Font { .system(size: 14 * scale) }
}
