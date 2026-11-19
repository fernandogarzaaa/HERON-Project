import SwiftUI
import AppKit
import HeronCore

// MARK: Chrome

/// The three-step bar at the top of first run.
struct FirstRunStepBar: View {
    let current: FirstRunStore.Step
    @Environment(\.heronTheme) private var theme

    var body: some View {
        HStack(spacing: Spacing.s6) {
            ForEach(FirstRunStore.Step.allCases, id: \.rawValue) { step in
                HStack(spacing: Spacing.s2) {
                    ZStack {
                        Circle().fill(step.rawValue <= current.rawValue ? theme.accent : theme.raised)
                        if step.rawValue < current.rawValue {
                            Image(systemName: "checkmark").font(.system(size: 9, weight: .bold)).foregroundStyle(theme.onAccent)
                        } else {
                            Text("\(step.rawValue + 1)").font(HeronFont.subLabel).monospacedDigit()
                                .foregroundStyle(step == current ? theme.onAccent : theme.ink2)
                        }
                    }
                    .frame(width: 20, height: 20)
                    Text(step.title).font(HeronFont.body.weight(step == current ? .semibold : .regular))
                        .foregroundStyle(step == current ? theme.ink : theme.ink2)
                }
                .accessibilityElement(children: .combine)
                .accessibilityLabel("Step \(step.rawValue + 1) of 3: \(step.title)")
                .accessibilityAddTraits(step == current ? .isSelected : [])
            }
        }
        .frame(maxWidth: .infinity, minHeight: 48)
        .background(theme.sidebar)
        .overlay(alignment: .bottom) { Rectangle().fill(theme.hairline).frame(height: 1) }
    }
}

// MARK: Step 1: the risk notice

/// The sandbox status card above the risk text: green on, red off (heron-mac-visual.md, D8).
struct SandboxStatusCard: View {
    let on: Bool
    @Environment(\.heronTheme) private var theme

    var body: some View {
        HStack(alignment: .top, spacing: Spacing.s3) {
            Image(systemName: on ? "lock.shield" : "exclamationmark.triangle")
                .font(.title2).foregroundStyle(on ? theme.ok : theme.danger).frame(width: 28)
            if on {
                (Text("Sandbox is on. ").fontWeight(.semibold) + Text("Commands and CLI models run inside the macOS sandbox. They can read your working folder, and nothing outside it unless you allow it."))
                    .foregroundStyle(theme.ink)
            } else {
                (Text("Sandbox is off. ").fontWeight(.semibold) + Text("The macOS sandbox didn't start, so Heron won't run shell commands or CLI models. You can still accept this notice. Settings › Permissions shows the cause."))
                    .foregroundStyle(theme.ink)
            }
        }
        .fixedSize(horizontal: false, vertical: true)
        .padding(Spacing.s4)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: Radius.card, style: .continuous).fill(on ? theme.okSoft : theme.dangerSoft))
        .accessibilityElement(children: .combine)
    }
}

/// The risk notice screen (first-run step 1, and the sheet shown after an update). The text is shown
/// exactly as served and never rewritten.
public struct RiskNoticeScreen: View {
    public enum Style: Sendable { case firstRun, sheet }

    let notice: RiskNotice
    let sandboxOn: Bool
    let noticeUpdated: Bool
    @Binding var accepted: Bool
    let style: Style
    let avatarStyle: AvatarStyle
    let onAccept: () -> Void
    let onQuit: () -> Void

    @Environment(\.heronTheme) private var theme

    public init(notice: RiskNotice, sandboxOn: Bool, noticeUpdated: Bool = false, accepted: Binding<Bool>,
                style: Style = .firstRun, avatarStyle: AvatarStyle = .ripple,
                onAccept: @escaping () -> Void = {}, onQuit: @escaping () -> Void = {}) {
        self.notice = notice
        self.sandboxOn = sandboxOn
        self.noticeUpdated = noticeUpdated
        self._accepted = accepted
        self.style = style
        self.avatarStyle = avatarStyle
        self.onAccept = onAccept
        self.onQuit = onQuit
    }

