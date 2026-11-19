import SwiftUI
import AppKit
import HeronCore

// MARK: Surfaces

public extension View {
    /// A card: `surface`, `card` radius, a hairline border (1 pt `ink` under Increase Contrast).
    func heronCard(padding: CGFloat = Spacing.s4) -> some View { modifier(HeronCard(padding: padding)) }
}

private struct HeronCard: ViewModifier {
    let padding: CGFloat
    @Environment(\.heronTheme) private var theme
    func body(content: Content) -> some View {
        content
            .padding(padding)
            .background(RoundedRectangle(cornerRadius: Radius.card, style: .continuous).fill(theme.surface))
            .overlay(RoundedRectangle(cornerRadius: Radius.card, style: .continuous).stroke(theme.cardBorder, lineWidth: theme.cardBorderWidth))
    }
}

/// A `ScrollView` that draws flat in snapshot renders, which can't draw scroll views.
struct HScroll<Content: View>: View {
    var axes: Axis.Set = .vertical
    var maxHeight: CGFloat?
    @ViewBuilder var content: Content
    @Environment(\.heronSnapshotMode) private var snapshot

    var body: some View {
        if snapshot {
            content.frame(maxWidth: .infinity, maxHeight: maxHeight, alignment: .topLeading).clipped()
        } else {
            ScrollView(axes) { content.frame(maxWidth: .infinity, alignment: .topLeading) }
                .frame(maxHeight: maxHeight)
        }
    }
}

// MARK: Buttons

public enum HButtonKind: Sendable { case primary, secondary, danger, link }

public struct HButtonStyle: ButtonStyle {
    let kind: HButtonKind
    public init(_ kind: HButtonKind = .secondary) { self.kind = kind }

    public func makeBody(configuration: Configuration) -> some View {
        StyledLabel(configuration: configuration, kind: kind)
    }

    private struct StyledLabel: View {
        let configuration: ButtonStyle.Configuration
        let kind: HButtonKind
        @Environment(\.heronTheme) private var theme
        @Environment(\.isEnabled) private var enabled

        var body: some View {
            let pressed = configuration.isPressed
            configuration.label
                .font(.body.weight(kind == .link ? .regular : .medium))
                .foregroundStyle(foreground)
                .padding(.horizontal, kind == .link ? 0 : Spacing.s3)
                .padding(.vertical, kind == .link ? 0 : 5)
                .background {
                    if kind != .link {
                        RoundedRectangle(cornerRadius: Radius.control, style: .continuous).fill(fill.opacity(pressed ? 0.85 : 1))
                    }
                }
                .overlay {
                    if kind == .secondary {
                        RoundedRectangle(cornerRadius: Radius.control, style: .continuous).stroke(theme.hairline, lineWidth: 1)
                    }
                }
                .opacity(enabled ? 1 : 0.45)
                .animation(Motion.fast, value: pressed)
                .contentShape(Rectangle())
        }

        private var fill: Color {
            switch kind {
            case .primary: theme.accent
            case .danger: theme.danger
            case .secondary: theme.raised
            case .link: .clear
            }
        }

        private var foreground: Color {
            switch kind {
            case .primary: theme.onAccent
            case .danger: theme.isDark ? Color.black : Color.white
            case .secondary: theme.ink
            case .link: theme.accent
            }
        }
    }
}

// MARK: Tags, labels

public enum HTagTone: Sendable { case neutral, ok, danger, waiting, outside, accent }

/// A small status tag: coloured fill, readable text, optional SF Symbol. Colour is never the only signal.
public struct HTag: View {
    let text: String
    let tone: HTagTone
    let symbol: String?
    @Environment(\.heronTheme) private var theme

    public init(_ text: String, tone: HTagTone = .neutral, symbol: String? = nil) {
        self.text = text
        self.tone = tone
        self.symbol = symbol
    }

    public var body: some View {
        HStack(spacing: 4) {
            if let symbol { Image(systemName: symbol).imageScale(.small) }
            Text(text)
        }
        .font(HeronFont.subLabel)
        .foregroundStyle(colors.fg)
        .padding(.horizontal, 7)
        .padding(.vertical, 2)
        .background(RoundedRectangle(cornerRadius: Radius.tag, style: .continuous).fill(colors.bg))
        .accessibilityElement(children: .combine)
    }

