import SwiftUI
import AppKit
import HeronCore

/// The standard Settings scene: eight tabs, 720 pt wide.
public struct SettingsView: View {
    @Bindable var model: AppModel

    public init(model: AppModel) { self.model = model }

    public var body: some View {
        HeronThemed(preferences: model.preferences) {
            TabView(selection: $model.settingsTab) {
                ForEach(SettingsTab.allCases) { tab in
                    SettingsTabContent(model: model, tab: tab)
                        .tabItem { Label(tab.title, systemImage: tab.symbol) }
                        .tag(tab)
                }
            }
            .frame(width: 720, height: 560)
            .task { await model.settings.load(); await model.persona.load() }
        }
    }
}

/// One settings tab inside the page's one scroll container (heron-v1.md item 41b).
struct SettingsTabContent: View {
    @Bindable var model: AppModel
    let tab: SettingsTab
    @Environment(\.heronTheme) private var theme

    var body: some View {
        HScroll {
            VStack(alignment: .leading, spacing: Spacing.s5) {
                switch tab {
                case .general: GeneralTab(model: model)
                case .heron: PersonaTab(model: model)
                case .appearance: AppearanceTab(model: model)
                case .model: ModelTab(model: model)
                case .permissions: PermissionsTab(model: model)
                case .features: FeaturesSettingsView(store: model.features, shortcuts: model.shortcuts)
                case .connections: ConnectionsTab(model: model)
                case .usage: UsageTab(model: model)
                }
            }
            .padding(Spacing.s6)
        }
        .background(theme.surface)
    }
}

private struct GeneralTab: View {
    @Bindable var model: AppModel
    @Environment(\.heronTheme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s5) {
            group("Working folder") {
                HStack {
                    Text(model.settings.settings?.workspace ?? model.state?.workspace ?? "~/Work").font(HeronFont.mono).foregroundStyle(theme.ink)
                    Spacer()
                    Button("Choose…") { chooseFolder() }.buttonStyle(HButtonStyle(.secondary)).disabled(true)
                        .help("Changing the folder arrives with the live client.")
                }
                Text("Heron reads and writes here. Folders that hold secrets are refused in every mode.").font(HeronFont.callout).foregroundStyle(theme.ink2)
            }
            group("Startup") {
                Toggle("Launch Heron at login", isOn: .constant(false)).disabled(true)
                Text("Not in this preview. A login item arrives once unattended work through a subscription login is allowed.")
                    .font(HeronFont.callout).foregroundStyle(theme.ink2)
                Toggle("Show Heron in the menu bar", isOn: Bindable(model.preferences).showMenuBarExtra)
                Text("The menu bar item keeps Heron, and its schedules, running when you close the window.").font(HeronFont.callout).foregroundStyle(theme.ink2)
            }
            group("Phone connection") {
                Label(model.settings.settings?.phoneConnected == true ? "Connected" : "Not connected",
                      systemImage: model.settings.settings?.phoneConnected == true ? "iphone" : "iphone.slash")
                    .foregroundStyle(theme.ink)
                if !model.client.endpoint.backgroundOwner {
                    Text("Another Heron holds the phone connection.").font(HeronFont.callout).foregroundStyle(theme.ink2)
                }
            }
        }
    }

    private func chooseFolder() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.prompt = "Choose"
        _ = panel.runModal()
    }

    private func group<C: View>(_ title: String, @ViewBuilder _ content: () -> C) -> some View {
        VStack(alignment: .leading, spacing: Spacing.s2) { SectionLabel(title); content() }
    }
}

