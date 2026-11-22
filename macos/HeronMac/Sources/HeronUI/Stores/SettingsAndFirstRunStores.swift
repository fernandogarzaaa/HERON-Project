import Foundation
import Observation
import HeronCore

// MARK: Persona

/// The agent's name and personality (`GET` and `POST /api/persona`). Shown in Settings › Heron and in
/// the third first-run step.
@MainActor
@Observable
public final class PersonaStore {
    public let client: any CoreClientProtocol
    public private(set) var persona: Persona?
    public var draftName = ""
    public var draftText = ""
    public private(set) var loadState: LoadState = .idle
    public private(set) var saved = false

    public init(client: any CoreClientProtocol) { self.client = client }

    public var name: String { persona?.name ?? "Heron" }
    public var maxCharacters: Int { persona?.maxCharacters ?? 2000 }
    public var charactersLeft: Int { maxCharacters - draftText.count }
    public var isDirty: Bool { draftName != (persona?.name ?? "") || draftText != (persona?.text ?? "") }
    public var canSave: Bool { isDirty && !draftName.trimmingCharacters(in: .whitespaces).isEmpty && draftName.count <= 40 && charactersLeft >= 0 }

    public func load() async {
        loadState = .loading
        do {
            let p = try await client.persona()
            persona = p
            draftName = p.name
            draftText = p.text
            loadState = .loaded
        } catch { loadState = .failed(describe(error)) }
    }

    @discardableResult
    public func save() async -> Bool {
        guard canSave else { return false }
        do {
            let p = try await client.setPersona(name: draftName.trimmingCharacters(in: .whitespaces), text: draftText)
            persona = p
            draftName = p.name
            draftText = p.text
            saved = true
            return true
        } catch { loadState = .failed(describe(error)); return false }
    }
}

// MARK: Settings

public enum SettingsTab: String, CaseIterable, Sendable, Identifiable {
    case general, heron, appearance, model, permissions, connections, usage
    public var id: String { rawValue }
    public var title: String { rawValue.capitalized }
    public var symbol: String {
        switch self {
        case .general: "gearshape"
        case .heron: "person.crop.circle"
        case .appearance: "paintpalette"
        case .model: "cpu"
        case .permissions: "hand.raised"
        case .connections: "link"
        case .usage: "chart.bar"
        }
    }
}

@MainActor
@Observable
public final class SettingsStore {
    public let client: any CoreClientProtocol
    public private(set) var state: CoreState?
    public private(set) var settings: CoreSettings?
    public private(set) var providers: [ProviderStatus] = []
    public private(set) var grants: [Grant] = []
    /// Every "never" rule (`GET /api/nevers`), listed in Settings › Permissions.
    public private(set) var nevers: [NeverRule] = []
    /// The rule the user asked to remove; the confirm sheet shows while it is set.
    public private(set) var pendingNeverRemoval: NeverRule?
    /// What went wrong loading the list or removing a rule, shown inline in the "Never" section.
    public private(set) var neverError: String?
    public private(set) var isRemovingNever = false
    /// Sources whose connect or disconnect request is in flight (`ConnectionInfo.id`), for the per-row spinner.
    public private(set) var busySources: Set<String> = []
    /// The last connect or disconnect failure per source, in the core's own words, shown inline on that row or its sheet.
    public private(set) var sourceErrors: [String: String] = [:]
    /// The source the user asked to disconnect; the confirm sheet shows while it is set.
    public private(set) var pendingDisconnect: ConnectionInfo?
    /// Google and Microsoft sign-ins (`GET /api/oauth`): each connects Email and Calendar, read-only.
    public private(set) var oauthAccounts: [OAuthAccount] = []
    /// The provider whose sign-in page is open in the browser while Heron waits for it to finish.
    public private(set) var signingIn: OAuthProviderID?
    /// Accounts that just signed in and are being read for the first time.
    public private(set) var syncingAccounts: Set<OAuthProviderID> = []
    /// True after "Use your Mac's Calendar" was refused: the row then offers System Settings.
    public private(set) var localCalendarDenied = false
    public let localCalendar: any LocalCalendarAccess
    /// Why "Use your Mac's Mail" last failed, so the row offers the right System Settings pane.
    public private(set) var localMailProblem: LocalMailProblem?
    public let localMail: any LocalMailAccess
    /// How often the sign-in is checked, and how long Heron waits (the core's sign-in expires after ten minutes).
    public var pollInterval: Duration = .seconds(2)
    public var signInTimeout: Duration = .seconds(600)
    /// nil means the real `Task.sleep`, called inline in `signIn`. Not a stored default closure `{ try await Task.sleep(for: $0) }`:
    /// calling that aborts the macOS 26 runtime ("freed pointer was not the last allocation") when it returns.
    @ObservationIgnored private let sleep: (@Sendable (Duration) async throws -> Void)?
    @ObservationIgnored private var signInTask: Task<Void, Never>?
    public private(set) var usage: UsageReport?
    public private(set) var loadState: LoadState = .idle
    public private(set) var error: String?

