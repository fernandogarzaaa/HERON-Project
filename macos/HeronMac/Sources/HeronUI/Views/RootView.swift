import SwiftUI
import AppKit
import HeronCore

/// The smallest sizes the app's windows allow: the `minWidth` and `minHeight` it sets. The overflow snapshots
/// (heron-v1.md item 41b) render at these, so the layouts are checked where space is tightest.
enum WindowMinimum {
    /// The main window.
    static let main = CGSize(width: 900, height: 600)
    /// The sidebar's least width; the page beside it gets the rest.
    static let sidebarWidth: CGFloat = 220
    /// The first-run window.
    static let firstRun = CGSize(width: 900, height: 700)
    /// The Settings window is a fixed 720 × 560 pt; its tab bar takes the top of that, so a tab's page is a little
    /// shorter than the window.
    static let settingsPage = CGSize(width: 720, height: 500)
    /// The page beside the sidebar in the smallest main window.
    static var page: CGSize { CGSize(width: main.width - sidebarWidth, height: main.height) }
}

/// The app's root: first run, or the main window. It applies the theme, text size and accent.
public struct RootView: View {
    @Bindable var model: AppModel

    public init(model: AppModel) { self.model = model }

    public var body: some View {
        HeronThemed(preferences: model.preferences) {
            RootContent(model: model)
        }
    }
}

struct RootContent: View {
    @Bindable var model: AppModel
    @Environment(\.heronTheme) private var theme

    var body: some View {
        Group {
            if !model.hasBootstrapped {
                StartingView(avatarStyle: model.preferences.avatarStyle)
            } else if model.needsFirstRun {
                FirstRunView(model: model).frame(minWidth: WindowMinimum.firstRun.width, minHeight: WindowMinimum.firstRun.height)
            } else {
                MainView(model: model)
            }
        }
        .background(theme.window)
        .task { await model.bootstrap() }
    }
}

/// "Starting Heron…": nothing for the first 2 seconds, then the thinking avatar.
struct StartingView: View {
    let avatarStyle: AvatarStyle
    @Environment(\.heronTheme) private var theme
    @State private var visible = false

    var body: some View {
        VStack(spacing: Spacing.s3) {
            AvatarView(style: avatarStyle, state: .thinking, size: 56)
            Text("Starting Heron…").font(HeronFont.title3).foregroundStyle(theme.ink)
            Text("This usually takes a second or two.").foregroundStyle(theme.ink2)
        }
        .opacity(visible ? 1 : 0)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .frame(minWidth: 600, minHeight: 400)
        .task { try? await Task.sleep(for: .seconds(2)); withAnimation(Motion.std) { visible = true } }
    }
}

// MARK: Main window

struct MainView: View {
    @Bindable var model: AppModel
    @Environment(\.heronTheme) private var theme
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var showsLearnMore = false

    var body: some View {
        NavigationSplitView {
            SidebarView(model: model)
                .navigationSplitViewColumnWidth(min: WindowMinimum.sidebarWidth, ideal: 240, max: 300)
        } detail: {
            VStack(spacing: 0) {
                banners
                DestinationView(model: model)
            }
            .background(theme.surface)
            .navigationTitle(title)
            .navigationSubtitle("Research preview")
            .toolbar { toolbar }
        }
        .frame(minWidth: WindowMinimum.main.width, minHeight: WindowMinimum.main.height)
        .sheet(item: Binding(get: { model.chat.liveApproval }, set: { _ in })) { request in
            ApprovalSheetView(request: request, avatarStyle: model.preferences.avatarStyle, agentName: model.agentName) { decision in
                Task { await model.chat.answer(decision) }
            }
            .interactiveDismissDisabled(true)
        }
        .sheet(isPresented: $model.showsRiskNoticeSheet) { riskNoticeSheet }
        .onChange(of: model.chat.needsRiskNotice) { _, needs in
            if needs { model.showsRiskNoticeSheet = true; model.chat.clearRiskNoticeRequest() }
        }
    }

    private var title: String {
        model.selection == .chat ? model.chat.title : model.selection.title
    }

    // MARK: Banners