private struct PersonaTab: View {
    @Bindable var model: AppModel
    @Environment(\.heronTheme) private var theme
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        let persona = model.persona
        VStack(alignment: .leading, spacing: Spacing.s4) {
            Text("Name and personality").font(HeronFont.title3).foregroundStyle(theme.ink)
            VStack(alignment: .leading, spacing: Spacing.s2) {
                SectionLabel("Name")
                TextField("Heron", text: Bindable(persona).draftName).textFieldStyle(.roundedBorder).frame(maxWidth: 320)
                    .accessibilityLabel("Agent name")
            }
            VStack(alignment: .leading, spacing: Spacing.s2) {
                SectionLabel("Personality")
                TextEditor(text: Bindable(persona).draftText).font(HeronFont.body).frame(height: 140)
                    .scrollContentBackground(.hidden).padding(6)
                    .background(RoundedRectangle(cornerRadius: Radius.control, style: .continuous).fill(theme.raised))
                    .accessibilityLabel("Personality")
                Text("\(persona.charactersLeft) characters left").font(HeronFont.sub).monospacedDigit()
                    .foregroundStyle(persona.charactersLeft < 0 ? theme.danger : theme.ink3)
            }
            HStack {
                Button("Save") { Task { await persona.save() } }.buttonStyle(HButtonStyle(.primary)).disabled(!persona.canSave)
                if persona.saved && !persona.isDirty { Label("Saved", systemImage: "checkmark").foregroundStyle(theme.ok) }
            }
            Divider().overlay(theme.hairline)
            VStack(alignment: .leading, spacing: Spacing.s2) {
                SectionLabel("Getting to know you")
                Text("Heron can ask a few questions about you and your work, then remember the answers. You confirm each one in Memory.")
                    .font(HeronFont.callout).foregroundStyle(theme.ink2)
                Button("Start the questions") {
                    model.chat.startNewConversation()
                    model.chat.composer = "Ask me a few questions so you can get to know me. One at a time, and remember what I tell you."
                    model.selection = .chat
                    NSApp.windows.first { $0.identifier?.rawValue.contains("main") == true }?.makeKeyAndOrderFront(nil)
                }.buttonStyle(HButtonStyle(.secondary))
            }
        }
    }
}

private struct AppearanceTab: View {
    @Bindable var model: AppModel
    @Environment(\.heronTheme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s5) {
            AppearancePickers(preferences: model.preferences, agentName: model.agentName)
            VStack(alignment: .leading, spacing: Spacing.s2) {
                SectionLabel("Light or dark")
                Picker("Light or dark", selection: Bindable(model.preferences).appearance) {
                    ForEach(AppearanceChoice.allCases) { Text($0.title).tag($0) }
                }.pickerStyle(.segmented).labelsHidden().frame(maxWidth: 280)
            }
            VStack(alignment: .leading, spacing: Spacing.s2) {
                SectionLabel("Text size")
                Picker("Text size", selection: Bindable(model.preferences).textSize) {
                    ForEach(TextSizeChoice.allCases) { Text($0.title).tag($0) }
                }.pickerStyle(.segmented).labelsHidden().frame(maxWidth: 280)
                Text("⌘+ and ⌘− change it inside a conversation too.").font(HeronFont.callout).foregroundStyle(theme.ink2)
            }
        }
    }
}

private struct ModelTab: View {
    @Bindable var model: AppModel
    @Environment(\.heronTheme) private var theme