    public init(client: any CoreClientProtocol, localCalendar: any LocalCalendarAccess = SystemCalendarAccess(),
                localMail: any LocalMailAccess = SystemMailAccess(),
                sleep: (@Sendable (Duration) async throws -> Void)? = nil) {
        self.client = client
        self.localCalendar = localCalendar
        self.localMail = localMail
        self.sleep = sleep
    }

    public var sandboxOn: Bool { state?.sandboxOn ?? false }
    public var mode: PermissionMode { settings?.mode ?? state?.mode ?? .ask }
    public var ruleNotices: [RuleNotice] { settings?.ruleNotices ?? [] }

    public func load() async {
        loadState = .loading
        do {
            async let s = client.state()
            async let st = client.settings()
            async let p = client.providers()
            async let g = client.grants()
            async let u = client.usage()
            (state, settings, providers, grants, usage) = try await (s, st, p, g, u)
            loadState = .loaded
        } catch { loadState = .failed(describe(error)) }
        if let accounts = try? await client.oauthAccounts() { oauthAccounts = accounts }
        // Its own request: a core that can't list "never" rules still lets the rest of Settings load.
        do { nevers = try await client.nevers(); neverError = nil } catch { neverError = describe(error) }
    }

    public func revoke(_ grant: Grant) async {
        do { try await client.revokeGrant(rule: grant.rule); grants.removeAll { $0.rule == grant.rule } } catch { self.error = describe(error) }
    }

    // MARK: "Never" rules (heron-v1.md item 35h)

    /// Remove asks first: this opens the confirm sheet and changes nothing yet.
    public func askToRemoveNever(_ rule: NeverRule) {
        guard !isRemovingNever else { return }
        pendingNeverRemoval = rule
    }

    public func cancelNeverRemoval() { pendingNeverRemoval = nil }

    /// The confirmed removal: `DELETE /api/nevers`, then the list is read again so the page shows what the core holds.
    /// A failure stays on the page as `neverError`, whether the delete or the refresh failed.
    public func confirmNeverRemoval() async {
        guard let rule = pendingNeverRemoval, !isRemovingNever else { return }
        pendingNeverRemoval = nil
        isRemovingNever = true
        defer { isRemovingNever = false }
        neverError = nil
        var message: String?
        do { try await client.removeNever(rule: rule.rule) } catch { message = describe(error) }
        do { nevers = try await client.nevers() } catch { message = message ?? describe(error) }
        neverError = message
    }

    // MARK: Connections (Settings › Connections, and the first-run sources step)

    public func isBusy(source id: String) -> Bool { busySources.contains(id) }

    public func clearSourceError(_ id: String) { sourceErrors[id] = nil }

    /// Reads the Connections rows again, without the rest of Settings.
    public func reloadConnections() async {
        if let fresh = try? await client.settings() { settings = fresh }
        if let accounts = try? await client.oauthAccounts() { oauthAccounts = accounts }
    }

    // MARK: Sign in with Google or Microsoft

    /// The provider's state, or "unknown, so try it" when the core didn't list it.
    public func account(_ provider: OAuthProviderID) -> OAuthAccount {
        oauthAccounts.first { $0.provider == provider } ?? OAuthAccount(provider: provider, connected: false, clientConfigured: true)
    }

