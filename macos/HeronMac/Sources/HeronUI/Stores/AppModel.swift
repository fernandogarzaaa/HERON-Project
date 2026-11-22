import Foundation
import Observation
import HeronCore

public enum Destination: String, CaseIterable, Sendable, Identifiable, Hashable {
    case today, chat, inbox, memory, tasks, schedules, activity
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
        }
    }
    /// ⌘1 to ⌘7.
    public var shortcutDigit: Int { (Self.allCases.firstIndex(of: self) ?? 0) + 1 }
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
    @ObservationIgnored private let now: () -> Date

    public init(client: any CoreClientProtocol, preferences: DisplayPreferences? = nil,
                userFirstName: String? = nil, now: @escaping () -> Date = Date.init,
                features featuresClient: (any FeaturesClientProtocol)? = nil) {
        self.client = client
        self.preferences = preferences ?? DisplayPreferences()
        self.now = now
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
        features = FeaturesSettingsStore(client: featuresBackend)
        chat.onOtherApproval = { [weak self] event in self?.addToFeed(event) }
    }

    // MARK: Derived

    public var agentName: String { persona.persona?.name ?? state?.name ?? "Heron" }
    public var paused: Bool { state?.paused ?? today.summary?.paused ?? false }
    /// `client.endpoint` isn't observable and is only the real endpoint once the core has answered, so the banner waits for `hasBootstrapped`.
    public var showsAnotherHeronBanner: Bool { hasBootstrapped && !client.endpoint.backgroundOwner && !dismissedAnotherHeronBanner }

    /// F2: the chip at the top of the chat. It shows for a conversation that has read outside content, unless the
    /// owner switched "Explain approvals" off (or the core hasn't said yet).
    public var showsTaintChip: Bool { chat.isOutsideContent && features.isOn(coreKey: AskReasonsFeature.key) }

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
        async let g: Void = features.load()
        _ = await (a, b, c, d, e, f, g)
    }

    public func refreshState() async {
        if let s = try? await client.state() { state = s }
    }

    public func startEvents() {
        guard eventTask == nil else { return }
        let stream = client.events(since: nil)
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
        case .tainted:
            chat.handle(event)
            // The chip reads the `ask_reasons` key; ask the core again if it hadn't answered when the app started.
            if features.loadState != .loaded { Task { await features.load() } }
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
