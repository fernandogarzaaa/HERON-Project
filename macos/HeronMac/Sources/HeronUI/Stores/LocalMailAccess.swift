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

/// Mail over Apple Events, read-only, through `/usr/bin/osascript` run as a child process. `NSAppleScript` is meant for the
/// main thread, and running it there would freeze the app while Mail launches or answers; a child process can wait
/// anywhere. The script goes in on stdin and comes back as delimited text (`parse`). Needs `NSAppleEventsUsageDescription`
/// in Info.plist and, in a hardened build, the Apple Events entitlement (`packaging/macos/Heron.entitlements`). It never
/// reads ~/Library/Mail.
public struct SystemMailAccess: LocalMailAccess {
    public init() {}

    static let mailBundleID = "com.apple.mail"
    static let osascriptPath = "/usr/bin/osascript"
    /// Between records and between the fields of a record: ASCII "record separator" and "unit separator". The script
    /// strips both from the text it sends, so a message can't break the format.
    static let recordSeparator: Character = "\u{1E}"
    static let fieldSeparator: Character = "\u{1F}"
    /// The script's own timeout is 180 s; this is the backstop for an osascript that never exits.
    static let processTimeout: TimeInterval = 200

    public func isMailRunning() -> Bool {
        !NSRunningApplication.runningApplications(withBundleIdentifier: Self.mailBundleID).isEmpty
    }

    public func messages() async throws -> [LocalMailMessage] {
        // A unit test must never reach the real Mail: Apple Events from a test host with no usage description
        // is killed by macOS. Tests inject a fake behind `LocalMailAccess` instead.
        if NSClassFromString("XCTestCase") != nil { throw MailAccessError.failed("Mail is not available in tests.") }
        // A detached task, so the blocking read of the child's output is never on the main thread or the shared pool's
        // short-lived workers; Mail may first have to launch.
        return try await Task.detached(priority: .utility) {
            let script = Self.script(daysBack: LocalMailPolicy.daysBack, limit: LocalMailPolicy.maxMessages, bodyCharacters: LocalMailPolicy.maxBodyCharacters)
            let output = try Self.run(script)
            return LocalMailPolicy.prepare(try Self.parse(output))
        }.value
    }

    /// The AppleScript. Prints `no-accounts`, or records of `id US sender US subject US secondsAgo US account US body`
    /// joined with RS. Dates are sent as "seconds ago", which doesn't depend on the time zone or the date format.
    static func script(daysBack: Int, limit: Int, bodyCharacters: Int) -> String {
        """
        on clean(t)
          set saved to AppleScript's text item delimiters
          set AppleScript's text item delimiters to {character id 30, character id 31}
          set parts to text items of (t as text)
          set AppleScript's text item delimiters to " "
          set cleaned to parts as text
          set AppleScript's text item delimiters to saved
          return cleaned
        end clean

        with timeout of 180 seconds
          tell application "Mail"
            if (count of accounts) is 0 then return "no-accounts"
            set nowDate to current date
            set cutoff to nowDate - \(daysBack) * days
            set out to ""
            set n to 0
            repeat with acct in accounts
              if n ≥ \(limit) then exit repeat
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
                  if n ≥ \(limit) then exit repeat
                  set bodyText to ""
                  try
                    set bodyText to content of (item i of recent)
                    if (count of bodyText) > \(bodyCharacters) then set bodyText to text 1 thru \(bodyCharacters) of bodyText
                  end try
                  set age to (nowDate - (item i of theDates)) as integer
                  set rec to (my clean(item i of theIds)) & (character id 31) & (my clean(item i of theSenders)) & (character id 31) & (my clean(item i of theSubjects)) & (character id 31) & (age as text) & (character id 31) & (my clean(acctName)) & (character id 31) & (my clean(bodyText))
                  set out to out & rec & (character id 30)
                  set n to n + 1
                end repeat
              end if
            end repeat
            return out
          end tell
        end timeout
        """
    }

    /// Runs the script in `osascript` (script on stdin) and returns what it printed. Child of Heron, so macOS asks the
    /// Automation question on Heron's behalf.
    static func run(_ source: String) throws -> String {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: osascriptPath)
        process.arguments = ["-"]
        let stdin = Pipe(), stdout = Pipe(), stderr = Pipe()
        process.standardInput = stdin
        process.standardOutput = stdout
        process.standardError = stderr
        do { try process.run() } catch { throw MailAccessError.failed("Heron couldn't start osascript to ask Mail.") }
        let watchdog = DispatchWorkItem { if process.isRunning { process.terminate() } }
        DispatchQueue.global().asyncAfter(deadline: .now() + processTimeout, execute: watchdog)
        defer { watchdog.cancel() }
        stdin.fileHandleForWriting.write(Data(source.utf8))
        try? stdin.fileHandleForWriting.close()
        // stdout can be a megabyte: read it to the end (so the child never blocks on a full pipe) before stderr.
        let out = stdout.fileHandleForReading.readDataToEndOfFile()
        let err = stderr.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        guard process.terminationStatus == 0 else {
            throw failure(stderr: String(decoding: err, as: UTF8.self), status: process.terminationStatus)
        }
        return String(decoding: out, as: UTF8.self)
    }

    /// osascript reports `execution error: <text> (<number>)` on stderr and exits 1.
    static func failure(stderr: String, status: Int32) -> MailAccessError {
        let text = stderr.trimmingCharacters(in: .whitespacesAndNewlines)
        var number = 0
        if let open = text.lastIndex(of: "("), let close = text.lastIndex(of: ")"), open < close,
           let n = Int(text[text.index(after: open)..<close]) { number = n }
        if number == 0, status == 15 || status == 9 { return .failed("Mail didn't answer in time. Try again.") }
        let message = text.replacingOccurrences(of: "execution error: ", with: "")
        return map(errorNumber: number, message: message.isEmpty ? "" : message)
    }

    /// `-1743` is "not authorized to send Apple events"; `-600` and `-609` mean Mail isn't there to answer.
    static func map(errorNumber: Int, message: String) -> MailAccessError {
        switch errorNumber {
        case -1743, -1744: .notAuthorized
        case -600, -609, -10004: .notRunning
        default: .failed(message.isEmpty ? "Mail answered with error \(errorNumber)." : message)
        }
    }

    /// The script's printed text as messages. `now` turns "seconds ago" into a date.
    static func parse(_ output: String, now: Date = Date()) throws -> [LocalMailMessage] {
        let text = output.trimmingCharacters(in: .whitespacesAndNewlines)
        if text == "no-accounts" { throw MailAccessError.noAccounts }
        let nowSeconds = Int(now.timeIntervalSince1970)
        return text.split(separator: recordSeparator, omittingEmptySubsequences: true).compactMap { record in
            let fields = record.split(separator: fieldSeparator, maxSplits: 5, omittingEmptySubsequences: false).map(String.init)
            guard fields.count == 6, let ago = Int(fields[3].trimmingCharacters(in: .whitespaces)) else { return nil }
            return LocalMailMessage(id: fields[0].trimmingCharacters(in: .whitespacesAndNewlines), from: fields[1], subject: fields[2],
                                    date: nowSeconds - ago, account: fields[4], body: fields[5])
        }
    }
}