    /// "Sign in with Google": asks the core to start the sign-in, opens the consent page it returns in the system
    /// browser through `open`, then polls until the core has finished the sign-in and reads the account once.
    /// Returns the running task so a caller (or a test) can wait for it. Only one sign-in runs at a time.
    @discardableResult
    public func beginSignIn(_ provider: OAuthProviderID, open: @escaping @MainActor (URL) -> Bool) -> Task<Void, Never> {
        cancelSignIn()
        let task = Task { await signIn(provider, open: open) }
        signInTask = task
        return task
    }

    /// "Cancel" while waiting. If the user finishes in the browser anyway, the account shows up the next time the page loads.
    public func cancelSignIn() {
        signInTask?.cancel()
        signInTask = nil
        signingIn = nil
    }

    private func signIn(_ provider: OAuthProviderID, open: @MainActor (URL) -> Bool) async {
        let id = provider.rawValue
        sourceErrors[id] = nil
        guard account(provider).clientConfigured else { sourceErrors[id] = SourceCopy.signInNotSetUp(provider); return }
        signingIn = provider
        defer { if signingIn == provider { signingIn = nil } }
        let url: URL
        do { url = try await client.startOAuth(provider: provider) } catch { sourceErrors[id] = describe(error); return }
        if Task.isCancelled { return }
        guard open(url) else { sourceErrors[id] = SourceCopy.browserWouldNotOpen; return }
        var waited = Duration.zero
        while !Task.isCancelled {
            do { if let sleep { try await sleep(pollInterval) } else { try await Task.sleep(for: pollInterval) } } catch { return }
            waited += pollInterval
            if let accounts = try? await client.oauthAccounts() {
                oauthAccounts = accounts
                if accounts.first(where: { $0.provider == provider })?.connected == true { break }
            }
            if waited >= signInTimeout { sourceErrors[id] = SourceCopy.signInTimedOut; return }
        }
        if Task.isCancelled { return }
        signingIn = nil
        syncingAccounts.insert(provider)
        defer { syncingAccounts.remove(provider) }
        do { try await client.syncOAuth(provider: provider) } catch {
            sourceErrors[id] = SourceCopy.firstSyncFailed(describe(error))
        }
        await reloadConnections()
    }

    /// Disconnect an account: asks first, like every source.
    public func askToDisconnect(account provider: OAuthProviderID) {
        let row = account(provider)
        guard row.connected, !busySources.contains(provider.rawValue) else { return }
        pendingDisconnect = ConnectionInfo(id: provider.rawValue, name: provider.displayName, connected: true, detail: row.account ?? "")
    }

    // MARK: The Mac's Calendar app

    /// "Use your Mac's Calendar": asks macOS for read access if needed, reads the events and sends them to the core.
    @discardableResult
    public func connectLocalCalendar() async -> Bool {
        let id = "calendar_local"
        guard !busySources.contains(id) else { return false }
        busySources.insert(id)
        sourceErrors[id] = nil
        localCalendarDenied = false
        defer { busySources.remove(id) }
        var access = localCalendar.authorization()
        if access == .notDetermined { access = await localCalendar.requestAccess() }
        guard access == .granted else {
            localCalendarDenied = true
            sourceErrors[id] = SourceCopy.calendarAccessDenied
            return false
        }
        do {
            try await client.connectSource(.localCalendar(events: try await localCalendar.events()))
        } catch {
            sourceErrors[id] = describe(error)
            return false
        }
        await reloadConnections()
        return true
    }

    /// Events change after the first sync, so a connected Mac Calendar is read again when the app starts. Silent:
    /// it never prompts, and a failure leaves what the core already holds.
    public func refreshLocalCalendarIfConnected() async {
        if settings == nil { await reloadConnections() }
        guard settings?.connections.first(where: { $0.id == "calendar_local" })?.connected == true,
              localCalendar.authorization() == .granted,
              let events = try? await localCalendar.events() else { return }
        try? await client.connectSource(.localCalendar(events: events))
        await reloadConnections()
    }

    // MARK: The Mac's Mail app

