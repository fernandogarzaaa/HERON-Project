import SwiftUI
import HeronCore
import HeronUI

/// The one place the app's runtime client is built. U4a runs on the preview stub; U4b replaces this with
/// `CoreClient` over `CoreProcess`, and no view changes.
enum RuntimeClient {
    static func make() -> any CoreClientProtocol {
        PreviewCoreClient(pace: .milliseconds(450))
    }
}

@main
struct HeronApp: App {
    @State private var model = AppModel(client: RuntimeClient.make())

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
