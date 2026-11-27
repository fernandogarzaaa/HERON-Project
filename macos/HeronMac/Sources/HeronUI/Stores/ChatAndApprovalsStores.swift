import Foundation
import Observation
import HeronCore

/// How a store's data is loading. Lists show skeleton rows only after 300 ms (the view decides),
/// so the store just reports `loading`.
public enum LoadState: Equatable, Sendable {
    case idle, loading, loaded
    case failed(String)
}

func describe(_ error: Error) -> String {
    switch error {
    case CoreError.failed(let m): return m
    case CoreError.notFound: return "That item no longer exists."
    case CoreError.sessionBusy: return "Heron is still working on this conversation."
    case CoreError.riskNoticeNotAccepted: return "Accept the risk notice first."
    case CoreError.riskNoticeOutdated: return "The risk notice was updated."
    case CoreError.coreStopped: return "Heron's core isn't running."
    default: return error.localizedDescription
    }
}

// MARK: Chat

public struct ChatStep: Identifiable, Equatable, Sendable {
    public enum Status: Equatable, Sendable { case running, done, failed, waiting, movedToInbox }
    public var id: String
    public var name: String
    public var sentence: String
    public var status: Status
    public var durationMs: Int?
    /// Read outside content: shows the violet "Outside content" tag.
    public var untrusted: Bool
}

public enum ChatErrorAction: Equatable, Sendable {
    case copyCommand(String)
    case tryAgain
    case continueUntested(provider: String, version: String)
    case modelSettings
    case stopReply
    case chooseFolder
    case showCause
}

/// An error under a reply: a `dangerSoft` card with a title, text and actions. The text comes from the
/// event's `code`, never a generic "Something went wrong".
public struct ChatErrorCard: Equatable, Sendable {
    public var title: String
    public var text: String
    public var actions: [ChatErrorAction]

    /// Under a reply the core never finished (`interrupted`): it was stopped, ended in an error, or the core
    /// restarted while it ran. The core can't tell which, so neither does the card.
    public static let cutOff = ChatErrorCard(title: "This reply didn't finish",
                                             text: "It was stopped, or Heron's core restarted before it was done. Send your message again to carry on.",
                                             actions: [.tryAgain])

    /// The core is down and starting it again for this message failed. The details stay on the
    /// "Heron's core stopped" card, behind its Copy button.
    public static let coreStopped = ChatErrorCard(title: "Heron's core isn't running",
                                                  text: "It stopped and didn't start again. Your message is kept in the box. Try again, or restart the core from the card at the top of the window.",
                                                  actions: [.tryAgain])

    /// The card for a failed reply. The core says what failed in `code`; the message text is shown, never
    /// searched, so a hint inside it ("If OpenCode is logged out: …") can't turn into the wrong card.
    /// `login` is only ever a command that runs as is; `hint` says how to sign in, in words.
    public static func from(message: String, code: String?, provider: String?, version: String?, login: String? = nil, hint: String? = nil) -> ChatErrorCard {
        switch code {
        case "cli_logged_out":
            let cli = provider ?? "The model's CLI"
            guard let login, !login.isEmpty else {
                let how = hint.map { $0.trimmingCharacters(in: .whitespaces) }.flatMap { $0.isEmpty ? nil : $0 }
                let text = how.map { "To sign in: \($0). Then send your message again, or pick another model in Settings." }
                    ?? "Sign in to it in Terminal, then send your message again, or pick another model in Settings."
                return ChatErrorCard(title: "\(cli) is logged out", text: text, actions: [.modelSettings, .tryAgain])
            }
            return ChatErrorCard(title: "\(cli) is logged out",
                                 text: "Run `\(login)` in Terminal, then send your message again.",
                                 actions: [.copyCommand(login), .tryAgain])
        case "cli_version_untested":
            let v = version ?? "this version", p = provider ?? "the CLI"
            return ChatErrorCard(title: "Untested version \(v) of \(p)",
                                 text: "Heron hasn't tested \(v). Nothing ran. Continue with \(v), or go back to a tested version.",
                                 actions: [.continueUntested(provider: p, version: v), .modelSettings])
        case "workspace_refused":
            return ChatErrorCard(title: "Heron can't work in that folder",
                                 text: "That folder holds secrets, so Heron refuses it in every mode. Choose another working folder.",
                                 actions: [.chooseFolder])
        case "risk_notice_not_accepted":
            return ChatErrorCard(title: "Read the risk notice first",
                                 text: "Heron starts nothing until you accept the current risk notice.", actions: [])
        case "session_busy":
            return ChatErrorCard(title: "Heron is still working on this conversation",
                                 text: "Your message is kept in the box. Send it when Heron finishes, or stop the current reply.",
                                 actions: [.stopReply])
        default:
            return ChatErrorCard(title: message, text: "Heron stopped this reply.", actions: [.tryAgain])
        }
    }
}

