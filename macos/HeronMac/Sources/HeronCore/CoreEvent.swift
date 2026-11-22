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
        /// Something the owner should see outside the app: a scheduled job's result, an automation's notice.
        /// Not part of any conversation, so `sessionID` and `runID` are empty.
        case notification(title: String, body: String)
        case learned([String])
        case done(text: String, title: String?)
        case cancelled
        /// `code` is `cli_version_untested` (with `provider` and `version`), `risk_notice_not_accepted`,
        /// `workspace_refused`, or nil.
        case error(message: String, code: String?, provider: String?, version: String?)
        case runEnd
        /// The client fell behind or asked for a `since` the ring no longer holds: reload, then continue.
        case resync(oldest: Int)
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
    case failed(String)
}
