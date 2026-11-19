import XCTest
import SwiftUI
import AppKit
@testable import HeronUI

/// `tokens.json` is the only source of colour values: the Swift theme must match it, and the contrast
/// table must hold (heron-mac-visual.md, Verification).
@MainActor
final class ThemeTokenTests: XCTestCase {
    private typealias Palette = [String: String]

    private var tokens: [String: Any] {
        get throws {
            let root = URL(fileURLWithPath: #filePath)
                .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
                .deletingLastPathComponent().deletingLastPathComponent()
            let url = root.appendingPathComponent("docs/designs/heron-mac-visual/tokens.json")
            let data = try Data(contentsOf: url)
            return try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
        }
    }

    private func hex(_ s: String) -> UInt32 { UInt32(s.dropFirst(), radix: 16)! }

    private func rgb(_ c: Color) throws -> (r: Int, g: Int, b: Int) {
        let n = try XCTUnwrap(NSColor(c).usingColorSpace(.sRGB))
        return (Int((n.redComponent * 255).rounded()), Int((n.greenComponent * 255).rounded()), Int((n.blueComponent * 255).rounded()))
    }

    private func assertColor(_ c: Color, _ expected: String, _ label: String, file: StaticString = #filePath, line: UInt = #line) throws {
        let v = hex(expected)
        let got = try rgb(c)
        XCTAssertEqual(got.r, Int((v >> 16) & 0xFF), "\(label) red", file: file, line: line)
        XCTAssertEqual(got.g, Int((v >> 8) & 0xFF), "\(label) green", file: file, line: line)
        XCTAssertEqual(got.b, Int(v & 0xFF), "\(label) blue", file: file, line: line)
    }

    // MARK: Swift values against tokens.json

    func testGeneratedTokensMatchTokensJSON() throws {
        let t = try tokens
        let themes = try XCTUnwrap(t["themes"] as? [String: [String: Palette]])
        XCTAssertEqual(Set(GeneratedTokens.neutrals.keys), Set(themes.keys))
        for (theme, modes) in themes {
            for (mode, palette) in modes {
                let swift = try XCTUnwrap(GeneratedTokens.neutrals[theme]?[mode], "\(theme) \(mode)")
                for (i, role) in GeneratedTokens.neutralRoles.enumerated() {
                    XCTAssertEqual(swift[i], hex(try XCTUnwrap(palette[role])), "\(theme) \(mode) \(role)")
                }
            }
        }
        let accents = try XCTUnwrap(t["accents"] as? [String: Palette])
        XCTAssertEqual(Set(GeneratedTokens.accents.keys), Set(accents.keys))
        for (name, values) in accents {
            for (key, value) in values { XCTAssertEqual(GeneratedTokens.accents[name]?[key], hex(value), "accent \(name) \(key)") }
            XCTAssertEqual(GeneratedTokens.accents[name]?.count, values.count)
        }
        let status = try XCTUnwrap(t["status"] as? [String: Palette])
        for (mode, values) in status {
            for (key, value) in values { XCTAssertEqual(GeneratedTokens.status[mode]?[key], hex(value), "status \(mode) \(key)") }
            XCTAssertEqual(GeneratedTokens.status[mode]?.count, values.count)
        }
        let defaults = try XCTUnwrap(t["defaults"] as? Palette)
        XCTAssertEqual(GeneratedTokens.defaults, defaults)
    }

    func testResolvedThemeColoursMatchTokensJSON() throws {
        let t = try tokens
        let themes = try XCTUnwrap(t["themes"] as? [String: [String: Palette]])
        let accents = try XCTUnwrap(t["accents"] as? [String: Palette])
        let status = try XCTUnwrap(t["status"] as? [String: Palette])
        for themeName in ThemeName.allCases {
            for dark in [false, true] {
                let mode = dark ? "dark" : "light"
                let p = try XCTUnwrap(themes[themeName.rawValue]?[mode])
                for accentName in AccentName.allCases {
                    let theme = HeronTheme.resolve(theme: themeName, accent: accentName, isDark: dark)
                    let label = "\(themeName) \(mode) \(accentName)"
                    try assertColor(theme.window, p["window"]!, "\(label) window")
                    try assertColor(theme.sidebar, p["sidebar"]!, "\(label) sidebar")
                    try assertColor(theme.surface, p["surface"]!, "\(label) surface")
                    try assertColor(theme.raised, p["raised"]!, "\(label) raised")
                    try assertColor(theme.hairline, p["hairline"]!, "\(label) hairline")
                    try assertColor(theme.ink, p["ink"]!, "\(label) ink")
                    try assertColor(theme.ink2, p["ink2"]!, "\(label) ink2")
                    try assertColor(theme.ink3, p["ink3"]!, "\(label) ink3")
                    let a = try XCTUnwrap(accents[accentName.rawValue])
                    try assertColor(theme.accent, a[mode]!, "\(label) accent")
                    try assertColor(theme.accentSoft, a[dark ? "darkSoft" : "lightSoft"]!, "\(label) accentSoft")
                    try assertColor(theme.onAccent, a[dark ? "onDark" : "onLight"]!, "\(label) onAccent")
                    let s = try XCTUnwrap(status[mode])
                    for (key, color) in [("waiting", theme.waiting), ("waitingSoft", theme.waitingSoft), ("waitingMark", theme.waitingMark),
                                         ("outside", theme.outside), ("outsideSoft", theme.outsideSoft), ("danger", theme.danger),
                                         ("dangerSoft", theme.dangerSoft), ("ok", theme.ok), ("okSoft", theme.okSoft)] {
                        try assertColor(color, s[key]!, "\(label) \(key)")
                    }
                }
            }
        }
    }

    func testDefaultsAreDuskSystemRipple() {
        XCTAssertEqual(GeneratedTokens.defaults["theme"], "dusk")
        XCTAssertEqual(GeneratedTokens.defaults["accentColor"], "system")
        XCTAssertEqual(GeneratedTokens.defaults["avatarStyle"], "ripple")
        XCTAssertEqual(GeneratedTokens.defaults["appearance"], "system")
    }

    // MARK: Reserved colours and the system accent

    func testReservedSystemAccentFallsBackToSlate() throws {
        let reserved = HeronTheme.resolve(theme: .dusk, accent: .system, isDark: false, systemAccent: .live(reserved: true))
        let slate = HeronTheme.resolve(theme: .dusk, accent: .slate, isDark: false)
        try assertColor(reserved.accent, "#2F6690", "fallback accent")
        XCTAssertEqual(try rgb(reserved.accentSoft).r, try rgb(slate.accentSoft).r)
        try assertColor(reserved.accentSoft, "#E2ECF4", "fallback soft")
        // A named accent ignores the system accent.
        let rose = HeronTheme.resolve(theme: .dusk, accent: .rose, isDark: false, systemAccent: .live(reserved: true))
        try assertColor(rose.accent, "#9C3D62", "rose")
    }

    func testNoAccentIsGoldOrViolet() throws {
        for accent in AccentName.allCases {
            for dark in [false, true] {
                let theme = HeronTheme.resolve(theme: .dusk, accent: accent, isDark: dark)
                let a = try rgb(theme.accent), w = try rgb(theme.waitingMark), o = try rgb(theme.outside)
                XCTAssertNotEqual([a.r, a.g, a.b], [w.r, w.g, w.b], "\(accent) must not match 'waiting'")
                XCTAssertNotEqual([a.r, a.g, a.b], [o.r, o.g, o.b], "\(accent) must not match 'outside content'")
            }
        }
    }

    func testIncreaseContrastSwapsSecondaryInkAndStrengthensBorders() throws {
        let normal = HeronTheme.resolve(theme: .dusk, accent: .system, isDark: false)
        let high = HeronTheme.resolve(theme: .dusk, accent: .system, isDark: false, increasedContrast: true)
        XCTAssertEqual(try rgb(high.ink2).r, try rgb(high.ink).r)
        XCTAssertEqual(try rgb(high.ink3).g, try rgb(high.ink).g)
        XCTAssertNotEqual(try rgb(normal.ink2).r, try rgb(normal.ink).r)
        XCTAssertEqual(try rgb(high.cardBorder).b, try rgb(high.ink).b, "cards and sheets get a 1 pt ink border")
        XCTAssertEqual(try rgb(normal.cardBorder).b, try rgb(normal.hairline).b)
        XCTAssertEqual(high.cardBorderWidth, 1)
        XCTAssertNotEqual(try rgb(high.hairline).r, try rgb(normal.hairline).r)
    }

    // MARK: Contrast (WCAG 2.1), recomputed from tokens.json

    private func luminance(_ h: UInt32) -> Double {
        func f(_ c: UInt32) -> Double { let s = Double(c) / 255; return s <= 0.03928 ? s / 12.92 : pow((s + 0.055) / 1.055, 2.4) }
        return 0.2126 * f((h >> 16) & 0xFF) + 0.7152 * f((h >> 8) & 0xFF) + 0.0722 * f(h & 0xFF)
    }

    private func contrast(_ a: String, _ b: String) -> Double {
        let x = luminance(hex(a)), y = luminance(hex(b))
        return (max(x, y) + 0.05) / (min(x, y) + 0.05)
    }

    func testContrastRatiosAreAtLeastAA() throws {
        XCTAssertEqual(contrast("#000000", "#FFFFFF"), 21, accuracy: 0.001, "the helper computes WCAG ratios")
        XCTAssertEqual(contrast("#777777", "#FFFFFF"), 4.48, accuracy: 0.01)
        let t = try tokens
        let themes = try XCTUnwrap(t["themes"] as? [String: [String: Palette]])
        let accents = try XCTUnwrap(t["accents"] as? [String: Palette])
        let status = try XCTUnwrap(t["status"] as? [String: Palette])
        for (theme, modes) in themes {
            for (mode, p) in modes {
                let grounds = ["window", "sidebar", "surface", "raised"].map { p[$0]! }
                for ground in grounds {
                    XCTAssertGreaterThanOrEqual(contrast(p["ink"]!, ground), 7, "\(theme) \(mode) ink on \(ground) passes AAA")
                    XCTAssertGreaterThanOrEqual(contrast(p["ink2"]!, ground), 4.5, "\(theme) \(mode) ink2 on \(ground)")
                    XCTAssertGreaterThanOrEqual(contrast(p["ink3"]!, ground), 4.5, "\(theme) \(mode) ink3 on \(ground)")
                    for (name, a) in accents {
                        XCTAssertGreaterThanOrEqual(contrast(a[mode]!, ground), 4.5, "\(theme) \(mode) accent \(name) on \(ground)")
                    }
                    let s = try XCTUnwrap(status[mode])
                    for key in ["waiting", "outside", "danger", "ok"] {
                        XCTAssertGreaterThanOrEqual(contrast(s[key]!, ground), 4.5, "\(theme) \(mode) \(key) text on \(ground)")
                    }
                }
            }
        }
        for (name, a) in accents {
            XCTAssertGreaterThanOrEqual(contrast(a["onLight"]!, a["light"]!), 4.5, "button text on \(name) light")
            XCTAssertGreaterThanOrEqual(contrast(a["onDark"]!, a["dark"]!), 4.5, "button text on \(name) dark")
            XCTAssertGreaterThanOrEqual(contrast(a["light"]!, a["lightSoft"]!), 4.5, "\(name) text on its soft fill, light")
            XCTAssertGreaterThanOrEqual(contrast(a["dark"]!, a["darkSoft"]!), 4.5, "\(name) text on its soft fill, dark")
        }
        for (mode, s) in status {
            for key in ["waiting", "outside", "danger", "ok"] {
                XCTAssertGreaterThanOrEqual(contrast(s[key]!, s["\(key)Soft"]!), 4.5, "\(mode) \(key) on its soft fill (tags)")
            }
        }
    }

    // MARK: Text size

    /// Settings › Appearance › Text size must work whether or not macOS scales text styles, so the reading
    /// font scales explicitly.
    func testReadingFontGrowsWithTheTextSizeControl() throws {
        func width(_ scale: CGFloat) throws -> Int {
            let view = Text("The quick brown fox jumps over the lazy dog").font(HeronFont.reading(scale: scale)).fixedSize()
            let renderer = ImageRenderer(content: view)
            renderer.scale = 1
            return try XCTUnwrap(renderer.cgImage).width
        }
        let widths = try TextSizeChoice.allCases.map { try width($0.scale) }
        XCTAssertEqual(widths, widths.sorted())
        XCTAssertEqual(Set(widths).count, widths.count, "every size renders at its own width")
        XCTAssertGreaterThan(Double(widths[3]) / Double(widths[1]), 1.2, "XL is at least 20% wider than M")
    }
}