    @ViewBuilder private var banners: some View {
        VStack(spacing: 0) {
            if model.showsAnotherHeronBanner {
                BannerView(kind: .info,
                           text: "Another Heron is running your schedules and phone connection. This window still works as usual.",
                           actionTitle: "Learn more", action: { showsLearnMore = true },
                           onDismiss: { model.dismissedAnotherHeronBanner = true })
                    .popover(isPresented: $showsLearnMore, arrowEdge: .bottom) {
                        Text("Schedules and the phone connection can run in only one place at a time, and the other Heron already holds them. This window starts its own core for everything you see, and never stops the other one.")
                            .padding(Spacing.s4).frame(width: 320)
                    }
            }
            if model.paused {
                BannerView(kind: .warning,
                           text: "Heron is paused. It starts nothing on its own until you resume it. You can still ask it for things.",
                           actionTitle: "Resume", action: { Task { await model.setPaused(false) } })
            }
            if model.coreStatus == .restarted && !model.dismissedRestartBanner {
                BannerView(kind: .info, text: "Heron restarted its core. If a reply was cut off, send it again.",
                           onDismiss: { model.dismissedRestartBanner = true })
            }
            if case .stopped(let details) = model.coreStatus {
                CoreStoppedCard(details: details) { model.restartCore?() }
            }
        }
        .animation(reduceMotion ? Motion.crossfade : Motion.std, value: model.paused)
    }

    // MARK: Toolbar

    @ToolbarContentBuilder private var toolbar: some ToolbarContent {
        ToolbarItemGroup(placement: .primaryAction) {
            if model.selection == .chat && model.chat.isOutsideContent {
                HTag("Outside content", tone: .outside, symbol: "globe")
                    .help("This conversation has read outside content, so Heron asks before it writes, sends or reads outside your folder.")
                    .accessibilityLabel("Outside content. This conversation has read outside content, so Heron asks before it writes, sends or reads outside your folder.")
            }
            Menu {
                ForEach(PermissionMode.allCases, id: \.self) { mode in
                    Button { Task { await model.setMode(mode) } } label: {
                        if model.state?.mode == mode { Label(mode.title, systemImage: "checkmark") } else { Text(mode.title) }
                    }
                }
            } label: { Label(model.state?.mode.title ?? "Ask first", systemImage: "hand.raised") }
                .help("Permission mode: \(model.state?.mode.sentence ?? "")")
                .accessibilityLabel("Permission mode")
            Button { Task { await model.setPaused(!model.paused) } } label: {
                Label(model.paused ? "Resume Heron" : "Pause Heron", systemImage: "pause.circle")
            }
            .help(model.paused ? "Resume Heron" : "Pause Heron")
            Button { model.newConversation() } label: { Label("New conversation", systemImage: "square.and.pencil") }
                .help("New conversation (⌘N)")
        }
    }

    private var riskNoticeSheet: some View {
        Group {
            if let notice = model.firstRun.notice {
                RiskNoticeScreen(notice: notice, sandboxOn: model.state?.sandboxOn ?? true, noticeUpdated: model.firstRun.noticeUpdated,
                                 accepted: Bindable(model.firstRun).accepted, style: .sheet, avatarStyle: model.preferences.avatarStyle,
                                 onAccept: { Task { await model.acceptUpdatedRiskNotice() } },
                                 onQuit: { NSApp.terminate(nil) })
            } else {
                StartingView(avatarStyle: model.preferences.avatarStyle)
            }
        }
        .frame(width: 760, height: 700)
        .interactiveDismissDisabled(true)
        .task { await model.firstRun.load() }
    }
}

/// Shown when `CoreProcess` gave up after two exits within a minute.
struct CoreStoppedCard: View {
    let details: String
    let onRestart: () -> Void
    @Environment(\.heronTheme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s2) {
            Label("Heron's core stopped", systemImage: "exclamationmark.triangle").font(HeronFont.title3).foregroundStyle(theme.danger)
            Text("It stopped again within a minute of restarting, so Heron isn't trying again on its own. Your conversations and settings are safe.")
                .foregroundStyle(theme.ink)
            HStack {
                Button("Restart Heron's core", action: onRestart).buttonStyle(HButtonStyle(.primary))
                Button("Copy details") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(details, forType: .string)
                }.buttonStyle(HButtonStyle(.secondary))
            }
        }
        .padding(Spacing.s4)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(theme.dangerSoft)
    }
}

struct DestinationView: View {
    @Bindable var model: AppModel

    var body: some View {
        switch model.selection {
        case .today: TodayView(model: model)
        case .chat: ChatView(model: model)
        case .inbox: InboxView(model: model)
        case .memory: MemoryView(model: model)
        case .tasks: TasksView(model: model)
        case .schedules: SchedulesView(model: model)
        case .activity: ActivityView(model: model)
        }
    }
}

// MARK: Sidebar

struct SidebarView: View {
    @Bindable var model: AppModel
    @Environment(\.heronTheme) private var theme
    @Environment(\.openSettings) private var openSettings
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