    var body: some View {
        let store = model.settings
        VStack(alignment: .leading, spacing: Spacing.s4) {
            RowGroup {
                ForEach(Array(store.providers.enumerated()), id: \.element.id) { i, p in
                    if i > 0 { RowDivider() }
                    HStack(alignment: .top, spacing: Spacing.s3) {
                        VStack(alignment: .leading, spacing: 2) {
                            HStack(spacing: Spacing.s2) {
                                Text(p.displayName).font(HeronFont.body.weight(.semibold)).foregroundStyle(theme.ink)
                                switch p.rowState {
                                case .signedIn: HTag("Signed in", tone: .ok, symbol: "checkmark")
                                case .signedOut: HTag("Signed out", tone: .danger)
                                case .untested: HTag("Untested version", tone: .waiting, symbol: "hand.raised")
                                case .notInstalled: EmptyView()
                                }
                                if p.isActive { HTag("In use", tone: .accent) }
                            }
                            Text(rowLine(p)).font(HeronFont.callout).foregroundStyle(theme.ink2).fixedSize(horizontal: false, vertical: true)
                        }
                        Spacer()
                        if p.rowState == .untested { Button("Use anyway…") { Task { await store.acceptVersion(p) } }.buttonStyle(HButtonStyle(.secondary)) }
                        else if p.isSelectable && !p.isActive { Button("Use") { Task { await store.select(provider: p) } }.buttonStyle(HButtonStyle(.secondary)) }
                    }
                    .padding(.horizontal, Spacing.s4).padding(.vertical, 10)
                    .accessibilityElement(children: .contain)
                }
            }
            Text("Heron never stores your CLI password. It runs the app you're signed in to, inside the sandbox, with its own tools turned off.")
                .font(HeronFont.callout).foregroundStyle(theme.ink2)
            VStack(alignment: .leading, spacing: Spacing.s1) {
                Toggle("Let schedules use a subscription login", isOn: Binding(get: { store.settings?.allowSubscriptionUnattended ?? false },
                                                                               set: { v in Task { await store.setUnattended(v) } }))
                Text("Off: scheduled runs wait until you open Heron. Check your provider's terms before turning this on.")
                    .font(HeronFont.callout).foregroundStyle(theme.ink2)
            }
        }
    }

    private func rowLine(_ p: ProviderStatus) -> String {
        switch p.rowState {
        case .signedIn:
            p.signedInLine
        default: p.detailText
        }
    }
}

private struct PermissionsTab: View {
    @Bindable var model: AppModel
    @Environment(\.heronTheme) private var theme

    var body: some View {
        let store = model.settings
        VStack(alignment: .leading, spacing: Spacing.s5) {
            VStack(alignment: .leading, spacing: Spacing.s2) {
                SectionLabel("Sandbox")
                HStack(alignment: .top, spacing: Spacing.s3) {
                    Image(systemName: store.sandboxOn ? "lock.shield" : "exclamationmark.triangle")
                        .foregroundStyle(store.sandboxOn ? theme.ok : theme.danger)
                    if store.sandboxOn {
                        Text("On (macOS sandbox). Commands and CLI models can read \(store.settings?.workspace ?? "~/Work") and the folders you allow below. Everything else asks first, and secrets are always refused.")
                    } else {
                        Text("Off. The macOS sandbox didn't start, so Heron won't run shell commands or CLI models. Chat with an API-key model still works.")
                    }
                }
                .foregroundStyle(theme.ink).fixedSize(horizontal: false, vertical: true)
                .padding(Spacing.s3).frame(maxWidth: .infinity, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: Radius.card, style: .continuous).fill(store.sandboxOn ? theme.okSoft : theme.dangerSoft))
                .accessibilityElement(children: .combine)
            }
            VStack(alignment: .leading, spacing: Spacing.s2) {
                SectionLabel("Permission mode")
                Picker("Permission mode", selection: Binding(get: { store.mode }, set: { m in Task { await store.setMode(m); await model.refreshState() } })) {
                    ForEach(PermissionMode.allCases, id: \.self) { Text($0.title).tag($0) }
                }.pickerStyle(.segmented).labelsHidden().frame(maxWidth: 420)
                Text(store.mode.sentence).font(HeronFont.callout).foregroundStyle(theme.ink2)
            }
            VStack(alignment: .leading, spacing: Spacing.s2) {
                SectionLabel("What you've allowed")
                if store.grants.isEmpty {
                    Text("Nothing yet. When Heron asks, you can allow something for a folder or a conversation.").foregroundStyle(theme.ink2)
                } else {
                    RowGroup {
                        ForEach(Array(store.grants.enumerated()), id: \.element.id) { i, g in
                            if i > 0 { RowDivider() }
                            HStack {
                                VStack(alignment: .leading, spacing: 1) {
                                    Text(g.kind == "read" ? "Read files in" : "Allowed").font(HeronFont.callout).foregroundStyle(theme.ink2)
                                    Text(g.rule).font(HeronFont.mono).foregroundStyle(theme.ink)
                                }
                                Spacer()
                                Button("Remove") { Task { await store.revoke(g) } }.buttonStyle(HButtonStyle(.secondary))
                                    .accessibilityLabel("Remove \(g.rule)")
                            }.padding(.horizontal, Spacing.s4).padding(.vertical, 10)
                        }
                    }
                }
            }
            NeverSection(store: store)
            if !store.ruleNotices.isEmpty {
                VStack(alignment: .leading, spacing: Spacing.s2) {
                    SectionLabel("Older rules")
                    RowGroup {
                        ForEach(Array(store.ruleNotices.enumerated()), id: \.element.id) { i, n in
                            if i > 0 { RowDivider() }
                            VStack(alignment: .leading, spacing: Spacing.s1) {
                                HStack { Text(n.rule).font(HeronFont.mono).foregroundStyle(theme.ink); HTag("No longer applies", tone: .waiting) }
                                Text(n.notice).font(HeronFont.callout).foregroundStyle(theme.ink2).fixedSize(horizontal: false, vertical: true)
                            }.padding(.horizontal, Spacing.s4).padding(.vertical, 10)
                        }
                    }
                }
            }
        }
    }
}

