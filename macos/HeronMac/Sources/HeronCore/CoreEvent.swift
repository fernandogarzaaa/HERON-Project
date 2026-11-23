import Foundation

/// One event from `GET /api/events` (and from the `/api/chat` subset): the envelope
/// `{seq, session_id, run_id, type, ...fields}` of heron-v1.md §3 "One event bus".
public struct CoreEvent: Sendable, Equatable {
    public var seq: Int
    public var sessionID: String?
    public var runID: String?
    public var kind: Kind

    public init(seq: Int = 0, sessionID: String? = nil, runID: String? = nil, kind: Kind) {
        self.seq = seq
        self.sessionID = sessionID
        self.runID = runID
        self.kind = kind
    }

    public enum Kind: Sendable, Equatable {
        case session(id: String, runID: String)
        case text(String)
        case toolStart(id: String, name: String, summary: String, risk: String)
        case toolEnd(id: String, name: String, ok: Bool, preview: String)
        case approval(ApprovalRequest)
        case approvalResolved(id: String, decision: String)
        case approvalParked(id: String, parkedID: Int)
        case usage(prompt: Int, completion: Int)
        case model(name: String, why: String)
        case notice(String)
        case learned([String])
        case done(text: String, title: String?)
        case cancelled
        /// `code` is `cli_version_untested` (with `provider` and `version`), `cli_logged_out` (with
        /// `provider`, the CLI's name; `login`, a command that runs as is, only when the CLI has one;
        /// and `hint`, how to sign in, in words), `risk_notice_not_accepted`, `workspace_refused`, or nil.
        case error(message: String, code: String?, provider: String?, version: String?, login: String? = nil, hint: String? = nil)
        case runEnd
        /// The client fell behind or asked for a `since` the ring no longer holds: reload, then continue.
        case resync(oldest: Int)
    }
}

extension CoreEvent {
    /// One of the events that end a run's reply: `done`, `error`, `cancelled` or `run_end`.
    public var isRunEnding: Bool {
        switch kind {
        case .done, .error, .cancelled, .runEnd: true
        default: false
        }
    }
}

/// Errors the core's routes return that a screen reacts to.
public enum CoreError: Error, Sendable, Equatable {
    /// `409 session_busy` on `POST /api/chat`.
    case sessionBusy
    /// `428 risk_notice_not_accepted`: open the risk notice.
    case riskNoticeNotAccepted
    /// `409` on `POST /api/risk-notice`: the register changed while the screen was open.
    case riskNoticeOutdated
    case notFound
    /// The core stopped twice within a minute and is not running. The details (exit, last log lines) are for
    /// the "Heron's core stopped" card's Copy button, not for a screen's text.
    case coreStopped(details: String)
    case failed(String)
}
