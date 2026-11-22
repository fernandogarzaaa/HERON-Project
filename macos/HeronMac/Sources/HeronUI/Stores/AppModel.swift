import Foundation
import AppKit
import Observation
import HeronCore

public enum Destination: String, CaseIterable, Sendable, Identifiable, Hashable {
    case today, chat, inbox, memory, tasks, schedules, activity
    /// F3's screen. It is not in `allCases`: the sidebar and the Go menu add it only while "Runs and notifications" is on.
    case runs
    public static var allCases: [Destination] { [.today, .chat, .inbox, .memory, .tasks, .schedules, .activity] }
    /// What the sidebar and the Go menu list: the seven screens, and Runs while "Runs and notifications" is on.
    public static func visible(runsEnabled: Bool) -> [Destination] {
        var list = allCases
        if runsEnabled { list.append(.runs) }
        return list
    }
    public var id: String { rawValue }
    public var title: String { rawValue.capitalized }
    public var symbol: String {
        switch self {
        case .today: "sun.max"
        case .chat: "bubble.left"
        case .inbox: "tray"
        case .memory: "brain"
        case .tasks: "checklist"
        case .schedules: "clock"
        case .activity: "list.bullet.rectangle"
        case .runs: "play.circle"
        }
    }
    /// ⌘1 to ⌘7, and ⌘8 for Runs.
    public var shortcutDigit: Int { self == .runs ? 8 : (Self.allCases.firstIndex(of: self) ?? 0) + 1 }
}

/// What `CoreProcess` (U4b) reports. U4a only renders it.
public enum CoreStatus: Equatable, Sendable {
    case starting, running
    /// The core exited once and was restarted: shows a dismissable banner.
    case restarted
    /// The core stopped twice within a minute: shows the "Heron's core stopped" card.
    case stopped(details: String)
}

/// The app's root model: the client, the nine screen stores (plus Inbox), preferences, the avatar and
/// navigation. It consumes the event stream and routes each event to the stores.
@MainActor
@Observable
public final class AppModel {
    public let client: any CoreClientProtocol
    public let preferences: DisplayPreferences
    public let avatar: AvatarModel
    public let chat: ChatStore
    public let approvals: ApprovalsStore
    public let inbox: InboxStore
    public let today: TodayStore
    public let memory: MemoryStore
    public let tasks: TasksStore
    public let schedules: SchedulesStore
    public let activity: ActivityStore
    public let settings: SettingsStore
    public let persona: PersonaStore
    public let firstRun: FirstRunStore
    public let features: FeaturesSettingsStore
    public let runs: RunsStore
    public let runNotifier: RunNotifier

    public var selection: Destination = .today
    public var settingsTab: SettingsTab = .general
    public private(set) var state: CoreState?
    /// True until the first bootstrap says the user is onboarded.
    public private(set) var needsFirstRun = true
    public private(set) var hasBootstrapped = false
    /// After an update raised the risk register's version: the notice shows again as a sheet.
    public var showsRiskNoticeSheet = false
    public var coreStatus: CoreStatus = .running
    public var dismissedAnotherHeronBanner = false
    public var dismissedRestartBanner = false
    /// Set by `CoreProcess` in U4b; the "Restart Heron's core" button calls it.
    @ObservationIgnored public var restartCore: (() -> Void)?

    @ObservationIgnored private var eventTask: Task<Void, Never>?
    /// How the stream is opened: `(since, notify)`. `notify` is true while "Runs and notifications" is on.
    @ObservationIgnored private let openEvents: (Int?, Bool) -> AsyncStream<CoreEvent>
    /// The `seq` of the newest event routed, so a restarted stream resumes after it.
    @ObservationIgnored private var lastEventSeq: Int?
    @ObservationIgnored private var defaultsObserver: NSObjectProtocol?
    @ObservationIgnored private var activeObserver: NSObjectProtocol?
    @ObservationIgnored private let now: () -> Date
    @ObservationIgnored private let poster: any NotificationPoster
    /// Whether macOS has refused notifications. Cached so the stream can be opened synchronously; refreshed by
    /// `refreshNotificationAuthorization()`.
    @ObservationIgnored private var notificationsDenied = false

    /// The core hands its notices to the app (`?notify=1`) only while the app can post them: the switch is on and
    /// macOS has not refused. Otherwise the core keeps posting its own, and a schedule's result is not lost.
    private var wantsCoreNotices: Bool { runs.enabled && !notificationsDenied }

