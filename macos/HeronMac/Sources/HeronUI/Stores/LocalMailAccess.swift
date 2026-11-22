import Foundation
import AppKit
import HeronCore

// MARK: The Mac's Mail app (Apple Events)

/// Why the Mail app could not be read. Each has its own sentence for the Connections row.
public enum MailAccessError: Error, Equatable, Sendable {
    /// Automation permission for Mail was refused (macOS error -1743), or never granted.
    case notAuthorized
    /// Mail has no accounts, so there is nothing to read.
    case noAccounts
    /// Mail could not be opened or did not answer.
    case notRunning
    case failed(String)
}

/// What went wrong reading the Mail app, when System Settings is where to fix it.
public enum LocalMailProblem: Sendable, Equatable { case automationDenied, noAccounts }

/// Read access to the inbox messages in the Mac's Mail app. The store talks to this protocol so tests never
/// reach Apple Events or the Automation prompt.
public protocol LocalMailAccess: Sendable {
    /// True while Mail is open. A quiet refresh only reads an open Mail: Apple Events would open it.
    func isMailRunning() -> Bool
    /// Recent inbox messages from every account in Mail. The first call shows the macOS Automation prompt. It can take
    /// a while, because Apple Events opens Mail first, so callers run it off the main thread (this does).
    func messages() async throws -> [LocalMailMessage]
}

/// What the app sends and keeps of the mail it reads: a short window, a cap, and one copy of each message.
public enum LocalMailPolicy {
    public static let daysBack = 14
    public static let maxMessages = 300
    public static let maxBodyCharacters = 4000

    /// Drops messages with no id or older than the window, merges the same Message-ID seen in several accounts
    /// (naming each account), keeps the newest `maxMessages`, and truncates long text.
    public static func prepare(_ raw: [LocalMailMessage], now: Date = Date()) -> [LocalMailMessage] {
        let cutoff = Int(now.timeIntervalSince1970) - daysBack * 86_400
        var order: [String] = []
        var byID: [String: LocalMailMessage] = [:]
        for var message in raw {
            message.id = message.id.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !message.id.isEmpty, message.date >= cutoff else { continue }
            if var seen = byID[message.id] {
                if !message.account.isEmpty, !seen.account.components(separatedBy: ", ").contains(message.account) {
                    seen.account = seen.account.isEmpty ? message.account : seen.account + ", " + message.account
                    byID[message.id] = seen
                }
            } else {
                order.append(message.id)
                message.body = String(message.body.prefix(maxBodyCharacters))
                byID[message.id] = message
            }
        }
        return order.compactMap { byID[$0] }.sorted { $0.date > $1.date }.prefix(maxMessages).map { $0 }
    }
}

/// Mail over Apple Events (`NSAppleScript`), read-only. Needs `NSAppleEventsUsageDescription` in Info.plist and, in a
/// hardened build, the Apple Events entitlement (`packaging/macos/Heron.entitlements`). It never reads ~/Library/Mail.
public struct SystemMailAccess: LocalMailAccess {
    public init() {}

    static let mailBundleID = "com.apple.mail"

    public func isMailRunning() -> Bool {
        !NSRunningApplication.runningApplications(withBundleIdentifier: Self.mailBundleID).isEmpty
    }

    public func messages() async throws -> [LocalMailMessage] {
        // A detached task: the script blocks until Mail answers, and Mail may first have to launch.
        try await Task.detached(priority: .utility) {
            let raw = try Self.run(Self.script(daysBack: LocalMailPolicy.daysBack, limit: LocalMailPolicy.maxMessages, bodyCharacters: LocalMailPolicy.maxBodyCharacters))
            return LocalMailPolicy.prepare(try Self.parse(raw))
        }.value
    }

    /// Returns the text "no-accounts", or a list of {id, from, subject, date, account, body}.
    static func script(daysBack: Int, limit: Int, bodyCharacters: Int) -> String {
        """
        with timeout of 180 seconds
          tell application "Mail"
            if (count of accounts) is 0 then return "no-accounts"
            set cutoff to (current date) - \(daysBack) * days
            set out to {}
            repeat with acct in accounts
              if (count of out) ≥ \(limit) then exit repeat
              set acctName to name of acct
              set recent to {}
              try
                set recent to (messages of mailbox "INBOX" of acct whose date received > cutoff)
              on error
                try
                  set recent to (messages of mailbox "Inbox" of acct whose date received > cutoff)
                end try
              end try
              set total to count of recent
              if total > 0 then
                set theIds to message id of recent
                set theSubjects to subject of recent
                set theSenders to sender of recent
                set theDates to date received of recent
                repeat with i from 1 to total
                  if (count of out) ≥ \(limit) then exit repeat
                  set bodyText to ""
                  try
                    set bodyText to content of (item i of recent)
                    if (count of bodyText) > \(bodyCharacters) then set bodyText to text 1 thru \(bodyCharacters) of bodyText
                  end try
                  set end of out to {item i of theIds, item i of theSenders, item i of theSubjects, item i of theDates, acctName, bodyText}
                end repeat
              end if
            end repeat
            return out
          end tell
        end timeout
        """
    }

    static func run(_ source: String) throws -> NSAppleEventDescriptor {
        guard let script = NSAppleScript(source: source) else { throw MailAccessError.failed("Heron couldn't prepare the request to Mail.") }
        var info: NSDictionary?
        let result = script.executeAndReturnError(&info)
        if let info {
            let number = (info[NSAppleScript.errorNumber] as? Int) ?? 0
            let message = (info[NSAppleScript.errorMessage] as? String) ?? ""
            throw map(errorNumber: number, message: message)
        }
        return result
    }

    /// `-1743` is "not authorized to send Apple events"; `-600` and `-609` mean Mail isn't there to answer.
    static func map(errorNumber: Int, message: String) -> MailAccessError {
        switch errorNumber {
        case -1743, -1744: .notAuthorized
        case -600, -609, -10004: .notRunning
        default: .failed(message.isEmpty ? "Mail answered with error \(errorNumber)." : message)
        }
    }

    static func parse(_ descriptor: NSAppleEventDescriptor) throws -> [LocalMailMessage] {
        if descriptor.descriptorType != typeAEList {
            if descriptor.stringValue == "no-accounts" { throw MailAccessError.noAccounts }
            throw MailAccessError.failed("Mail answered in a way Heron didn't expect.")
        }
        guard descriptor.numberOfItems > 0 else { return [] }
        return (1...descriptor.numberOfItems).compactMap { i in
            guard let row = descriptor.atIndex(i), row.numberOfItems >= 6 else { return nil }
            func text(_ n: Int) -> String { row.atIndex(n)?.stringValue ?? "" }
            guard let date = row.atIndex(4)?.dateValue else { return nil }
            return LocalMailMessage(id: text(1), from: text(2), subject: text(3), date: Int(date.timeIntervalSince1970), account: text(5), body: text(6))
        }
    }
}
