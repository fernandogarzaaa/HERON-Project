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

    public static func from(message: String, code: String?, provider: String?, version: String?) -> ChatErrorCard {
        switch code {
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
            if message.lowercased().contains("logged out") || message.lowercased().contains("not logged in") {
                let cli = provider ?? "Claude Code"
                return ChatErrorCard(title: "\(cli) is logged out",
                                     text: "Run `claude auth login` in Terminal, then send your message again.",
                                     actions: [.copyCommand("claude auth login"), .tryAgain])
            }
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
    public private(set) var currentSessionID: String?
    public private(set) var messages: [ChatMessage] = []
    public var composer = ""
    public private(set) var isRunning = false
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

    public func open(id: String) async {
        loadState = .loading
        do {
            let detail = try await client.session(id: id)
            currentSessionID = id
            messages = detail.messages.map { m in
                ChatMessage(id: "m\(m.id)", role: m.role == "user" ? .user : .assistant, text: m.content,
                            steps: m.toolCalls.map { ChatStep(id: $0.id, name: $0.name, sentence: $0.summary,
                                                              status: $0.ok == false ? .failed : .done, durationMs: $0.durationMs, untrusted: $0.untrusted) },
                            finished: true, error: nil)
            }
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

    /// Esc stops the current reply.
    public func stop() async {
        guard let id = currentSessionID, isRunning else { return }
        _ = try? await client.cancel(sessionID: id)
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
        switch event.kind {
        case .session(let id, _):
            if currentSessionID == nil { currentSessionID = id }
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
        case .done(let text, _):
            mutateAssistant { m in
                if m.text.isEmpty { m.text = text }
                m.finished = true
            }
        case .cancelled:
            mutateAssistant { $0.finished = true }
        case .error(let message, let code, let provider, let version):
            let card = ChatErrorCard.from(message: message, code: code, provider: provider, version: version)
            mutateAssistant { $0.error = card; $0.finished = true }
            if code == "risk_notice_not_accepted" { needsRiskNotice = true }
        case .runEnd:
            isRunning = false
            liveApproval = nil
            mutateAssistant { $0.finished = true }
            Task { await loadSessions() }
        case .usage, .model, .notice, .learned, .resync:
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