    public init(client: any CoreClientProtocol, preferences: DisplayPreferences? = nil,
                userFirstName: String? = nil, now: @escaping () -> Date = Date.init,
                features featuresClient: (any FeaturesClientProtocol)? = nil,
                runs runsClient: (any RunsClientProtocol)? = nil,
                defaults: UserDefaults = .standard,
                poster: any NotificationPoster = SystemNotificationPoster(),
                windowIsFrontmost: @escaping @MainActor () -> Bool = { RunNotifier.mainWindowIsFrontmost() },
                openEvents: ((Int?, Bool) -> AsyncStream<CoreEvent>)? = nil) {
        self.client = client
        self.preferences = preferences ?? DisplayPreferences()
        self.now = now
        self.poster = poster
        avatar = AvatarModel()
        chat = ChatStore(client: client)
        approvals = ApprovalsStore(client: client)
        inbox = InboxStore(client: client, approvals: approvals)
        today = TodayStore(client: client, userFirstName: userFirstName ?? TodayStore.systemFirstName())
        memory = MemoryStore(client: client)
        tasks = TasksStore(client: client, now: now)
        schedules = SchedulesStore(client: client)
        activity = ActivityStore(client: client)
        settings = SettingsStore(client: client)
        persona = PersonaStore(client: client)
        firstRun = FirstRunStore(client: client)
        // The preview client has no core behind it, so its features come from memory; a live client's follow its endpoint.
        let featuresBackend: any FeaturesClientProtocol
        if let featuresClient {
            featuresBackend = featuresClient
        } else if client is PreviewCoreClient {
            featuresBackend = PreviewFeaturesClient()
        } else {
            featuresBackend = FeaturesClient(endpoint: { [client] in client.endpoint })
        }
        let runsBackend: any RunsClientProtocol
        if let runsClient {
            runsBackend = runsClient
        } else if client is PreviewCoreClient {
            runsBackend = PreviewRunsClient()
        } else {
            runsBackend = RunsClient(endpoint: { [client] in client.endpoint })
        }
        // The runs store registers the switch's default (on) first, so the Features list reads it as on.
        let runsStore = RunsStore(client: runsBackend, defaults: defaults)
        runs = runsStore
        features = FeaturesSettingsStore(client: featuresBackend, defaults: defaults, poster: poster)
        runNotifier = RunNotifier(poster: poster, runs: runsBackend, isEnabled: { runsStore.enabled }, windowIsFrontmost: windowIsFrontmost)
        self.openEvents = openEvents ?? { [client] since, notify in
            if notify, let source = client as? any NotifyingEventsClient { return source.events(since: since, notify: true) }
            return client.events(since: since)
        }
        chat.onOtherApproval = { [weak self] event in self?.addToFeed(event) }
        // macOS refusing the first request, or the owner changing it in System Settings, changes what the core is asked.
        runNotifier.onAuthorizationRefused = { [weak self] in Task { await self?.refreshNotificationAuthorization() } }
        activeObserver = NotificationCenter.default.addObserver(forName: NSApplication.didBecomeActiveNotification, object: nil, queue: .main) { [weak self] _ in
            Task { @MainActor in await self?.refreshNotificationAuthorization() }
        }
        // The switch lives in `UserDefaults`, written by the Features list; follow it.
        defaultsObserver = NotificationCenter.default.addObserver(forName: UserDefaults.didChangeNotification, object: defaults, queue: .main) { [weak self] _ in
            Task { @MainActor in self?.applyRunsSetting() }
        }
    }

    // MARK: Derived

    public var agentName: String { persona.persona?.name ?? state?.name ?? "Heron" }
    public var paused: Bool { state?.paused ?? today.summary?.paused ?? false }
    /// `client.endpoint` isn't observable and is only the real endpoint once the core has answered, so the banner waits for `hasBootstrapped`.
    public var showsAnotherHeronBanner: Bool { hasBootstrapped && !client.endpoint.backgroundOwner && !dismissedAnotherHeronBanner }

    /// Items waiting for the user: live approvals (including the open conversation's), parked ones, and unread suggestions are separate.
    public var waitingCount: Int { approvals.waitingCount + (chat.liveApproval == nil ? 0 : 1) }

