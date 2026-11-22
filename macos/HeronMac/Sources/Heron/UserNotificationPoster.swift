import Foundation
import UserNotifications
import HeronUI

/// Posts the core's `notification` events through `UserNotifications`, so macOS shows them as Heron
/// (with Heron's icon) and a click brings Heron forward. `osascript` would show them as Script Editor.
///
/// It asks for permission the first time there is something to show, not at launch: the prompt then
/// arrives with a real notification instead of before the owner has seen the app. If they decline,
/// notifications are dropped (the same result is in the Inbox).
final class UserNotificationPoster: NSObject, NotificationPosting, UNUserNotificationCenterDelegate {
    private let center: UNUserNotificationCenter

    /// Nil outside an app bundle (`swift run Heron`), where `UNUserNotificationCenter` would crash.
    init?(bundle: Bundle = .main) {
        guard bundle.bundleURL.pathExtension == "app", bundle.bundleIdentifier != nil else { return nil }
        center = UNUserNotificationCenter.current()
        super.init()
        center.delegate = self
    }

    func post(title: String, body: String, identifier: String) {
        let center = center
        Task {
            guard await Self.isAllowed(center) else { return }
            let content = UNMutableNotificationContent()
            content.title = title
            content.body = body
            content.sound = .default
            try? await center.add(UNNotificationRequest(identifier: identifier, content: content, trigger: nil))
        }
    }

    private static func isAllowed(_ center: UNUserNotificationCenter) async -> Bool {
        let settings = await center.notificationSettings()
        switch settings.authorizationStatus {
        case .authorized, .provisional: return true
        case .notDetermined: return (try? await center.requestAuthorization(options: [.alert, .sound])) ?? false
        default: return false
        }
    }

    /// Show the banner even while Heron is in front: a finished job is worth seeing wherever the owner is.
    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification,
                                withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
        completionHandler([.banner, .sound])
    }

    /// Clicking the banner activates Heron (the default action); nothing else to do.
    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
                                withCompletionHandler completionHandler: @escaping () -> Void) {
        completionHandler()
    }
}