    public var body: some View {
        VStack(spacing: 0) {
            if style == .firstRun { FirstRunStepBar(current: .risks) }
            VStack(alignment: .leading, spacing: Spacing.s4) {
                hero
                SandboxStatusCard(on: sandboxOn)
                if noticeUpdated {
                    Text("The risk notice was updated. Please read the new version.")
                        .foregroundStyle(theme.waiting).font(HeronFont.body.weight(.medium))
                        .padding(Spacing.s3).frame(maxWidth: .infinity, alignment: .leading)
                        .background(RoundedRectangle(cornerRadius: Radius.control, style: .continuous).fill(theme.waitingSoft))
                }
                VStack(alignment: .leading, spacing: Spacing.s2) {
                    HStack(alignment: .firstTextBaseline) {
                        SectionLabel("Known risks")
                        Spacer()
                        Text("Version \(notice.version) · from the risk register built into this copy of Heron")
                            .font(HeronFont.sub).foregroundStyle(theme.ink3)
                    }
                    HScroll(maxHeight: 230) { MarkdownText(markdown: notice.text).padding(Spacing.s4) }
                        .frame(height: 230)
                        .background(RoundedRectangle(cornerRadius: Radius.card, style: .continuous).fill(theme.surface))
                        .overlay(RoundedRectangle(cornerRadius: Radius.card, style: .continuous).stroke(theme.cardBorder, lineWidth: theme.cardBorderWidth))
                        .clipShape(RoundedRectangle(cornerRadius: Radius.card, style: .continuous))
                        .accessibilityLabel("Known risks, version \(notice.version)")
                }
                HCheckbox(isOn: $accepted, label: "I've read these risks and accept them on this Mac")
                footer
            }
            .padding(.horizontal, Spacing.s8)
            .padding(.top, Spacing.s6)
            .padding(.bottom, Spacing.s5)
            .frame(maxWidth: 720)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        }
        .background(theme.window)
    }

