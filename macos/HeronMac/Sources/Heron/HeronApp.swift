import SwiftUI
import AppKit
import UserNotifications
import HeronCore
import HeronUI

/// Shows the app's notices while it is active, and opens what a clicked notice is about (F3): the run, or the
/// approval sheet. A notice offers no buttons, so a click is the only way to respond to one.
@MainActor
final class NoticeCenterDelegate: NSObject, UNUserNotificationCenterDelegate {
    private let model: AppModel
    /// The center keeps its delegate weakly, so the one installed is held here for the life of the app.
    private static var installed: NoticeCenterDelegate?

    init(model: AppModel) { self.model = model }

    /// Makes a click on a notice open its run or approval. Only an app bundle has a notification center to ask.
    static func install(model: AppModel) {
        guard Bundle.main.bundleURL.pathExtension == "app" else { return }
        let delegate = NoticeCenterDelegate(model: model)
        installed = delegate
        UNUserNotificationCenter.current().delegate = delegate
    }

    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification) async -> UNNotificationPresentationOptions {
        [.banner, .list]
    }

    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse) async {
        let identifier = response.notification.request.identifier
        await MainActor.run {
            Self.bringMainWindowForward()
            Task { await self.model.openFromNotification(identifier: identifier) }
        }
    }

    /// The window outlives its close button, so ordering it front brings Heron back from the menu bar.
    private static func bringMainWindowForward() {
        NSApp.activate(ignoringOtherApps: true)
        let candidates = NSApp.windows.filter { $0.canBecomeMain }
        let window = candidates.first { $0.identifier?.rawValue.hasPrefix("main") == true }
            ?? candidates.first { !($0.identifier?.rawValue.localizedCaseInsensitiveContains("settings") ?? false) }
        window?.makeKeyAndOrderFront(nil)
    }
}

/// The app's runtime: the bundled core, and the client that follows it across restarts.
///
/// `HERON_PREVIEW=1` runs on `PreviewCoreClient` instead, for working on the screens without a core binary
/// (`swift run Heron`). Anything else is the real thing: `CoreClient` over `CoreProcess`.
@MainActor
final class Runtime {
    let client: any CoreClientProtocol
    let process: CoreProcess?

    init(environment: [String: String] = ProcessInfo.processInfo.environment) {
        if environment["HERON_PREVIEW"] == "1" {
            client = PreviewCoreClient(pace: .milliseconds(450))
            process = nil
        } else {
            let process = CoreProcess(launch: .bundled(environment: environment))
            self.process = process
            client = ManagedCoreClient(process: process)
        }
    }

    /// Starts the core and feeds its status to the model (the banners and the "core stopped" card).
    func start(model: AppModel) {
        guard let process else { return }
        model.restartCore = { [process] in process.restart() }
        let statuses = process.statuses()
        Task { @MainActor in
            for await status in statuses {
                switch status {
                case .starting: model.coreStatus = .starting
                case .running: model.coreStatus = .running
                case .restarted: model.coreStatus = .restarted; model.dismissedRestartBanner = false
                case .stopped(let details): model.coreStatus = .stopped(details: details)
                }
            }
        }
        process.start()
        // Quitting closes the core's stdin, which is how it learns the app is gone.
        NotificationCenter.default.addObserver(forName: NSApplication.willTerminateNotification, object: nil, queue: .main) { _ in
            process.stop()
        }
    }
}

@main
struct HeronApp: App {
    @State private var model: AppModel

    init() {
        let runtime = Runtime()
        let model = AppModel(client: runtime.client)
        runtime.start(model: model)
        NoticeCenterDelegate.install(model: model)
        _model = State(initialValue: model)
    }

    var body: some Scene {
        Window("Heron", id: "main") {
            RootView(model: model)
        }
        .defaultSize(width: 1180, height: 760)
        .commands { HeronCommands(model: model) }

        Settings {
            SettingsView(model: model)
        }

        MenuBarExtra(isInserted: Bindable(model.preferences).showMenuBarExtra) {
            MenuBarContent(model: model)
        } label: {
            Image(nsImage: MenuBarIcon.image(waiting: model.waitingCount > 0, badge: model.runs.badgeCount))
        }
        .menuBarExtraStyle(.window)
    }
}