    private var colors: (fg: Color, bg: Color) {
        switch tone {
        case .neutral: (theme.ink2, theme.raised)
        case .ok: (theme.ok, theme.okSoft)
        case .danger: (theme.danger, theme.dangerSoft)
        case .waiting: (theme.waiting, theme.waitingSoft)
        case .outside: (theme.outside, theme.outsideSoft)
        case .accent: (theme.accent, theme.accentSoft)
        }
    }
}

struct SectionLabel: View {
    let text: String
    @Environment(\.heronTheme) private var theme
    init(_ text: String) { self.text = text }
    var body: some View {
        Text(text).font(HeronFont.subLabel).foregroundStyle(theme.ink3).accessibilityAddTraits(.isHeader)
    }
}

// MARK: Checkbox and radio (drawn, so they look the same everywhere, including snapshots)

struct HCheckbox: View {
    @Binding var isOn: Bool
    let label: String
    @Environment(\.heronTheme) private var theme

    var body: some View {
        Button { isOn.toggle() } label: {
            HStack(spacing: Spacing.s2) {
                ZStack {
                    RoundedRectangle(cornerRadius: Radius.tag, style: .continuous)
                        .fill(isOn ? theme.accent : theme.surface)
                    RoundedRectangle(cornerRadius: Radius.tag, style: .continuous)
                        .stroke(isOn ? theme.accent : theme.ink3, lineWidth: 1.2)
                    if isOn { Image(systemName: "checkmark").font(.system(size: 10, weight: .bold)).foregroundStyle(theme.onAccent) }
                }
                .frame(width: 16, height: 16)
                Text(label).foregroundStyle(theme.ink)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(isOn ? .isSelected : [])
        .accessibilityValue(isOn ? "On" : "Off")
    }
}

struct HRadioMark: View {
    let selected: Bool
    @Environment(\.heronTheme) private var theme
    @Environment(\.isEnabled) private var enabled
    var body: some View {
        ZStack {
            Circle().stroke(selected ? theme.accent : theme.ink3, lineWidth: 1.2)
            if selected { Circle().fill(theme.accent).padding(3.5) }
        }
        .frame(width: 16, height: 16)
        .opacity(enabled ? 1 : 0.45)
    }
}

/// A radio row: a drawn mark, a title and an optional second line.
struct HRadioRow<Value: Hashable>: View {
    let value: Value
    @Binding var selection: Value
    let title: String
    var subtitle: String?
    @Environment(\.heronTheme) private var theme

    var body: some View {
        Button { selection = value } label: {
            HStack(alignment: .top, spacing: Spacing.s2) {
                HRadioMark(selected: selection == value).padding(.top, 1)
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).foregroundStyle(theme.ink)
                    if let subtitle { Text(subtitle).font(HeronFont.callout).foregroundStyle(theme.ink2) }
                }
                Spacer(minLength: 0)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(selection == value ? .isSelected : [])
    }
}

// MARK: Empty, loading, banners

/// "What should we get done?"-style empty state: a serif title, one line of text, one action.
struct EmptyStateView: View {
    let title: String
    let text: String
    var actionTitle: String?
    var action: (() -> Void)?
    @Environment(\.heronTheme) private var theme

    var body: some View {
        VStack(spacing: Spacing.s3) {
            Text(title).font(HeronFont.emptyTitle).foregroundStyle(theme.ink).multilineTextAlignment(.center)
            Text(text).foregroundStyle(theme.ink2).multilineTextAlignment(.center).frame(maxWidth: 420)
            if let actionTitle, let action {
                Button(actionTitle, action: action).buttonStyle(HButtonStyle(.primary)).padding(.top, Spacing.s1)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(Spacing.s8)
        .accessibilityElement(children: .contain)
    }
}

/// Skeleton rows for a list that is loading. They appear only after 300 ms, as flat `raised` blocks;
/// the shimmer stops under Reduce Motion.
struct SkeletonRows: View {
    var count = 4
    @Environment(\.heronTheme) private var theme
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var visible = false
    @State private var dim = false

    var body: some View {
        VStack(spacing: Spacing.s3) {
            ForEach(0..<count, id: \.self) { _ in
                RoundedRectangle(cornerRadius: Radius.tag, style: .continuous).fill(theme.raised).frame(height: 18)
            }
        }
        .opacity(visible ? (dim && !reduceMotion ? 0.55 : 1) : 0)
        .padding(Spacing.s6)
        .task {
            try? await Task.sleep(for: .milliseconds(300))
            visible = true
            if !reduceMotion { withAnimation(.easeInOut(duration: 0.9).repeatForever()) { dim = true } }
        }
        .accessibilityLabel("Loading")
    }
}

enum BannerKind { case info, warning, error }

struct BannerView: View {
    let kind: BannerKind
    let text: String
    var actionTitle: String?
    var action: (() -> Void)?
    var onDismiss: (() -> Void)?
    @Environment(\.heronTheme) private var theme

    var body: some View {
        HStack(spacing: Spacing.s3) {
            Image(systemName: symbol).foregroundStyle(fg)
            Text(text).foregroundStyle(fg == theme.ink ? theme.ink : theme.ink)
            Spacer(minLength: 0)
            if let actionTitle, let action { Button(actionTitle, action: action).buttonStyle(HButtonStyle(.link)) }
            if let onDismiss {
                Button(action: onDismiss) { Image(systemName: "xmark") }
                    .buttonStyle(.plain).accessibilityLabel("Dismiss").foregroundStyle(theme.ink2)
            }
        }
        .padding(.horizontal, Spacing.s4)
        .padding(.vertical, Spacing.s2 + 2)
        .background(bg)
        .accessibilityElement(children: .combine)
    }

    private var bg: Color { kind == .error ? theme.dangerSoft : kind == .warning ? theme.waitingSoft : theme.accentSoft }
    private var fg: Color { kind == .error ? theme.danger : kind == .warning ? theme.waiting : theme.accent }
    private var symbol: String { kind == .error ? "exclamationmark.triangle" : kind == .warning ? "pause.circle" : "info.circle" }
}

// MARK: Markdown

/// Renders Markdown as served: paragraphs, headings and bullet lists, with inline bold, italics, links and
/// code. Text is never rewritten.
struct MarkdownText: View {
    let markdown: String
    var font: Font = .body
    @Environment(\.heronTheme) private var theme

    private enum Block: Hashable { case heading(Int, String), bullet(String), paragraph(String) }

    private var blocks: [Block] {
        var out: [Block] = []
        var paragraph: [String] = []
        func flush() { if !paragraph.isEmpty { out.append(.paragraph(paragraph.joined(separator: " "))); paragraph = [] } }
        for raw in markdown.components(separatedBy: "\n") {
            let line = raw.trimmingCharacters(in: .whitespaces)
            if line.isEmpty { flush(); continue }
            let hashes = line.prefix(while: { $0 == "#" }).count
            if (1...3).contains(hashes), line.dropFirst(hashes).first == " " {
                flush(); out.append(.heading(hashes, line.dropFirst(hashes).trimmingCharacters(in: .whitespaces)))
            } else if line.hasPrefix("- ") || line.hasPrefix("* ") {
                flush(); out.append(.bullet(String(line.dropFirst(2))))
            } else { paragraph.append(line) }
        }
        flush()
        return out
    }

    private func attributed(_ s: String) -> AttributedString {
        (try? AttributedString(markdown: s, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(s)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s3) {
            ForEach(Array(blocks.enumerated()), id: \.offset) { _, block in
                switch block {
                case .heading(let level, let t):
                    Text(attributed(t)).font(level == 1 ? .title3.weight(.semibold) : .headline).foregroundStyle(theme.ink)
                case .bullet(let t):
                    HStack(alignment: .firstTextBaseline, spacing: Spacing.s2) {
                        Text("•").foregroundStyle(theme.ink3)
                        Text(attributed(t)).font(font).foregroundStyle(theme.ink)
                    }
                case .paragraph(let t):
                    Text(attributed(t)).font(font).foregroundStyle(theme.ink).fixedSize(horizontal: false, vertical: true)
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .textSelection(.enabled)
    }
}

// MARK: Dates

enum Fmt {
    static func time(_ d: Date) -> String { format("HH:mm", d) }
    /// "09:30" for a real date, otherwise the core's own words ("tomorrow 10:00").
    static func time(of event: CalendarEvent) -> String { event.start.map { time($0) } ?? event.whenText }
    static func relative(_ d: Date, now: Date = Date()) -> String {
        let secs = now.timeIntervalSince(d)
        if abs(secs) < 60 { return "just now" }
        let f = RelativeDateTimeFormatter()
        f.unitsStyle = .short
        f.locale = Locale(identifier: "en_US_POSIX")
        return f.localizedString(for: d, relativeTo: now)
    }
    static func format(_ pattern: String, _ d: Date) -> String {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = pattern
        return f.string(from: d)
    }
    static func tokens(_ n: Int) -> String { n >= 1000 ? String(format: "%.1fk", Double(n) / 1000) : String(n) }
}
