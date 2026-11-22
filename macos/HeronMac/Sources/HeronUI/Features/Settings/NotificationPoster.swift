import Foundation
import UserNotifications

/// What the app needs from the notification center. A protocol so tests use a recorder: whether the real
/// `UNUserNotificationCenter` works from `swift test` is unverified, and this avoids the question.
public protocol NotificationPoster: Sendable {
    /// Asks macOS for permission to post. Returns whether it was granted.
    func requestAuthorization() async -> Bool
    /// Whether macOS currently lets the app post.
    func isAuthorized() async -> Bool
    func post(title: String, body: String, identifier: String) async throws
}

/// The real center. `UNUserNotificationCenter.current()` is read only when a method runs, so building this in a
/// process with no app bundle (a test host) is harmless until something calls it.
public struct SystemNotificationPoster: NotificationPoster {
    public init() {}

    public func requestAuthorization() async -> Bool {
        (try? await UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound])) ?? false
    }

    public func isAuthorized() async -> Bool {
        let settings = await UNUserNotificationCenter.current().notificationSettings()
        switch settings.authorizationStatus {
        case .authorized, .provisional: return true
        default: return false
        }
    }

    public func post(title: String, body: String, identifier: String) async throws {
        let content = UNMutableNotificationContent()
        content.title = title
        content.body = body
        try await UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: identifier, content: content, trigger: nil))
    }
}