    private var hero: some View {
        HStack(alignment: .center, spacing: Spacing.s4) {
            AvatarView(style: avatarStyle, state: .idle, name: "Heron", size: 64)
            VStack(alignment: .leading, spacing: Spacing.s1) {
                Text(style == .sheet ? "The risk notice has changed" : "Before you start")
                    .font(HeronFont.large).foregroundStyle(theme.ink).accessibilityAddTraits(.isHeader)
                Text("Heron is a research preview. It can read and change files in your folder, run commands, and use the accounts you connect. Read what that means for you, then accept to continue.")
                    .foregroundStyle(theme.ink2).fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private var footer: some View {
        HStack {
            Button("Quit Heron", action: onQuit).buttonStyle(HButtonStyle(.secondary))
            Spacer()
            Text("Heron v1.0 · Research preview").font(HeronFont.sub).foregroundStyle(theme.ink3)
            Button("Accept and continue", action: onAccept)
                .buttonStyle(HButtonStyle(.primary)).disabled(!accepted)
                .keyboardShortcut(.defaultAction)
        }
    }
}

// MARK: Step 2: connect a model

struct ProviderRows: View {
    let providers: [ProviderStatus]
    let recommendedID: String?
    @Binding var selection: String?
    var onAcceptUntested: (ProviderStatus) -> Void = { _ in }
    @Environment(\.heronTheme) private var theme

    var body: some View {
        VStack(spacing: 0) {
            ForEach(Array(providers.enumerated()), id: \.element.id) { index, p in
                if index > 0 { Divider().overlay(theme.hairline) }
                row(p)
            }
        }
        .background(RoundedRectangle(cornerRadius: Radius.card, style: .continuous).fill(theme.surface))
        .overlay(RoundedRectangle(cornerRadius: Radius.card, style: .continuous).stroke(theme.cardBorder, lineWidth: theme.cardBorderWidth))
        .clipShape(RoundedRectangle(cornerRadius: Radius.card, style: .continuous))
    }

    private func row(_ p: ProviderStatus) -> some View {
        HStack(alignment: .top, spacing: Spacing.s3) {
            Button { if p.isSelectable { selection = p.id } } label: { HRadioMark(selected: selection == p.id).padding(.top, 2) }
                .buttonStyle(.plain).disabled(!p.isSelectable)
                .accessibilityLabel("Use \(p.displayName)")
                .accessibilityAddTraits(selection == p.id ? .isSelected : [])
            VStack(alignment: .leading, spacing: Spacing.s1) {
                HStack(spacing: Spacing.s2) {
                    Text(p.displayName).font(HeronFont.body.weight(.semibold)).foregroundStyle(theme.ink)
                    tag(p)
                    if p.id == recommendedID { HTag("Recommended", tone: .accent) }
                }
                Text(p.detailText).font(HeronFont.callout).foregroundStyle(theme.ink2).fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 0)
            action(p)
        }
        .padding(.horizontal, Spacing.s4).padding(.vertical, 10)
    }

    @ViewBuilder private func tag(_ p: ProviderStatus) -> some View {
        switch p.rowState {
        case .signedIn: HTag("Signed in", tone: .ok, symbol: "checkmark")
        case .signedOut: HTag("Signed out", tone: .danger)
        case .untested: HTag("Untested version", tone: .waiting, symbol: "hand.raised")
        case .notInstalled: EmptyView()
        }
    }

    @ViewBuilder private func action(_ p: ProviderStatus) -> some View {
        switch p.rowState {
        case .signedOut:
            Button("Copy command") {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(p.loginCommand, forType: .string)
            }.buttonStyle(HButtonStyle(.secondary))
        case .untested:
            Button("Use anyway…") { onAcceptUntested(p) }.buttonStyle(HButtonStyle(.secondary))
        case .notInstalled:
            Button("How to install") {}.buttonStyle(HButtonStyle(.link))
        case .signedIn:
            EmptyView()
        }
    }
}

// MARK: The flow

public struct FirstRunView: View {
    @Bindable var model: AppModel
    @Environment(\.heronTheme) private var theme

    public init(model: AppModel) { self.model = model }

    private var store: FirstRunStore { model.firstRun }

    public var body: some View {
        Group {
            switch store.step {
            case .risks:
                if let notice = store.notice {
                    RiskNoticeScreen(notice: notice, sandboxOn: store.sandboxOn, noticeUpdated: store.noticeUpdated,
                                     accepted: Bindable(store).accepted, avatarStyle: model.preferences.avatarStyle,
                                     onAccept: { Task { await store.accept() } },
                                     onQuit: { NSApp.terminate(nil) })
                } else {
                    VStack { FirstRunStepBar(current: .risks); startingView }.background(theme.window)
                }
            case .model: shell(.model) { modelStep }
            case .personalize: shell(.personalize) { personalizeStep }
            }
        }
        .animation(Motion.slow, value: store.step)
    }

    private var startingView: some View {
        VStack(spacing: Spacing.s3) {
            AvatarView(style: model.preferences.avatarStyle, state: .thinking, size: 48)
            Text("Starting Heron…").font(HeronFont.title3).foregroundStyle(theme.ink)
            Text("This usually takes a second or two.").foregroundStyle(theme.ink2)
        }.frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func shell<Content: View>(_ step: FirstRunStore.Step, @ViewBuilder content: () -> Content) -> some View {
        VStack(spacing: 0) {
            FirstRunStepBar(current: step)
            HScroll { content().padding(.horizontal, Spacing.s8).padding(.vertical, Spacing.s6).frame(maxWidth: 720) }
                .frame(maxWidth: .infinity)
        }
        .background(theme.window)
    }

    private var modelStep: some View {
        VStack(alignment: .leading, spacing: Spacing.s4) {
            Text("Connect a model").font(HeronFont.title).foregroundStyle(theme.ink).accessibilityAddTraits(.isHeader)
            Text("Heron uses an AI app you're already signed in to on this Mac. You won't type a password here.").foregroundStyle(theme.ink2)
            ProviderRows(providers: store.providers, recommendedID: store.recommendedID,
                         selection: Bindable(store).selectedProviderID,
                         onAcceptUntested: { p in Task { await model.settings.acceptVersion(p); await store.checkAgain() } })
            HStack {
                Button("Check again") { Task { await store.checkAgain() } }.buttonStyle(HButtonStyle(.secondary))
                Button("Use an API key instead") { Task { await store.useAPIKeyInstead() } }.buttonStyle(HButtonStyle(.link))
                Spacer()
                Button("Continue") { Task { await store.continueFromModel() } }
                    .buttonStyle(HButtonStyle(.primary)).disabled(store.selectedProviderID == nil)
                    .keyboardShortcut(.defaultAction)
            }
        }
    }

    private var personalizeStep: some View {
        VStack(alignment: .leading, spacing: Spacing.s5) {
            Text("Make it yours").font(HeronFont.title).foregroundStyle(theme.ink).accessibilityAddTraits(.isHeader)
            Text("Name your agent and choose how it looks. You can change all of this later in Settings › Appearance.").foregroundStyle(theme.ink2)
            VStack(alignment: .leading, spacing: Spacing.s2) {
                SectionLabel("Name")
                TextField("Heron", text: Bindable(model.persona).draftName).textFieldStyle(.roundedBorder).frame(maxWidth: 320)
                    .accessibilityLabel("Agent name")
            }
            AppearancePickers(preferences: model.preferences, agentName: model.persona.draftName)
            HStack {
                Spacer()
                Button("Start using Heron") { Task { await model.finishFirstRun() } }
                    .buttonStyle(HButtonStyle(.primary)).keyboardShortcut(.defaultAction)
            }
        }
    }
}

/// Avatar style, theme and accent pickers, shared by first run and Settings › Appearance.
struct AppearancePickers: View {
    @Bindable var preferences: DisplayPreferences
    let agentName: String
    @Environment(\.heronTheme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s5) {
            VStack(alignment: .leading, spacing: Spacing.s2) {
                SectionLabel("Avatar")
                HStack(spacing: Spacing.s3) {
                    ForEach(AvatarStyle.allCases) { style in
                        let selected = preferences.avatarStyle == style
                        Button { preferences.avatarStyle = style } label: {
                            VStack(spacing: Spacing.s2) {
                                AvatarView(style: style, state: .idle, name: agentName.isEmpty ? "Heron" : agentName, size: 44,
                                           image: style == .image ? NSImage(contentsOf: DisplayPreferences.avatarImageURL) : nil)
                                Text(style.title).font(HeronFont.sub).foregroundStyle(theme.ink)
                            }
                            .padding(Spacing.s3).frame(width: 88)
                            .background(RoundedRectangle(cornerRadius: Radius.card, style: .continuous).fill(selected ? theme.accentSoft : theme.surface))
                            .overlay(RoundedRectangle(cornerRadius: Radius.card, style: .continuous)
                                .stroke(selected ? theme.accent : theme.cardBorder, lineWidth: selected ? 2 : 1))
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel("\(style.title) avatar")
                        .accessibilityAddTraits(selected ? .isSelected : [])
                    }
                }
            }
            VStack(alignment: .leading, spacing: Spacing.s2) {
                SectionLabel("Theme")
                HStack(spacing: Spacing.s3) {
                    ForEach(ThemeName.allCases) { name in
                        let t = HeronTheme.resolve(theme: name, accent: preferences.accentColor, isDark: theme.isDark)
                        let selected = preferences.theme == name
                        Button { preferences.theme = name } label: {
                            VStack(spacing: Spacing.s2) {
                                HStack(spacing: 0) { t.sidebar; t.surface; t.raised }.frame(width: 72, height: 28)
                                    .clipShape(RoundedRectangle(cornerRadius: Radius.control, style: .continuous))
                                Text(name.title).font(HeronFont.sub).foregroundStyle(theme.ink)
                            }
                            .padding(Spacing.s3)
                            .overlay(RoundedRectangle(cornerRadius: Radius.card, style: .continuous)
                                .stroke(selected ? theme.accent : theme.hairline, lineWidth: selected ? 2 : 1))
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel("\(name.title) theme")
                        .accessibilityAddTraits(selected ? .isSelected : [])
                    }
                }
            }
            VStack(alignment: .leading, spacing: Spacing.s2) {
                SectionLabel("Accent colour")
                HStack(spacing: Spacing.s3) {
                    ForEach(AccentName.allCases) { accent in
                        let c = HeronTheme.resolve(theme: preferences.theme, accent: accent, isDark: theme.isDark).accent
                        let selected = preferences.accentColor == accent
                        Button { preferences.accentColor = accent } label: {
                            Circle().fill(c).frame(width: 24, height: 24)
                                .overlay(Circle().stroke(theme.ink, lineWidth: selected ? 2 : 0).padding(-3))
                                .padding(4)
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel(accent == .system ? "System accent" : accent.title)
                        .accessibilityAddTraits(selected ? .isSelected : [])
                    }
                }
            }
        }
    }
}
