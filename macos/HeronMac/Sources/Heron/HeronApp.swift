import SwiftUI
import AppKit
import HeronCore
import HeronUI

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
        let model = AppModel(client: runtime.client, notifier: UserNotificationPoster())
        runtime.start(model: model)
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
            Image(nsImage: MenuBarIcon.image(waiting: model.waitingCount > 0))
        }
        .menuBarExtraStyle(.window)
    }
}
