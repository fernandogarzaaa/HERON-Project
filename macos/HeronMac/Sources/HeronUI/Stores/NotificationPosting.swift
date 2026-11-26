import Foundation

/// Shows one desktop notification. The app target supplies the real one (`UserNotifications`, so macOS
/// names Heron as the sender); tests supply a recorder.
public protocol NotificationPosting: AnyObject {
    /// `identifier` is stable for one core event: posting it again replaces the earlier notification.
    func post(title: String, body: String, identifier: String)
}