public struct ChatMessage: Identifiable, Equatable, Sendable {
    public enum Role: Equatable, Sendable { case user, assistant }
    public var id: String
    public var role: Role
    public var text: String
    public var steps: [ChatStep]
    public var finished: Bool
    public var error: ChatErrorCard?
}

@MainActor
@Observable
public final class ChatStore {
    public let client: any CoreClientProtocol
    @ObservationIgnored private let now: () -> Date

    public private(set) var sessions: [SessionSummary] = []
    /// Shown under "Archived" in the sidebar once opened.
    public private(set) var archivedSessions: [SessionSummary] = []
    public private(set) var showsArchived = false
    /// The last sidebar action that failed (rename, pin, archive, delete), shown under the list.
    public private(set) var sessionActionError: String?
    public private(set) var currentSessionID: String?
    public private(set) var messages: [ChatMessage] = []
    public var composer = ""
    public private(set) var isRunning = false
    /// Between Stop and the core's answer: the Stop button is disabled so a second click sends nothing more.
    public private(set) var isStopping = false
    /// The live approval for the open conversation: shows the sheet.
    public private(set) var liveApproval: ApprovalRequest?
    /// What outside content the conversation has read, once known. Non-nil means the violet chip shows.
    public private(set) var outsideSource: String?
    public private(set) var isOutsideContent = false
    /// "Answered from your phone", shown for 3 seconds after an approval was answered elsewhere.
    public private(set) var answeredElsewhere = false
    /// A composer-level error (busy, risk notice).
    public private(set) var composerError: ChatErrorCard?
    public private(set) var needsRiskNotice = false
    public private(set) var loadState: LoadState = .idle

    private static let approvalStepPrefix = "approval-"
    @ObservationIgnored private var awaitingStart = false
    /// The run this conversation is waiting on, once known. The end of an earlier run (its `run_end` comes
    /// after the core has learned from it, which can be well into the next turn) must not end this one.
    @ObservationIgnored private var currentRunID: String?
    /// Events that arrived while `sendMessage` was in flight. The core starts the run before it answers
    /// `/api/chat`, so a fast reply can come before the turn's messages exist; they are replayed in order.
    @ObservationIgnored private var heldEvents: [CoreEvent] = []
    /// Where an approval goes when a held event turns out to belong to another conversation (the Inbox feed).
    @ObservationIgnored public var onOtherApproval: ((CoreEvent) -> Void)?
    @ObservationIgnored private var stepStarts: [String: Date] = [:]
    @ObservationIgnored private var locallyAnswered: Set<String> = []

    public init(client: any CoreClientProtocol, now: @escaping () -> Date = Date.init) {
        self.client = client
        self.now = now
    }

    public var title: String { sessions.first { $0.id == currentSessionID }?.title ?? "New conversation" }
    public var canSend: Bool { !composer.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !isRunning && liveApproval == nil }

    /// The composer's placeholder while a run waits for an answer.
    public var composerPlaceholder: String {
        liveApproval != nil ? "Heron is waiting for your answer above." : "Ask Heron anything"
    }

    /// "Thinking. Esc stops Heron." until a step or text shows, then the latest step ("Reading your calendar…").
    public var runStatusLine: String? {
        guard isRunning, let last = messages.last, last.role == .assistant, last.text.isEmpty else { return nil }
        if let step = last.steps.last(where: { $0.status == .running }) { return step.sentence }
        return "Thinking. Esc stops Heron."
    }

    public func loadSessions() async {
        do { sessions = try await client.sessions() } catch { loadState = .failed(describe(error)) }
    }

    // MARK: Sidebar actions

    public func toggleArchived() async {
        showsArchived.toggle()
        if showsArchived { await loadArchived() }
    }

    public func loadArchived() async {
        do { archivedSessions = try await client.archivedSessions() } catch { sessionActionError = describe(error) }
    }