    /// "Use your Mac's Mail": asks Mail for the recent inbox messages (macOS asks for Automation permission the first
    /// time, and Mail is opened if it isn't) and sends them to the core. The wait happens off the main thread.
    @discardableResult
    public func connectLocalMail() async -> Bool {
        let id = "mail_local"
        guard !busySources.contains(id) else { return false }
        busySources.insert(id)
        sourceErrors[id] = nil
        localMailProblem = nil
        defer { busySources.remove(id) }
        do {
            let messages = try await localMail.messages()
            try await client.connectSource(.localMail(messages: messages))
        } catch let problem as MailAccessError {
            switch problem {
            case .notAuthorized: localMailProblem = .automationDenied; sourceErrors[id] = SourceCopy.mailAccessDenied
            case .noAccounts: localMailProblem = .noAccounts; sourceErrors[id] = SourceCopy.mailHasNoAccounts
            case .notRunning: sourceErrors[id] = SourceCopy.mailWouldNotOpen
            case .failed(let message): sourceErrors[id] = message
            }
            return false
        } catch {
            sourceErrors[id] = describe(error)
            return false
        }
        await reloadConnections()
        return true
    }

    /// Mail changes, so a connected Mac Mail is read again when the app starts, on the Mac Calendar's cadence. Silent:
    /// it only reads a Mail that is already open (Apple Events would open it), never prompts, and a failure leaves
    /// what the core already holds.
    public func refreshLocalMailIfConnected() async {
        if settings == nil { await reloadConnections() }
        guard settings?.connections.first(where: { $0.id == "mail_local" })?.connected == true,
              !busySources.contains("mail_local"), localMail.isMailRunning(),
              let messages = try? await localMail.messages() else { return }
        try? await client.connectSource(.localMail(messages: messages))
        await reloadConnections()
    }

    /// Connects one source: `POST /api/sources/{name}`, then the rows are read again so they show what the core holds.
    /// The email password is in `source` only for this call; nothing here keeps or logs it. A failure becomes
    /// `sourceErrors[id]` and the function returns false.
    @discardableResult
    public func connect(_ source: SourceConnection) async -> Bool {
        let id = source.sourceID
        guard !busySources.contains(id) else { return false }
        busySources.insert(id)
        sourceErrors[id] = nil
        defer { busySources.remove(id) }
        do {
            try await client.connectSource(source)
        } catch {
            sourceErrors[id] = describe(error)
            return false
        }
        await reloadConnections()
        return true
    }

    /// Disconnect asks first: this opens the confirm sheet and changes nothing yet.
    public func askToDisconnect(_ id: String) {
        guard !busySources.contains(id), let row = settings?.connections.first(where: { $0.id == id }) else { return }
        pendingDisconnect = row
    }

    public func cancelDisconnect() { pendingDisconnect = nil }

    public func confirmDisconnect() async {
        guard let row = pendingDisconnect, !busySources.contains(row.id) else { return }
        pendingDisconnect = nil
        busySources.insert(row.id)
        sourceErrors[row.id] = nil
        defer { busySources.remove(row.id) }
        do {
            if let provider = OAuthProviderID(rawValue: row.id) {
                if signingIn == provider { cancelSignIn() }
                try await client.disconnectOAuth(provider: provider)
            } else {
                try await client.disconnectSource(id: row.id)
            }
        } catch { sourceErrors[row.id] = describe(error) }
        await reloadConnections()
    }

    public func setMode(_ mode: PermissionMode) async {
        do { try await client.setMode(mode); settings?.mode = mode; state?.mode = mode } catch { self.error = describe(error) }
    }

    public func select(provider: ProviderStatus) async {
        do { try await client.selectProvider(id: provider.id); await load() } catch { self.error = describe(error) }
    }

    public func acceptVersion(_ provider: ProviderStatus) async {
        guard let version = provider.version else { return }
        do { try await client.acceptProviderVersion(id: provider.id, version: version); await load() } catch { self.error = describe(error) }
    }

    public func setUnattended(_ allowed: Bool) async {
        do { try await client.setUnattendedSubscription(allowed); settings?.allowSubscriptionUnattended = allowed } catch { self.error = describe(error) }
    }

    public func setAwareness(_ enabled: Bool) async {
        do { try await client.setAwareness(enabled); settings?.awareness = enabled } catch { self.error = describe(error) }
    }
}

// MARK: Providers as rows (first run step 2 and Settings › Model)

public enum ProviderRowState: Equatable, Sendable {
    case signedIn, signedOut, untested, notInstalled
}

public extension ProviderStatus {
    var rowState: ProviderRowState {
        if !installed { return .notInstalled }
        if !loggedIn { return .signedOut }
        if !inRange && !versionAccepted { return .untested }
        return .signedIn
    }