private struct ConnectionsTab: View {
    @Bindable var model: AppModel
    @Environment(\.heronTheme) private var theme

    var body: some View {
        let store = model.settings
        VStack(alignment: .leading, spacing: Spacing.s4) {
            RowGroup {
                ForEach(Array((store.settings?.connections ?? []).enumerated()), id: \.element.id) { i, c in
                    if i > 0 { RowDivider() }
                    HStack {
                        VStack(alignment: .leading, spacing: 1) {
                            Text(c.name).foregroundStyle(theme.ink)
                            Text(c.detail).font(HeronFont.callout).foregroundStyle(theme.ink2)
                        }
                        Spacer()
                        HTag(c.connected ? "Connected" : "Not connected", tone: c.connected ? .ok : .neutral)
                    }.padding(.horizontal, Spacing.s4).padding(.vertical, 10)
                }
            }
            VStack(alignment: .leading, spacing: Spacing.s1) {
                Toggle("Awareness", isOn: Binding(get: { store.settings?.awareness ?? false }, set: { v in Task { await store.setAwareness(v) } }))
                Text("Lets Heron notice what you're working on, so it can suggest things. Its suggestions appear in the Inbox.")
                    .font(HeronFont.callout).foregroundStyle(theme.ink2)
            }
        }
    }
}

private struct UsageTab: View {
    @Bindable var model: AppModel
    @Environment(\.heronTheme) private var theme

    var body: some View {
        let usage = model.settings.usage
        VStack(alignment: .leading, spacing: Spacing.s4) {
            if let usage {
                Text("This month: \(usage.monthTokens.formatted()) tokens").font(HeronFont.title3).foregroundStyle(theme.ink).monospacedDigit()
                RowGroup {
                    ForEach(Array(usage.days.enumerated()), id: \.element.id) { i, d in
                        if i > 0 { RowDivider() }
                        HStack {
                            Text(d.day).monospacedDigit().foregroundStyle(theme.ink)
                            Text(d.model).foregroundStyle(theme.ink2)
                            Spacer()
                            Text("\(Fmt.tokens(d.promptTokens)) in · \(Fmt.tokens(d.completionTokens)) out").font(HeronFont.callout).monospacedDigit().foregroundStyle(theme.ink2)
                        }.padding(.horizontal, Spacing.s4).padding(.vertical, 10)
                    }
                }
            } else {
                SkeletonRows(count: 3)
            }
        }
    }
}