    /// Returns false, with the reason in `sessionActionError`, when the name was refused.
    @discardableResult
    public func rename(_ id: String, to title: String) async -> Bool {
        let name = title.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { sessionActionError = "A conversation needs a name."; return false }
        return await updateSession { try await $0.updateSession(id: id, title: name, pinned: nil, archived: nil) }
    }

    public func setPinned(_ id: String, _ pinned: Bool) async {
        await updateSession { try await $0.updateSession(id: id, title: nil, pinned: pinned, archived: nil) }
    }

    public func setArchived(_ id: String, _ archived: Bool) async {
        await updateSession { try await $0.updateSession(id: id, title: nil, pinned: nil, archived: archived) }
    }

    /// Deletes the conversation for good (the sidebar asks first). The open conversation gives way to a new one.
    public func delete(_ id: String) async {
        await updateSession { try await $0.deleteSession(id: id) }
        if id == currentSessionID, !sessions.contains(where: { $0.id == id }), sessionActionError == nil { startNewConversation() }
    }

    @discardableResult
    private func updateSession(_ change: (any CoreClientProtocol) async throws -> Void) async -> Bool {
        do {
            try await change(client)
            sessionActionError = nil
        } catch {
            sessionActionError = describe(error)
            return false
        }
        await loadSessions()
        if showsArchived { await loadArchived() }
        return true
    }

    public func open(id: String) async {
        loadState = .loading
        do {
            let detail = try await client.session(id: id)
            currentSessionID = id
            currentRunID = nil
            var loaded = detail.messages.map { m in
                ChatMessage(id: "m\(m.id)", role: m.role == "user" ? .user : .assistant, text: m.content,
                            steps: m.toolCalls.map { ChatStep(id: $0.id, name: $0.name, sentence: $0.summary,
                                                              status: $0.ok == false ? .failed : .done, durationMs: $0.durationMs, untrusted: $0.untrusted) },
                            finished: true, error: nil)
            }
            // The core kept the reply's steps but no run is finishing it (stopped, a restart, a quit mid-reply): say so,
            // so the conversation doesn't look done or stuck.
            if detail.interrupted && !detail.busy {
                if let last = loaded.indices.last, loaded[last].role == .assistant {
                    loaded[last].error = .cutOff
                } else {
                    loaded.append(ChatMessage(id: "cut-\(loaded.count)", role: .assistant, text: "", steps: [], finished: true, error: .cutOff))
                }
            }
            messages = loaded
            isRunning = detail.busy
            isOutsideContent = detail.tainted
            outsideSource = nil
            liveApproval = nil
            composerError = nil
            loadState = .loaded
        } catch {
            loadState = .failed(describe(error))
        }
    }

    public func startNewConversation() {
        currentSessionID = nil
        currentRunID = nil
        messages = []
        isRunning = false
        liveApproval = nil
        isOutsideContent = false
        outsideSource = nil
        composerError = nil
        composer = ""
    }

    /// Sends the composer text (or `text` when given, as "Prep my day" does).
    public func send(_ text: String? = nil, title: String? = nil) async {
        let body = (text ?? composer).trimmingCharacters(in: .whitespacesAndNewlines)
        guard !body.isEmpty else { return }
        composerError = nil
        awaitingStart = true
        do {
            let start = try await client.sendMessage(sessionID: currentSessionID, message: body, title: title)
            currentSessionID = start.sessionID
            currentRunID = start.runID
            messages.append(ChatMessage(id: "u-\(messages.count)", role: .user, text: body, steps: [], finished: true, error: nil))
            messages.append(ChatMessage(id: "a-\(messages.count)", role: .assistant, text: "", steps: [], finished: false, error: nil))
            isRunning = true
            if text == nil { composer = "" }
            replayHeldEvents()
            await loadSessions()
        } catch CoreError.sessionBusy {
            composerError = ChatErrorCard.from(message: "", code: "session_busy", provider: nil, version: nil)
        } catch CoreError.riskNoticeNotAccepted {
            needsRiskNotice = true
        } catch CoreError.coreStopped {
            coreWentAway()
            composerError = .coreStopped
        } catch {
            composerError = ChatErrorCard(title: describe(error), text: "Your message is kept in the box.", actions: [.tryAgain])
        }
        replayHeldEvents()
    }

    /// Ends the send's wait and handles what arrived meanwhile, in order. Called once on every path.
    private func replayHeldEvents() {
        guard awaitingStart else { return }
        awaitingStart = false
        let held = heldEvents
        heldEvents = []
        for event in held {
            if accepts(event) { handle(event) } else if case .approval = event.kind { onOtherApproval?(event) }
        }
    }