    /// Signed out and not installed rows can't be chosen.
    var isSelectable: Bool { rowState == .signedIn || rowState == .untested }

    /// "Version 2.1.4 · tested", "Codex is signed out. Run `codex login` in Terminal, then check again.", ...
    var detailText: String {
        switch rowState {
        case .signedIn:
            return "Version \(version ?? "unknown")" + (inRange ? " · tested" : " · accepted")
        case .signedOut:
            return "\(displayName) is signed out. Run `\(loginCommand)` in Terminal, then check again."
        case .untested:
            let range = testedRange.map { " (tested: \($0))" } ?? ""
            return "Version \(version ?? "unknown") hasn't been tested with Heron\(range)."
        case .notInstalled:
            return "Not installed on this Mac."
        }
    }
}

// MARK: First run

@MainActor
@Observable
public final class FirstRunStore {
    public enum Step: Int, CaseIterable, Sendable {
        case risks = 0, model, sources, personalize
        public var title: String {
            switch self {
            case .risks: "Read the risks"
            case .model: "Connect a model"
            case .sources: "Connect your sources"
            case .personalize: "Make it yours"
            }
        }
    }

    public let client: any CoreClientProtocol
    public var step: Step = .risks
    public private(set) var notice: RiskNotice?
    public private(set) var sandboxOn = true
    public private(set) var providers: [ProviderStatus] = []
    public var accepted = false
    /// "The risk notice was updated. Please read the new version." after a 409.
    public private(set) var noticeUpdated = false
    public var selectedProviderID: String?
    public private(set) var loadState: LoadState = .idle
    public private(set) var isFinished = false
    public private(set) var error: String?
    /// "Use an API key instead": skips the CLI list, and Settings › Model opens after first run.
    public private(set) var wantsAPIKey = false

    public init(client: any CoreClientProtocol) { self.client = client }

    public func useAPIKeyInstead() async {
        wantsAPIKey = true
        step = .sources
    }

    /// "Continue" and "Skip for now" on the sources step: every source is optional, so both go to the same place.
    public func continueFromSources() { step = .personalize }

    /// Starts the steps again for someone who is already set up ("Run setup again" in Settings › General).
    /// Nothing is deleted: `onboarded` stays set, and the risk notice stays accepted if it still is.
    public func restart() {
        step = .risks
        accepted = false
        noticeUpdated = false
        wantsAPIKey = false
        isFinished = false
        error = nil
        selectedProviderID = nil
    }

    public var canAccept: Bool { accepted && notice != nil }

    /// The best signed-in, tested provider, which gets the "Recommended" tag.
    public var recommendedID: String? {
        providers.first { $0.rowState == .signedIn && $0.isActive }?.id ?? providers.first { $0.rowState == .signedIn }?.id
    }

    public func load() async {
        loadState = .loading
        do {
            async let n = client.riskNotice()
            async let s = client.state()
            let (notice, state) = try await (n, s)
            self.notice = notice
            sandboxOn = state.sandboxOn
            loadState = .loaded
        } catch { loadState = .failed(describe(error)) }
    }

    /// Accepting sends the version. A 409 means the register changed: reload, untick, and say so.
    public func accept() async {
        guard let notice, accepted else { return }
        do {
            try await client.acceptRiskNotice(version: notice.version)
            noticeUpdated = false
            step = .model
            await loadProviders()
        } catch CoreError.riskNoticeOutdated {
            accepted = false
            noticeUpdated = true
            self.notice = try? await client.riskNotice()
        } catch { self.error = describe(error) }
    }

    public func loadProviders() async {
        do {
            providers = try await client.providers()
            if selectedProviderID == nil || providers.first(where: { $0.id == selectedProviderID })?.isSelectable != true {
                selectedProviderID = recommendedID
            }
        } catch { self.error = describe(error) }
    }

    /// "Check again" re-reads `/api/providers`.
    public func checkAgain() async { await loadProviders() }

    public func continueFromModel() async {
        if let id = selectedProviderID { try? await client.selectProvider(id: id) }
        step = .sources
    }

    public func finish(persona: PersonaStore) async {
        _ = await persona.save()
        try? await client.setOnboarded()
        isFinished = true
    }
}