    var body: some View {
        List(selection: Binding(get: { Optional(model.selection) }, set: { if let d = $0 { model.go(d) } })) {
            Section {
                ForEach(Destination.allCases) { d in
                    HStack(spacing: Spacing.s2) {
                        Image(systemName: d.symbol).foregroundStyle(theme.accent).frame(width: 20)
                        Text(d.title).foregroundStyle(theme.ink)
                        Spacer()
                        count(for: d)
                    }
                    .tag(d)
                    .listRowBackground(model.selection == d
                        ? RoundedRectangle(cornerRadius: Radius.control, style: .continuous).fill(theme.accentSoft).padding(.horizontal, 6) : nil)
                    .accessibilityLabel(d.title + countLabel(d))
                }
            }
            if !model.chat.sessions.isEmpty {
                Section {
                    ForEach(model.chat.sessions.prefix(8)) { s in
                        Button { Task { await model.openConversation(s.id) } } label: {
                            Text(s.title.isEmpty ? "New conversation" : s.title).lineLimit(1).foregroundStyle(theme.ink2)
                        }
                        .buttonStyle(.plain)
                    }
                } header: { SectionLabel("Conversations") }
            }
        }
        .listStyle(.sidebar)
        .scrollContentBackground(.hidden)
        .background(reduceTransparency ? AnyShapeStyle(theme.sidebar) : AnyShapeStyle(theme.sidebar.opacity(0.88)))
        .safeAreaInset(edge: .top, spacing: 0) { header }
        .safeAreaInset(edge: .bottom, spacing: 0) { footer }
    }

    private var header: some View {
        Button {
            model.openSettings(.heron)
            openSettings()
        } label: {
            HStack(spacing: Spacing.s3) {
                AvatarView(style: model.preferences.avatarStyle, state: model.avatar.state, name: model.agentName, size: 36,
                           image: model.preferences.avatarStyle == .image ? NSImage(contentsOf: DisplayPreferences.avatarImageURL) : nil)
                VStack(alignment: .leading, spacing: 1) {
                    Text(model.agentName).font(HeronFont.title3).foregroundStyle(theme.ink)
                    Text(model.statusLine).font(HeronFont.sub).foregroundStyle(model.avatar.state == .awaitingApproval ? theme.waiting : theme.ink2).lineLimit(1)
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, Spacing.s4).padding(.vertical, Spacing.s3)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(AvatarLook.accessibilityLabel(name: model.agentName, state: model.avatar.state))
        .accessibilityHint("Opens Settings for \(model.agentName)")
    }

    private var footer: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: Spacing.s2) {
                Circle().fill(theme.ok).frame(width: 7, height: 7).accessibilityHidden(true)
                Text(model.state?.model ?? "No model").font(HeronFont.sub).foregroundStyle(theme.ink)
            }
            Text(model.footerLine).font(HeronFont.sub).foregroundStyle(theme.ink3)
        }
        .padding(Spacing.s4).frame(maxWidth: .infinity, alignment: .leading)
        .overlay(alignment: .top) { Rectangle().fill(theme.hairline).frame(height: 1) }
    }

    @ViewBuilder private func count(for d: Destination) -> some View {
        switch d {
        case .inbox:
            if model.waitingCount > 0 {
                Text("\(model.waitingCount)").font(HeronFont.subLabel).monospacedDigit().foregroundStyle(theme.waiting)
                    .padding(.horizontal, 6).padding(.vertical, 1)
                    .background(Capsule().fill(theme.waitingSoft))
            }
        case .memory:
            if let n = model.state?.memories, n > 0 { plain(n) }
        case .tasks:
            if model.tasks.openCount > 0 { plain(model.tasks.openCount) }
        default: EmptyView()
        }
    }

    private func plain(_ n: Int) -> some View { Text("\(n)").font(HeronFont.sub).monospacedDigit().foregroundStyle(theme.ink3) }

    private func countLabel(_ d: Destination) -> String {
        switch d {
        case .inbox: model.waitingCount > 0 ? ", \(model.waitingCount) waiting for you" : ""
        case .memory: model.state.map { ", \($0.memories)" } ?? ""
        case .tasks: model.tasks.openCount > 0 ? ", \(model.tasks.openCount)" : ""
        default: ""
        }
    }
}

// MARK: Commands

/// ⌘1 to ⌘7 for the destinations, ⌘N for a new conversation, ⌘+ and ⌘− for text size.
public struct HeronCommands: Commands {
    let model: AppModel

    public init(model: AppModel) { self.model = model }

    public var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button("New Conversation") { model.newConversation() }.keyboardShortcut("n", modifiers: .command)
        }
        CommandMenu("Go") {
            ForEach(Destination.allCases) { d in
                Button(d.title) { model.go(d) }
                    .keyboardShortcut(KeyEquivalent(Character("\(d.shortcutDigit)")), modifiers: .command)
            }
        }
        CommandGroup(after: .toolbar) {
            Button("Larger Text") { model.preferences.textSize = model.preferences.textSize.larger() }
                .keyboardShortcut("+", modifiers: .command)
            Button("Smaller Text") { model.preferences.textSize = model.preferences.textSize.smaller() }
                .keyboardShortcut("-", modifiers: .command)
        }
    }
}