    /// Puts back a live approval for the open conversation after a resync: the original event is not replayed.
    public func restore(liveApproval request: ApprovalRequest) {
        guard request.sessionID == currentSessionID, currentSessionID != nil else { return }
        liveApproval = request
        isRunning = true
        if request.tainted { isOutsideContent = true; outsideSource = request.outsideSource ?? outsideSource }
    }

    public func clearRiskNoticeRequest() { needsRiskNotice = false }

    /// "Try again" on a card: sends what is kept in the box, or else the last message again.
    public func retry() async {
        if !composer.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            await send()
        } else if let last = messages.last(where: { $0.role == .user })?.text {
            await send(last)
        }
    }

    /// The Stop button and Esc. The core ends the run and sends `cancelled` and `run_end`. When it answers that
    /// nothing is running here (the run ended, or a restarted core never had it), the reply is over: the
    /// composer is freed rather than left on a Stop button that does nothing.
    public func stop() async {
        guard let id = currentSessionID, isRunning, !isStopping else { return }
        isStopping = true
        defer { isStopping = false }
        do {
            if try await client.cancel(sessionID: id) == false { endRun() }
        } catch CoreError.coreStopped {
            coreWentAway() // nothing is left to stop
        } catch {
            composerError = ChatErrorCard(title: "Heron couldn't stop the reply", text: describe(error), actions: [.stopReply])
        }
    }

    /// The core restarted or stopped: a reply it was running is gone and sends nothing more. Without this the
    /// conversation stays "running" (Stop instead of Send, the box disabled under a dead approval) until a
    /// resync that a stopped core never sends.
    public func coreWentAway() {
        guard isRunning || liveApproval != nil else { return }
        mutateAssistant { m in if !m.finished { m.error = .cutOff } }
        endRun()
    }

    /// The reply is over: the composer is free and the reply shows as finished.
    private func endRun() {
        isRunning = false
        liveApproval = nil
        mutateAssistant { $0.finished = true }
    }

    /// Answers the open sheet. `scope` is once, session or always; `deny` refuses.
    public func answer(_ decision: ApprovalDecision) async {
        guard let request = liveApproval else { return }
        locallyAnswered.insert(request.id)
        do {
            try await client.approve(id: request.id, decision: decision)
            if liveApproval?.id == request.id { liveApproval = nil }
        } catch {
            locallyAnswered.remove(request.id)
            composerError = ChatErrorCard(title: describe(error), text: "Heron didn't get your answer.", actions: [.tryAgain])
        }
    }

    public func clearAnsweredElsewhere() { answeredElsewhere = false }

    /// True when `event` belongs to the open conversation.
    public func accepts(_ event: CoreEvent) -> Bool {
        if let sid = event.sessionID, sid == currentSessionID { return true }
        return false
    }

    /// True while a send waits for the core's answer: conversation events are held until it comes.
    public var isStartingTurn: Bool { awaitingStart }

    public func handle(_ event: CoreEvent) {
        if awaitingStart, event.sessionID != nil { heldEvents.append(event); return }
        guard accepts(event) else { return }
        if event.isRunEnding, let run = event.runID, let current = currentRunID, run != current { return }
        switch event.kind {
        case .session(let id, let runID):
            if currentSessionID == nil { currentSessionID = id }
            currentRunID = runID
            isRunning = true
        case .text(let chunk):
            mutateAssistant { $0.text += chunk }
        case .toolStart(let id, let name, let summary, _):
            stepStarts[id] = now()
            mutateAssistant { $0.steps.append(ChatStep(id: id, name: name, sentence: summary, status: .running, durationMs: nil, untrusted: false)) }
        case .toolEnd(let id, _, let ok, let preview):
            let started = stepStarts.removeValue(forKey: id)
            let ms = started.map { Int(now().timeIntervalSince($0) * 1000) }
            mutateAssistant { m in
                guard let i = m.steps.firstIndex(where: { $0.id == id }) else { return }
                m.steps[i].status = ok ? .done : .failed
                m.steps[i].durationMs = ms
                if !preview.isEmpty && ok { m.steps[i].sentence = m.steps[i].sentence.hasSuffix("…") ? preview : m.steps[i].sentence }
            }
        case .approval(let request):
            liveApproval = request
            if request.tainted { isOutsideContent = true; outsideSource = request.outsideSource ?? outsideSource }
            mutateAssistant { m in
                if let i = m.steps.lastIndex(where: { $0.status == .running }) {
                    m.steps[i].status = .waiting
                    m.steps[i].sentence = "Waiting for you: \(request.summary)"
                } else {
                    // The approval came before its tool_start: show the wait as its own step.
                    m.steps.append(ChatStep(id: Self.approvalStepPrefix + request.id, name: request.tool,
                                            sentence: "Waiting for you: \(request.summary)", status: .waiting, durationMs: nil, untrusted: false))
                }
            }
        case .approvalResolved(let id, _):
            if liveApproval?.id == id {
                liveApproval = nil
                if !locallyAnswered.contains(id) { answeredElsewhere = true }
            }
            locallyAnswered.remove(id)
            mutateAssistant { m in
                guard let i = m.steps.lastIndex(where: { $0.status == .waiting }) else { return }
                // A step made for the approval gives way to the real tool_start; a tool's own step carries on.
                if m.steps[i].id.hasPrefix(Self.approvalStepPrefix) { m.steps.remove(at: i) } else { m.steps[i].status = .running }
            }
        case .approvalParked(let id, _):
            if liveApproval?.id == id { liveApproval = nil }
            mutateAssistant { m in
                if let i = m.steps.lastIndex(where: { $0.status == .waiting }) {
                    m.steps[i].status = .movedToInbox
                    m.steps[i].sentence = "Moved to your Inbox"
                }
            }
        // `done`, `error` and `cancelled` come once the core has freed the conversation: it takes the next
        // message from here, even while it still learns from this exchange before `run_end`.
        case .done(let text, _):
            mutateAssistant { m in
                if m.text.isEmpty { m.text = text }
            }
            endRun()
        case .cancelled:
            endRun()
        case .error(let message, let code, let provider, let version, let login, let hint):
            let card = ChatErrorCard.from(message: message, code: code, provider: provider, version: version, login: login, hint: hint)
            mutateAssistant { $0.error = card }
            endRun()
            if code == "risk_notice_not_accepted" { needsRiskNotice = true }
        case .runEnd:
            endRun()
            Task { await loadSessions() }
        case .usage, .model, .notice, .notification, .learned, .resync:
            break
        }
    }

    private func mutateAssistant(_ body: (inout ChatMessage) -> Void) {
        guard let i = messages.lastIndex(where: { $0.role == .assistant }) else { return }
        body(&messages[i])
    }
}