    /// The sidebar's one-line status: "Ready", "Working on “Collect October receipts”", "Waiting for your approval".
    public var statusLine: String {
        switch avatar.state {
        case .thinking, .speaking:
            let title = chat.title
            return chat.currentSessionID == nil ? "Working…" : "Working on “\(title)”"
        default: return avatar.state.statusLine
        }
    }

    /// "Sandbox on · Ask first", from `/api/state`.
    public var footerLine: String {
        let mode = state?.mode.title ?? PermissionMode.ask.title
        let sandbox = state?.sandboxOn == false ? "Sandbox off" : "Sandbox on"
        return "\(sandbox) · \(mode)"
    }

    // MARK: Start

    public func bootstrap() async {
        async let loadedPersona: Void = persona.load()
        do {
            state = try await client.state()
            needsFirstRun = !(state?.onboarded ?? false)
            showsRiskNoticeSheet = !needsFirstRun && !(state?.riskNoticeAccepted ?? true)
        } catch {
            needsFirstRun = false
        }
        await loadedPersona
        hasBootstrapped = true
        if needsFirstRun { await firstRun.load() } else { await loadAll() }
        await refreshNotificationAuthorization()
        startEvents()
    }

    public func finishFirstRun() async {
        await firstRun.finish(persona: persona)
        needsFirstRun = false
        if firstRun.wantsAPIKey { settingsTab = .model }
        selection = .today
        await refreshState()
        await loadAll()
    }

    public func loadAll() async {
        await refreshState()
        async let a: Void = chat.loadSessions()
        async let b: Void = today.load()
        async let c: Void = inbox.load()
        async let d: Void = memory.load()
        async let e: Void = tasks.load()
        async let f: Void = schedules.load()
        async let g: Void = loadRunsIfEnabled()
        _ = await (a, b, c, d, e, f, g)
    }

    public func refreshState() async {
        if let s = try? await client.state() { state = s }
    }

    /// - Parameter resumingAfter: the `seq` to resume from when reopening a stream; nil on a first start.
    public func startEvents(resumingAfter seq: Int? = nil) {
        guard eventTask == nil else { return }
        // With "Runs and notifications" on and macOS not refusing, the stream asks the core to hand over its notices (`?notify=1`).
        let stream = openEvents(seq, wantsCoreNotices)
        eventTask = Task { [weak self] in
            for await event in stream {
                guard let self else { return }
                self.route(event)
            }
        }
    }

    public func stopEvents() { eventTask?.cancel(); eventTask = nil }

    /// Sends one event to the avatar and the stores. Public so tests and U4b's replay can drive it directly.
    public func route(_ event: CoreEvent) {
        if event.seq > 0 { lastEventSeq = max(lastEventSeq ?? 0, event.seq) }
        runs.handle(event)
        let notifier = runNotifier
        Task { await notifier.handle(event) }
        avatar.handle(event)
        switch event.kind {
        case .approval:
            if selection == .chat, chat.isStartingTurn || event.sessionID == chat.currentSessionID {
                // A held approval that turns out not to be this conversation's comes back through `onOtherApproval`.
                chat.handle(event)
            } else {
                addToFeed(event)
            }
        case .resync:
            Task { await resynchronize() }
        case .runEnd, .approvalParked:
            chat.handle(event)
            approvals.handle(event)
            Task { await refreshState(); await today.load() }
        default:
            chat.handle(event)
            approvals.handle(event)
        }
    }

    private func addToFeed(_ event: CoreEvent) {
        guard case .approval(var request) = event.kind else { return }
        request.sessionID = request.sessionID ?? event.sessionID
        approvals.add(request)
    }

    /// After `resync`: reloads the open conversation and the parked approvals, then the approvals runs are
    /// waiting on now, since the events that announced them are not replayed.
    public func resynchronize() async {
        if let id = chat.currentSessionID { await chat.open(id: id) }
        await approvals.load()
        guard let live = try? await client.liveApprovals() else { return }
        var others: [ApprovalRequest] = []
        for request in live {
            if selection == .chat, request.sessionID == chat.currentSessionID { chat.restore(liveApproval: request) } else { others.append(request) }
        }
        approvals.replaceLive(with: others)
    }

    // MARK: Actions

    public func go(_ destination: Destination) {
        selection = destination
        Task { await load(destination) }
    }

