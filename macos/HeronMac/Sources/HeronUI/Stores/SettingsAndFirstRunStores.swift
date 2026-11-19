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
    public private(set) var usage: UsageReport?
    public private(set) var loadState: LoadState = .idle
    public private(set) var error: String?

    public init(client: any CoreClientProtocol) { self.client = client }

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
        case risks = 0, model, personalize
        public var title: String {
            switch self { case .risks: "Read the risks"; case .model: "Connect a model"; case .personalize: "Make it yours" }
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
        step = .personalize
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
        step = .personalize
    }

    public func finish(persona: PersonaStore) async {
        _ = await persona.save()
        try? await client.setOnboarded()
        isFinished = true
    }
}
