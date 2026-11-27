import Foundation
import os

/// What macOS currently allows Heron to show.
public enum NotificationAuthorization: Equatable, Sendable {
    case allowed
    case notDetermined
    case denied
}

/// The parts of `UNUserNotificationCenter` that posting needs, so the flow can be tested without an
/// app bundle (outside one, the real center crashes).
public protocol NotificationCenterClient: Sendable {
    func authorization() async -> NotificationAuthorization
    /// Shows the permission prompt. Returns whether the owner allowed notifications.
    func requestAuthorization() async throws -> Bool
    func add(identifier: String, title: String, body: String) async throws
}

/// Asks for permission the first time there is something to show, then posts. Neither failure stops
/// anything (the Inbox has the same notice), but both are logged so a missing banner can be traced.
public struct NotificationSender: Sendable {
    public enum Outcome: Equatable, Sendable {
        case posted
        /// Not allowed: denied earlier, declined at the prompt, or the prompt failed.
        case notAllowed
        case failed
    }

    private let center: any NotificationCenterClient
    private let log: @Sendable (String) -> Void

    public init(center: any NotificationCenterClient, log: @escaping @Sendable (String) -> Void = NotificationSender.logError) {
        self.center = center
        self.log = log
    }

    @discardableResult
    public func send(title: String, body: String, identifier: String) async -> Outcome {
        switch await center.authorization() {
        case .allowed: break
        case .denied: return .notAllowed
        case .notDetermined:
            do {
                guard try await center.requestAuthorization() else { return .notAllowed }
            } catch {
                log("Asking for notification permission failed: \(error.localizedDescription)")
                return .notAllowed
            }
        }
        do {
            try await center.add(identifier: identifier, title: title, body: body)
            return .posted
        } catch {
            log("Posting notification \(identifier) failed: \(error.localizedDescription)")
            return .failed
        }
    }

    private static let logger = Logger(subsystem: Bundle.main.bundleIdentifier ?? "app.heron.desktop", category: "notifications")

    /// The default sink. The message holds no notification text, only the identifier and the error.
    public static let logError: @Sendable (String) -> Void = { message in
        logger.error("\(message, privacy: .public)")
    }
}
