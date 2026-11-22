import Foundation
import Observation
import HeronCore

/// The app setting behind F1's row in Settings › Features. On once the feature is built (QF3), so it is
/// registered as a default: `UserDefaults.bool(forKey:)` then reads `true` until the owner turns it off.
public enum QuickEntrySettings {
    public static let defaultsKey = "heron.quickEntry.enabled"

    public static func registerDefaults(_ defaults: UserDefaults = .standard) {
        defaults.register(defaults: [defaultsKey: true])
    }

    /// On unless the owner stored `false`. Reading the stored value (not `bool(forKey:)`) keeps the answer the same
    /// whether or not `registerDefaults` ran: the registration domain is shared across `UserDefaults` instances in a process.
    public static func isEnabled(_ defaults: UserDefaults = .standard) -> Bool {
        (defaults.object(forKey: defaultsKey) as? Bool) ?? true
    }
}

/// What the quick-entry panel holds: the owner's words, the outside text attached as a chip, and the short
/// conversation the panel is continuing.
///
/// Attaching text never sends anything. A message goes only when the owner presses Return in the panel
/// (`send()`), so a Service, a hotkey or a shortcut can never start unattended work.
@MainActor
@Observable
public final class QuickEntryModel {
    /// The panel continues its last conversation if it was used within this many seconds (F1: 30 minutes).
    public static let continuationWindow: TimeInterval = 30 * 60

    /// Where attached text came from. Services is the only source in v1; the "Ask Heron" App Intent is deferred to
    /// v1.1 (macos/HeronMac/APP-INTENTS-CHECK.md) and would add a case that sends `ChatContext.Kind.intent`.
    public enum Source: Equatable, Sendable {
        /// Services: the name of the app that had the selection.
        case selection(app: String)

        var kind: ChatContext.Kind {
            switch self {
            case .selection: .selection
            }
        }

        var appName: String {
            switch self {
            case .selection(let app): app
            }
        }
    }

    /// Text attached to the next message, shown as "From Safari".
    public struct Chip: Equatable, Sendable {
        public var source: Source
        public var text: String
        public var label: String { "From \(source.appName)" }
        /// The context item the core receives. It is outside content whatever the source.
        public var context: ChatContext { ChatContext(kind: source.kind, app: source.appName, text: text) }
    }

    public enum Status: Equatable, Sendable {
        case idle
        case sending
        /// The run started and its reply is arriving.
        case working
        /// The run asked for an approval, which waits in the main window and the menu bar.
        case needsApproval
        case failed(String)
    }

    @ObservationIgnored private let send: @Sendable (String?, String, [ChatContext]) async throws -> ChatStart
    @ObservationIgnored private let now: () -> Date

    public var field = ""
    public private(set) var chip: Chip?
    /// The conversation the panel sends into next, or `nil` for a new one.
    public private(set) var sessionID: String?
    public private(set) var reply = ""
    public private(set) var status: Status = .idle
    /// Called by "Open in Heron" with the panel's conversation, if it has one. The app wires it to the main window.
    @ObservationIgnored public var onOpenInHeron: ((String?) -> Void)?

    @ObservationIgnored private var lastUsed: Date?
    @ObservationIgnored private var heldEvents: [CoreEvent] = []

    public init(client: any CoreClientProtocol, now: @escaping () -> Date = Date.init) {
        self.send = { sessionID, message, context in
            try await client.sendMessage(sessionID: sessionID, message: message, title: nil, context: context)
        }
        self.now = now
    }

    /// For tests and previews: the one call the model makes.
    public init(send: @escaping @Sendable (String?, String, [ChatContext]) async throws -> ChatStart,
                now: @escaping () -> Date = Date.init) {
        self.send = send
        self.now = now
    }

    // MARK: Opening

    /// Called whenever the panel is shown. A conversation unused for 30 minutes or more is let go, and the
    /// reply of the last one with it, so the panel starts clean.
    public func open() {
        if let used = lastUsed, now().timeIntervalSince(used) < Self.continuationWindow { return }
        sessionID = nil
        lastUsed = nil
        reply = ""
        if status != .sending { status = .idle }
    }

    /// Attaches outside text as the chip. The field is left alone: it is for the owner's own words.
    /// Nothing is sent.
    public func attach(text: String, from source: Source) {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        chip = Chip(source: source, text: text)
    }

    public func removeChip() { chip = nil }

    // MARK: Sending (only the owner's Return reaches this)

    /// Return sends when there is something to send: the owner's words, or an attached chip. With neither it refuses.
    public var canSend: Bool {
        status != .sending && (chip != nil || !field.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
    }

    /// Sends the field's text, with the chip as context. The words may be empty when a chip is attached (the core
    /// titles that session "Asked about <kind> from <app>"). A failed send keeps the field and the chip.
    public func submit() async {
        let body = field.trimmingCharacters(in: .whitespacesAndNewlines)
        guard canSend else { return }
        let context = chip.map { [$0.context] } ?? []
        status = .sending
        heldEvents = []
        do {
            let start = try await send(sessionID, body, context)
            sessionID = start.sessionID
            lastUsed = now()
            field = ""
            chip = nil
            reply = ""
            status = .working
            let held = heldEvents
            heldEvents = []
            for event in held { handle(event) }
        } catch {
            heldEvents = []
            status = .failed(describe(error))
        }
    }

    // MARK: The reply

    /// Feeds the core's event stream. Only the panel's own conversation counts.
    public func handle(_ event: CoreEvent) {
        guard let eventSession = event.sessionID else { return }
        if status == .sending { heldEvents.append(event); return }
        guard eventSession == sessionID else { return }
        switch event.kind {
        case .text(let chunk):
            reply += chunk
        case .approval:
            status = .needsApproval
        case .approvalResolved, .approvalParked:
            if status == .needsApproval { status = .working }
        case .done(let text, _):
            if reply.isEmpty { reply = text }
        case .error(let message, let code, let provider, let version):
            status = .failed(ChatErrorCard.from(message: message, code: code, provider: provider, version: version).title)
        case .cancelled:
            status = .idle
        case .runEnd:
            if status == .working || status == .needsApproval { status = .idle }
            lastUsed = now()
        default:
            break
        }
    }

    public func openInHeron() { onOpenInHeron?(sessionID) }
}