    public func load(_ destination: Destination) async {
        switch destination {
        case .today: await today.load()
        case .chat: await chat.loadSessions()
        case .inbox: await inbox.load()
        case .memory: await memory.load()
        case .tasks: await tasks.load()
        case .schedules: await schedules.load()
        case .activity: await activity.load()
        case .runs: await runs.load()
        }
    }

    // MARK: Runs and notifications (F3)

    private func loadRunsIfEnabled() async {
        if runs.enabled { await runs.load() }
    }

    /// Reads whether macOS has refused notifications. When that changes what the core should be asked, the event
    /// stream is reopened (with `?notify=1` only while the app can post the core's notices).
    public func refreshNotificationAuthorization() async {
        let denied = await poster.isDenied()
        guard denied != notificationsDenied else { return }
        let before = wantsCoreNotices
        notificationsDenied = denied
        if wantsCoreNotices != before { reopenEvents() }
    }

    private func reopenEvents() {
        guard eventTask != nil else { return }
        stopEvents()
        startEvents(resumingAfter: lastEventSeq)
    }

    /// Follows the "Runs and notifications" switch. When it changes, the event stream is reopened, with `?notify=1`
    /// while it is on and without it while it is off, so the core knows whether to keep posting its own notices.
    public func applyRunsSetting() {
        guard runs.refreshEnabled() else { return }
        reopenEvents()
        if runs.enabled {
            // Turning the switch on may have just asked macOS for permission.
            Task { await refreshNotificationAuthorization(); await runs.load() }
        } else if selection == .runs {
            selection = .today
        }
    }

    /// A row of the Runs screen: the approval when the run is waiting on one, otherwise the conversation.
    public func open(run: RunRecord) {
        if run.isWaiting, let request = approvals.live.first(where: { $0.sessionID == run.sessionID }) {
            review(live: request)
            return
        }
        Task { await openConversation(run.sessionID) }
    }

    /// A click on a notice the app posted. The identifier says what it was about.
    public func openFromNotification(identifier: String) async {
        switch RunNoticeTarget.parse(identifier) {
        case .approval(let id):
            if let request = approvals.live.first(where: { $0.id == id }) {
                review(live: request)
            } else if chat.liveApproval?.id == id {
                selection = .chat
            } else {
                // Answered already, or parked: the list shows where the run stands.
                go(.runs)
            }
        case .run(let id):
            if !runs.runs.contains(where: { $0.runID == id }) { await runs.load() }
            if let record = runs.runs.first(where: { $0.runID == id }) {
                await openConversation(record.sessionID)
            } else {
                go(.runs)
            }
        case .none:
            break
        }
    }

    public func newConversation() {
        chat.startNewConversation()
        selection = .chat
    }

    /// "New schedule": the core creates schedules from a described request, so this opens a new
    /// conversation with a starter sentence in the composer (DESIGN-CHOICES.md).
    public func newSchedule() {
        chat.startNewConversation()
        chat.composer = "Every weekday at 7:30, brief me on my day."
        selection = .chat
    }

    public func openConversation(_ id: String) async {
        selection = .chat
        await chat.open(id: id)
    }

    /// "Prep my day": a new conversation titled "Prep · Tue 18 Nov" that sends the D6 prompt.
    public func prepMyDay() async {
        chat.startNewConversation()
        selection = .chat
        await chat.send(TodayStore.prepPrompt, title: TodayStore.prepTitle(now: now()))
    }

    public func setPaused(_ paused: Bool) async {
        try? await client.setPaused(paused)
        await refreshState()
        await today.load()
    }

    public func setMode(_ mode: PermissionMode) async {
        try? await client.setMode(mode)
        await refreshState()
    }

    /// "Review" in the menu bar: opens the Inbox on that item.
    public func review(parked id: Int) {
        selection = .inbox
        inbox.selection = .parked(id)
        Task { await inbox.load() }
    }

    public func review(live request: ApprovalRequest) {
        selection = .inbox
        inbox.selection = .live(request.id)
    }

    public func openSettings(_ tab: SettingsTab) { settingsTab = tab }

    /// Accept from the sheet shown after an update raised the register's version.
    public func acceptUpdatedRiskNotice() async {
        await firstRun.load()
        await firstRun.accept()
        if firstRun.noticeUpdated == false && firstRun.step == .model {
            showsRiskNoticeSheet = false
            firstRun.step = .risks
            await refreshState()
        }
    }
}
