import SwiftUI
import AppKit
import HeronCore

/// The menu bar icon: a template image of the `ripple` avatar, drawn in code. While something waits for
/// you it gets a small dot. It never animates.
public enum MenuBarIcon {
    public static func image(waiting: Bool) -> NSImage {
        let size = NSSize(width: 18, height: 18)
        let image = NSImage(size: size, flipped: false) { rect in
            NSColor.black.setStroke()
            NSColor.black.setFill()
            let c = NSPoint(x: rect.midX, y: rect.midY)
            for (r, w) in [(7.6, 1.1), (5.0, 1.3)] as [(CGFloat, CGFloat)] {
                let ring = NSBezierPath(ovalIn: NSRect(x: c.x - r, y: c.y - r, width: r * 2, height: r * 2))
                ring.lineWidth = w
                ring.stroke()
            }
            NSBezierPath(ovalIn: NSRect(x: c.x - 2.3, y: c.y - 2.3, width: 4.6, height: 4.6)).fill()
            if waiting {
                let dot = NSRect(x: rect.maxX - 6, y: rect.maxY - 6, width: 6, height: 6)
                // Knock out a ring around the dot so it reads against the rings behind it.
                NSGraphicsContext.current?.compositingOperation = .clear
                NSBezierPath(ovalIn: dot.insetBy(dx: -1.4, dy: -1.4)).fill()
                NSGraphicsContext.current?.compositingOperation = .sourceOver
                NSBezierPath(ovalIn: dot).fill()
            }
            return true
        }
        image.isTemplate = true
        image.accessibilityDescription = waiting ? "Heron, waiting for you" : "Heron"
        return image
    }
}

/// The menu bar panel, 320 pt wide (`.menuBarExtraStyle(.window)`): status, what waits for you, the next
/// event, actions, and the quit line.
public struct MenuBarContent: View {
    @Bindable var model: AppModel
    @Environment(\.openWindow) private var openWindow
    @Environment(\.openSettings) private var openSettings

    public init(model: AppModel) { self.model = model }

    public var body: some View {
        HeronThemed(preferences: model.preferences) { Panel(model: model, openMain: openMain, openSettings: { openSettings() }) }
    }

    private func openMain() {
        NSApp.activate(ignoringOtherApps: true)
        openWindow(id: "main")
    }
}

private struct Panel: View {
    @Bindable var model: AppModel
    let openMain: () -> Void
    let openSettings: () -> Void
    @Environment(\.heronTheme) private var theme

    private var waiting: [(id: String, title: String, open: () -> Void)] {
        let parked = model.approvals.parked.map { p in (id: "p\(p.id)", title: p.summary, open: { model.review(parked: p.id); openMain() }) }
        let live = model.approvals.live.map { r in (id: r.id, title: r.summary, open: { model.review(live: r); openMain() }) }
        return Array((live + parked).prefix(3))
    }

    /// The core serves only events in the next day, so one whose time isn't a date ("tomorrow 10:00") is still ahead.
    private var nextEvent: CalendarEvent? { model.today.summary?.events.first { ($0.start ?? .distantFuture) >= Date() } }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: Spacing.s3) {
                AvatarView(style: model.preferences.avatarStyle, state: model.avatar.state, name: model.agentName, size: 36,
                           image: model.preferences.avatarStyle == .image ? NSImage(contentsOf: DisplayPreferences.avatarImageURL) : nil)
                VStack(alignment: .leading, spacing: 1) {
                    Text(model.agentName).font(HeronFont.title3).foregroundStyle(theme.ink)
                    Text(model.statusLine).font(HeronFont.sub).foregroundStyle(theme.ink2)
                }
                Spacer()
            }
            .padding(Spacing.s4)
            .accessibilityElement(children: .combine)
            .accessibilityLabel(AvatarLook.accessibilityLabel(name: model.agentName, state: model.avatar.state))

            if !waiting.isEmpty {
                RowDivider()
                VStack(alignment: .leading, spacing: Spacing.s2) {
                    SectionLabel("Waiting for you")
                    ForEach(waiting, id: \.id) { item in
                        HStack {
                            Circle().fill(theme.waitingMark).frame(width: 8, height: 8).accessibilityHidden(true)
                            Text(item.title).lineLimit(1).foregroundStyle(theme.ink)
                            Spacer()
                            Button("Review", action: item.open).buttonStyle(HButtonStyle(.link))
                        }
                    }
                }.padding(Spacing.s4)
            }
            if let next = nextEvent {
                RowDivider()
                VStack(alignment: .leading, spacing: Spacing.s1) {
                    SectionLabel("Next")
                    Text("\(Fmt.time(of: next))  \(next.title)").foregroundStyle(theme.ink).monospacedDigit()
                }.padding(Spacing.s4)
            }
            RowDivider()
            VStack(alignment: .leading, spacing: Spacing.s1) {
                if model.quickEntry.isEnabled {
                    action("Ask Heron…", symbol: "text.bubble") { model.quickEntry.show() }
                }
                action("Prep my day", symbol: "sun.max") { Task { await model.prepMyDay() }; openMain() }
                action("New conversation", symbol: "square.and.pencil", shortcut: "⌘N") { model.newConversation(); openMain() }
                action("Open Heron", symbol: "macwindow") { openMain() }
                action(model.paused ? "Resume Heron" : "Pause Heron", symbol: "pause.circle") { Task { await model.setPaused(!model.paused) } }
            }.padding(Spacing.s2)
            RowDivider()
            VStack(alignment: .leading, spacing: Spacing.s1) {
                action("Settings…", symbol: "gearshape", shortcut: "⌘,") { openSettings() }
                action("Quit Heron", symbol: "power", shortcut: "⌘Q") { NSApp.terminate(nil) }
                Text("Quitting stops schedules and the phone connection.").font(HeronFont.foot).foregroundStyle(theme.ink3).padding(.horizontal, Spacing.s3)
            }.padding(Spacing.s2)
        }
        .frame(width: 320)
        .background(theme.surface)
        .task { await model.today.load(); await model.approvals.load() }
    }

    private func action(_ title: String, symbol: String, shortcut: String? = nil, perform: @escaping () -> Void) -> some View {
        Button(action: perform) {
            HStack(spacing: Spacing.s2) {
                Image(systemName: symbol).foregroundStyle(theme.ink2).frame(width: 18)
                Text(title).foregroundStyle(theme.ink)
                Spacer()
                if let shortcut { Text(shortcut).font(HeronFont.sub).foregroundStyle(theme.ink3) }
            }
            .padding(.horizontal, Spacing.s3).padding(.vertical, 5).contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}