// MARK: Approvals (the feed outside the chat thread)

@MainActor
@Observable
public final class ApprovalsStore {
    public let client: any CoreClientProtocol
    /// Live approvals for conversations other than the open one.
    public private(set) var live: [ApprovalRequest] = []
    /// Parked approvals from schedules and phone turns.
    public private(set) var parked: [ParkedApproval] = []
    public private(set) var loadState: LoadState = .idle
    public private(set) var error: String?

    public init(client: any CoreClientProtocol) { self.client = client }

    /// What the sidebar's gold count and the menu bar's dot show.
    public var waitingCount: Int { live.count + parked.count }

    public func load() async {
        loadState = .loading
        do { parked = try await client.approvals(); loadState = .loaded } catch { loadState = .failed(describe(error)) }
    }

    public func add(_ request: ApprovalRequest) {
        if !live.contains(where: { $0.id == request.id }) { live.append(request) }
    }

    /// After a resync: the live list becomes exactly what the core is waiting on now.
    public func replaceLive(with requests: [ApprovalRequest]) { live = requests }

    public func handle(_ event: CoreEvent) {
        switch event.kind {
        case .approvalResolved(let id, _): live.removeAll { $0.id == id }
        case .approvalParked(let id, _):
            live.removeAll { $0.id == id }
            Task { await load() }
        default: break
        }
    }

    public func answer(_ request: ApprovalRequest, _ decision: ApprovalDecision) async {
        do { try await client.approve(id: request.id, decision: decision); live.removeAll { $0.id == request.id } } catch { self.error = describe(error) }
    }

    /// Approving a parked item runs this one step now; dismissing drops it.
    public func answerParked(_ item: ParkedApproval, _ action: ParkedAction) async {
        do { try await client.answerParked(id: item.id, action: action); parked.removeAll { $0.id == item.id } } catch { self.error = describe(error) }
    }
}
