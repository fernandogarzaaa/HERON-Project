import SwiftUI
import AppKit
import HeronCore

/// The words on the Connections rows and sheets, in one place so a test can read them.
enum SourceCopy {
    static let scanSentence = "Reads your operating system, time zone, which developer tools are installed, the names of the folders in your home folder, and your Git name. It opens no files."
    static let documentsSentence = "Pick a folder of notes or documents. Heron reads the files in it and remembers what it learns. Folders that hold keys or passwords are refused."
    static let calendarHint = "Google Calendar: Settings › your calendar › “Secret address in iCal format”. iCloud: the calendar's public (subscription) link. Anyone with the link can read the calendar, so keep it private."
    static let iCloudWhy = "iCloud doesn't let apps use your Apple ID password. Create an app-specific password for Heron under Sign-In and Security › App-Specific Passwords. Heron stores it only in its own protected secret store."
    static let iCloudURL = "https://account.apple.com"
    static let otherMailboxButton = "Connect another mailbox by app password…"
    static let emailHint = "Gmail and iCloud need an app-specific password, not your usual one. Create one in your account's security settings. Heron checks the sign-in before saving anything."

    static let accountsIntro = "One sign-in connects both your email and your calendar for that account. Heron can only read them, and it asks for nothing else."
    static let accountScope = "Email and Calendar, read-only"
    static let waitingForSignIn = "Waiting for you to finish signing in…"
    static let readingAccount = "Reading your email and calendar…"
    static let browserWouldNotOpen = "Heron couldn't open your browser. Open your default browser once, then try again."
    static let signInTimedOut = "The sign-in wasn't finished in time. Try again."
    static let localCalendarSentence = "Reads every calendar in the Calendar app on this Mac, including Google and iCloud accounts you've added there. Read-only, and macOS asks you first."
    static let calendarAccessDenied = "Heron can't read your calendars yet. Open System Settings › Privacy & Security › Calendars, turn Heron on, then try again."
    static let privacyCalendarsURL = "x-apple.systempreferences:com.apple.preference.security?Privacy_Calendars"

    static let localMailSentence = "Reads the last 14 days of inbox mail from every account in the Mail app on this Mac, read-only. To add Outlook or another Gmail, add it in System Settings › Internet Accounts, then connect here. macOS asks first, and Mail opens if it isn't."
    static let mailAccessDenied = "Heron can't read Mail yet. Open System Settings › Privacy & Security › Automation, turn on Mail under Heron, then try again."
    static let mailHasNoAccounts = "The Mail app has no accounts yet. Add one in System Settings › Internet Accounts, then try again."
    static let mailWouldNotOpen = "Heron couldn't open Mail. Open the Mail app once, then try again."
    static let connectedButCouldNotRefresh = "Connected, but Heron couldn't refresh this list. Close Settings and open it again."
    static let cancelledBeforeFinishing = "That was cancelled before it finished. Try again."
    static let mailWasEmpty = "Mail returned no inbox messages from the last 14 days. Is your account set up in Mail, and has Mail finished downloading? Press Refresh to try again."
    /// No error line on a row is longer than this, whatever a tool printed.
    static let maxErrorCharacters = 200
    static let readingMail = "Reading Mail… (it can take a moment)"
    static let privacyAutomationURL = "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation"
    static let internetAccountsURL = "x-apple.systempreferences:com.apple.Internet-Accounts-Settings.extension"

    static func signInButton(_ provider: OAuthProviderID) -> String { "Sign in with \(provider.displayName)" }
    static func signInNotSetUp(_ provider: OAuthProviderID) -> String { "\(provider.displayName) sign-in isn't set up in this build yet." }
    static func firstSyncFailed(_ reason: String) -> String { "Signed in, but the first read failed: \(reason) Heron will try again later." }

    /// What the confirm sheet says Heron will forget.
    static func forgetSentence(_ source: ConnectionInfo) -> String {
        switch source.id {
        case "documents": "Heron will stop reading your folders and forget the notes and facts it learned from them."
        case "calendar": "Heron will stop reading your calendar and forget the events and facts it learned from it."
        case "mail_local": "Heron will stop reading the Mail app on this Mac and forget the messages and facts it learned from it. Mail itself is not touched."
        case "calendar_local": "Heron will stop reading the Calendar app on this Mac and forget the events and facts it learned from it."
        case "email": "Heron will delete its saved app password and forget the messages and facts it learned from it."
        case "computer": "Heron will forget what it learned by scanning this Mac."
        case "google", "microsoft": "Heron will sign out of this account and forget the email, events and facts it learned from it. To take back its access at \(source.name) as well, remove Heron in your \(source.name) account's security settings."
        default: "Heron will forget what it learned from this source."
        }
    }

    static func connectTitle(_ id: String) -> String { id == "computer" ? "Scan" : "Connect…" }
}

/// Which sheet is open.
enum SourceSheet: String, Identifiable {
    case calendar, email, icloud
    var id: String { rawValue }
}

/// Everything Settings › Connections shows, and the first-run sources step shows the same: the account sign-ins
/// first, then the Mac's Calendar, folders and this Mac, then the link and password flows under "Other ways to connect".
struct ConnectionsPanel: View {
    let store: SettingsStore
    @Environment(\.heronTheme) private var theme
    @State private var sheet: SourceSheet?
    @State private var showsOtherWays = false

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s4) {
            VStack(alignment: .leading, spacing: Spacing.s2) {
                Text(SourceCopy.accountsIntro).font(HeronFont.callout).foregroundStyle(theme.ink2).fixedSize(horizontal: false, vertical: true)
                RowGroup {
                    ForEach(Array(OAuthProviderID.allCases.enumerated()), id: \.element.id) { i, provider in
                        if i > 0 { RowDivider() }
                        AccountRow(provider: provider, store: store)
                    }
                    RowDivider()
                    LocalCalendarRow(store: store)
                    RowDivider()
                    LocalMailRow(store: store)
                }
            }
            SourceRows(store: store, ids: ["email", "documents", "computer"], onSheet: { sheet = $0 })
            DisclosureGroup(isExpanded: $showsOtherWays) {
                VStack(alignment: .leading, spacing: Spacing.s2) {
                    Text("A calendar's private link, or the app password of any other mailbox. Use these if you can't sign in above.")
                        .font(HeronFont.callout).foregroundStyle(theme.ink2).fixedSize(horizontal: false, vertical: true)
                    SourceRows(store: store, ids: ["calendar"], onSheet: { sheet = $0 })
                    // Heron keeps one password-based mailbox: iCloud above, or another server here.
                    if store.settings?.connections.first(where: { $0.id == "email" })?.connected != true {
                        Button(SourceCopy.otherMailboxButton) { store.clearSourceError("email"); sheet = .email }
                            .buttonStyle(HButtonStyle(.link))
                    }
                }.padding(.top, Spacing.s2)
            } label: {
                Text("Other ways to connect").font(HeronFont.subLabel).foregroundStyle(theme.ink)
            }
            .accessibilityLabel("Other ways to connect")
        }
        .sheet(item: $sheet) { which in
            switch which {
            case .calendar: CalendarConnectSheet(store: store)
            case .email: EmailConnectSheet(store: store)
            case .icloud: ICloudConnectSheet(store: store)
            }
        }
        .sheet(item: Binding(get: { store.pendingDisconnect }, set: { if $0 == nil { store.cancelDisconnect() } })) { row in
            SourceDisconnectConfirmView(source: row, onCancel: { store.cancelDisconnect() },
                                        onConfirm: { Task { await store.confirmDisconnect() } })
        }
    }
}

/// One provider: the main button, the waiting state with Cancel, or the connected account with Disconnect.
struct AccountRow: View {
    let provider: OAuthProviderID
    let store: SettingsStore
    @Environment(\.heronTheme) private var theme

    private var account: OAuthAccount { store.account(provider) }
    private var waiting: Bool { store.signingIn == provider }
    private var reading: Bool { store.syncingAccounts.contains(provider) }
    private var busy: Bool { store.isBusy(source: provider.rawValue) }

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s1) {
            HStack(spacing: Spacing.s3) {
                VStack(alignment: .leading, spacing: 1) {
                    HStack(spacing: Spacing.s2) {
                        Text(provider.displayName).foregroundStyle(theme.ink)
                        if account.connected { HTag("Connected", tone: .ok) }
                    }
                    Text(account.connected ? (account.account ?? SourceCopy.accountScope) : SourceCopy.accountScope)
                        .font(HeronFont.callout).foregroundStyle(theme.ink2).fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: Spacing.s3)
                trailing
            }
            if !account.clientConfigured && !account.connected {
                Text(SourceCopy.signInNotSetUp(provider)).font(HeronFont.callout).foregroundStyle(theme.ink3)
            }
            if waiting {
                Text(SourceCopy.waitingForSignIn).font(HeronFont.callout).foregroundStyle(theme.ink2)
            }
            if let message = store.sourceErrors[provider.rawValue] {
                Text(message).font(HeronFont.callout).foregroundStyle(theme.danger).fixedSize(horizontal: false, vertical: true)
                    .accessibilityLabel("Couldn't connect \(provider.displayName). \(message)")
            }
        }
        .padding(.horizontal, Spacing.s4).padding(.vertical, 10)
        .accessibilityElement(children: .contain)
    }

    @ViewBuilder private var trailing: some View {
        if waiting {
            ProgressView().controlSize(.small)
            Button("Cancel") { store.cancelSignIn() }.buttonStyle(HButtonStyle(.secondary))
        } else if reading || busy {
            ProgressView().controlSize(.small)
            Text(reading ? SourceCopy.readingAccount : "Working…").font(HeronFont.callout).foregroundStyle(theme.ink2)
        } else if account.connected {
            Button("Disconnect") { store.askToDisconnect(account: provider) }
                .buttonStyle(HButtonStyle(.secondary)).accessibilityLabel("Disconnect \(provider.displayName)")
        } else {
            Button(SourceCopy.signInButton(provider)) {
                store.beginSignIn(provider) { NSWorkspace.shared.open($0) }
            }
            .buttonStyle(HButtonStyle(.primary)).disabled(!account.clientConfigured || store.signingIn != nil)
        }
    }
}

/// "Use your Mac's Calendar": one click, then the macOS permission prompt.
struct LocalCalendarRow: View {
    let store: SettingsStore
    @Environment(\.heronTheme) private var theme

    private var row: ConnectionInfo? { store.settings?.connections.first { $0.id == "calendar_local" } }
    private var connected: Bool { row?.connected == true }
    private var busy: Bool { store.isBusy(source: "calendar_local") }

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s1) {
            HStack(spacing: Spacing.s3) {
                VStack(alignment: .leading, spacing: 1) {
                    HStack(spacing: Spacing.s2) {
                        Text("Mac Calendar").foregroundStyle(theme.ink)
                        HTag(connected ? "Connected" : "Not connected", tone: connected ? .ok : .neutral)
                    }
                    if connected, let detail = row?.detail {
                        Text(detail).font(HeronFont.callout).foregroundStyle(theme.ink2)
                    }
                }
                Spacer(minLength: Spacing.s3)
                if busy {
                    ProgressView().controlSize(.small)
                    Text("Working…").font(HeronFont.callout).foregroundStyle(theme.ink2)
                } else if connected {
                    Button("Refresh") { Task { await store.connectLocalCalendar() } }.buttonStyle(HButtonStyle(.secondary))
                    Button("Disconnect") { store.askToDisconnect("calendar_local") }
                        .buttonStyle(HButtonStyle(.secondary)).accessibilityLabel("Disconnect Mac Calendar")
                } else {
                    Button("Use your Mac's Calendar") { Task { await store.connectLocalCalendar() } }
                        .buttonStyle(HButtonStyle(.secondary))
                }
            }
            Text(SourceCopy.localCalendarSentence).font(HeronFont.callout).foregroundStyle(theme.ink3).fixedSize(horizontal: false, vertical: true)
            if let message = store.sourceErrors["calendar_local"] {
                Text(message).font(HeronFont.callout).foregroundStyle(theme.danger).fixedSize(horizontal: false, vertical: true)
                    .accessibilityLabel("Couldn't connect the Mac Calendar. \(message)")
                if store.localCalendarDenied {
                    Button("Open System Settings") {
                        if let url = URL(string: SourceCopy.privacyCalendarsURL) { NSWorkspace.shared.open(url) }
                    }.buttonStyle(HButtonStyle(.link))
                }
            }
        }
        .padding(.horizontal, Spacing.s4).padding(.vertical, 10)
        .accessibilityElement(children: .contain)
    }
}

/// "Use your Mac's Mail": one click, then the macOS Automation prompt. Mirrors the Mac Calendar row.
struct LocalMailRow: View {
    let store: SettingsStore
    @Environment(\.heronTheme) private var theme

    private var row: ConnectionInfo? { store.settings?.connections.first { $0.id == "mail_local" } }
    private var connected: Bool { row?.connected == true }
    private var busy: Bool { store.isBusy(source: "mail_local") }

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s1) {
            HStack(spacing: Spacing.s3) {
                VStack(alignment: .leading, spacing: 1) {
                    HStack(spacing: Spacing.s2) {
                        Text("Mac Mail").foregroundStyle(theme.ink)
                        HTag(connected ? "Connected" : "Not connected", tone: connected ? .ok : .neutral)
                    }
                    if connected, let detail = row?.detail {
                        Text(detail).font(HeronFont.callout).foregroundStyle(theme.ink2)
                    }
                }
                Spacer(minLength: Spacing.s3)
                if busy {
                    ProgressView().controlSize(.small)
                    Text(SourceCopy.readingMail).font(HeronFont.callout).foregroundStyle(theme.ink2)
                } else if connected {
                    Button("Refresh") { Task { await store.connectLocalMail() } }.buttonStyle(HButtonStyle(.secondary))
                    Button("Disconnect") { store.askToDisconnect("mail_local") }
                        .buttonStyle(HButtonStyle(.secondary)).accessibilityLabel("Disconnect Mac Mail")
                } else {
                    Button("Use your Mac's Mail") { Task { await store.connectLocalMail() } }
                        .buttonStyle(HButtonStyle(.secondary))
                }
            }
            Text(SourceCopy.localMailSentence).font(HeronFont.callout).foregroundStyle(theme.ink3).fixedSize(horizontal: false, vertical: true)
            if let message = store.sourceErrors["mail_local"] {
                Text(message).font(HeronFont.callout).foregroundStyle(theme.danger).fixedSize(horizontal: false, vertical: true)
                    .accessibilityLabel("Couldn't connect the Mac Mail. \(message)")
                if let problem = store.localMailProblem {
                    Button(problem == .automationDenied ? "Open Automation settings" : "Open Internet Accounts") {
                        let target = problem == .automationDenied ? SourceCopy.privacyAutomationURL : SourceCopy.internetAccountsURL
                        if let url = URL(string: target) { NSWorkspace.shared.open(url) }
                    }.buttonStyle(HButtonStyle(.link))
                }
            }
        }
        .padding(.horizontal, Spacing.s4).padding(.vertical, 10)
        .accessibilityElement(children: .contain)
    }
}

/// Some of the sources as rows with Connect, Scan and Disconnect.
struct SourceRows: View {
    let store: SettingsStore
    let ids: [String]
    let onSheet: (SourceSheet) -> Void
    @Environment(\.heronTheme) private var theme

    var body: some View {
        let rows = (store.settings?.connections ?? []).filter { ids.contains($0.id) }
        Group {
            if rows.isEmpty {
                Text("Heron couldn't read your sources. Reopen Settings to try again.").foregroundStyle(theme.ink2)
            } else {
                RowGroup {
                    ForEach(Array(rows.enumerated()), id: \.element.id) { i, row in
                        if i > 0 { RowDivider() }
                        SourceRow(row: row, store: store, onConnect: { connect(row.id) })
                    }
                }
            }
        }
    }

    private func connect(_ id: String) {
        store.clearSourceError(id)
        switch id {
        case "documents": chooseFolder()
        case "calendar": onSheet(.calendar)
        case "email": onSheet(.icloud)
        case "computer": Task { await store.connect(.computer) }
        default: break
        }
    }

    private func chooseFolder() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.message = "Choose a folder of notes or documents for Heron to read."
        panel.prompt = "Connect"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        Task { await store.connect(.documents(path: url.path)) }
    }
}

private struct SourceRow: View {
    let row: ConnectionInfo
    let store: SettingsStore
    let onConnect: () -> Void
    @Environment(\.heronTheme) private var theme

    private var busy: Bool { store.isBusy(source: row.id) }

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s1) {
            HStack(spacing: Spacing.s3) {
                VStack(alignment: .leading, spacing: 1) {
                    HStack(spacing: Spacing.s2) {
                        Text(row.name).foregroundStyle(theme.ink)
                        HTag(row.connected ? "Connected" : "Not connected", tone: row.connected ? .ok : .neutral)
                    }
                    // "Not connected" is already the tag; the line is for what is connected (an address, a count).
                    if row.connected {
                        Text(row.detail).font(HeronFont.callout).foregroundStyle(theme.ink2).fixedSize(horizontal: false, vertical: true)
                    }
                }
                Spacer(minLength: Spacing.s3)
                if busy {
                    ProgressView().controlSize(.small)
                    Text("Working…").font(HeronFont.callout).foregroundStyle(theme.ink2)
                } else {
                    if row.connected {
                        if row.id == "computer" {
                            Button("Scan again", action: onConnect).buttonStyle(HButtonStyle(.secondary))
                        }
                        Button("Disconnect") { store.askToDisconnect(row.id) }
                            .buttonStyle(HButtonStyle(.secondary))
                            .accessibilityLabel("Disconnect \(row.name)")
                    } else {
                        Button(SourceCopy.connectTitle(row.id), action: onConnect)
                            .buttonStyle(HButtonStyle(.secondary))
                            .accessibilityLabel(row.id == "computer" ? "Scan this Mac" : "Connect \(row.name)")
                    }
                }
            }
            if row.id == "computer" {
                Text(SourceCopy.scanSentence).font(HeronFont.callout).foregroundStyle(theme.ink3).fixedSize(horizontal: false, vertical: true)
            } else if row.id == "documents" && !row.connected {
                Text(SourceCopy.documentsSentence).font(HeronFont.callout).foregroundStyle(theme.ink3).fixedSize(horizontal: false, vertical: true)
            }
            // Email and Calendar show their failures in the sheet that made the request, so the row only repeats the ones it caused itself.
            if let message = store.sourceErrors[row.id], !(row.id == "email" || row.id == "calendar") || row.connected {
                Text(message).font(HeronFont.callout).foregroundStyle(theme.danger).fixedSize(horizontal: false, vertical: true)
                    .accessibilityLabel("Couldn't change \(row.name). \(message)")
            }
        }
        .padding(.horizontal, Spacing.s4).padding(.vertical, 10)
        .accessibilityElement(children: .contain)
    }
}

// MARK: Sheets

private struct SheetFrame<Content: View>: View {
    let title: String
    @ViewBuilder let content: Content
    @Environment(\.heronTheme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s4) {
            Text(title).font(HeronFont.title2).foregroundStyle(theme.ink).accessibilityAddTraits(.isHeader)
            content
        }
        .padding(Spacing.s6)
        .frame(width: 480, alignment: .leading)
        .background(theme.surface)
        .overlay(RoundedRectangle(cornerRadius: Radius.sheet, style: .continuous).stroke(theme.cardBorder, lineWidth: theme.cardBorderWidth))
        .accessibilityElement(children: .contain)
        .accessibilityLabel(title)
    }
}

private struct SheetError: View {
    let message: String?
    @Environment(\.heronTheme) private var theme

    var body: some View {
        if let message {
            Text(message).foregroundStyle(theme.danger).fixedSize(horizontal: false, vertical: true)
                .padding(Spacing.s3).frame(maxWidth: .infinity, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: Radius.control, style: .continuous).fill(theme.dangerSoft))
                .accessibilityLabel("Couldn't connect. \(message)")
        }
    }
}

struct CalendarConnectSheet: View {
    let store: SettingsStore
    @Environment(\.dismiss) private var dismiss
    @Environment(\.heronTheme) private var theme
    @State private var link = ""

    private var busy: Bool { store.isBusy(source: "calendar") }
    private var canConnect: Bool { !link.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !busy }

    var body: some View {
        SheetFrame(title: "Connect a calendar by link") {
            Text("Paste your calendar's private link (an ICS link). Heron reads your events and never changes the calendar.")
                .foregroundStyle(theme.ink2).fixedSize(horizontal: false, vertical: true)
            VStack(alignment: .leading, spacing: Spacing.s2) {
                SectionLabel("Private ICS link")
                TextField("https://calendar.google.com/calendar/ical/…/basic.ics", text: $link)
                    .textFieldStyle(.roundedBorder).disabled(busy).accessibilityLabel("Private ICS link")
                    .onSubmit { if canConnect { submit() } }
            }
            Text(SourceCopy.calendarHint).font(HeronFont.callout).foregroundStyle(theme.ink2).fixedSize(horizontal: false, vertical: true)
            SheetError(message: store.sourceErrors["calendar"])
            HStack {
                if busy { ProgressView().controlSize(.small); Text("Checking the link…").font(HeronFont.callout).foregroundStyle(theme.ink2) }
                Spacer()
                Button("Cancel") { dismiss() }.buttonStyle(HButtonStyle(.secondary)).keyboardShortcut(.cancelAction)
                Button("Connect") { submit() }.buttonStyle(HButtonStyle(.primary)).disabled(!canConnect).keyboardShortcut(.defaultAction)
            }
        }
        .onAppear { store.clearSourceError("calendar") }
    }

    private func submit() {
        Task { if await store.connect(.calendar(url: link.trimmingCharacters(in: .whitespacesAndNewlines))) { dismiss() } }
    }
}

/// iCloud Mail: just the address and an app-specific password. The server, port and TLS are filled in.
struct ICloudConnectSheet: View {
    let store: SettingsStore
    @Environment(\.dismiss) private var dismiss
    @Environment(\.heronTheme) private var theme
    @State private var address = ""
    /// Lives only in this sheet: sent once with the request, then cleared.
    @State private var password = ""

    private var busy: Bool { store.isBusy(source: "email") }
    private var canConnect: Bool {
        ![address, password].contains { $0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty } && !busy
    }

    var body: some View {
        SheetFrame(title: "Connect iCloud Mail") {
            Text("Heron reads recent messages in your iCloud inbox so it can answer questions about them. It only reads.")
                .foregroundStyle(theme.ink2).fixedSize(horizontal: false, vertical: true)
            VStack(alignment: .leading, spacing: Spacing.s2) {
                SectionLabel("iCloud email address")
                TextField("you@icloud.com", text: $address).textFieldStyle(.roundedBorder).disabled(busy).accessibilityLabel("iCloud email address")
            }
            VStack(alignment: .leading, spacing: Spacing.s2) {
                SectionLabel("App-specific password")
                SecureField("xxxx-xxxx-xxxx-xxxx", text: $password).textFieldStyle(.roundedBorder).disabled(busy)
                    .accessibilityLabel("App-specific password").onSubmit { if canConnect { submit() } }
            }
            Text(SourceCopy.iCloudWhy).font(HeronFont.callout).foregroundStyle(theme.ink2).fixedSize(horizontal: false, vertical: true)
            Button("Open account.apple.com") {
                if let url = URL(string: SourceCopy.iCloudURL) { NSWorkspace.shared.open(url) }
            }.buttonStyle(HButtonStyle(.link))
            SheetError(message: store.sourceErrors["email"])
            HStack {
                if busy { ProgressView().controlSize(.small); Text("Signing in…").font(HeronFont.callout).foregroundStyle(theme.ink2) }
                Spacer()
                Button("Cancel") { dismiss() }.buttonStyle(HButtonStyle(.secondary)).keyboardShortcut(.cancelAction).disabled(busy)
                Button("Connect") { submit() }.buttonStyle(HButtonStyle(.primary)).disabled(!canConnect).keyboardShortcut(.defaultAction)
            }
        }
        .onAppear { store.clearSourceError("email") }
        .onDisappear { password = "" }
    }

    private func submit() {
        let request = SourceConnection.iCloudMail(address: address.trimmingCharacters(in: .whitespacesAndNewlines), password: password)
        Task {
            if await store.connect(request) { password = ""; dismiss() }
        }
    }
}

struct EmailConnectSheet: View {
    let store: SettingsStore
    @Environment(\.dismiss) private var dismiss
    @Environment(\.heronTheme) private var theme
    @State private var host = ""
    @State private var address = ""
    /// Lives only in this sheet: sent once with the request, then cleared. The store never sees it except as the request's argument.
    @State private var password = ""

    private var busy: Bool { store.isBusy(source: "email") }
    private var canConnect: Bool {
        ![host, address, password].contains { $0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty } && !busy
    }

    var body: some View {
        SheetFrame(title: "Connect a mailbox with an app password") {
            Text("Heron reads recent messages in your inbox so it can answer questions about them. It signs in with an app password you create for it.")
                .foregroundStyle(theme.ink2).fixedSize(horizontal: false, vertical: true)
            VStack(alignment: .leading, spacing: Spacing.s2) {
                SectionLabel("Mail server")
                TextField("imap.gmail.com", text: $host).textFieldStyle(.roundedBorder).disabled(busy).accessibilityLabel("Mail server")
            }
            VStack(alignment: .leading, spacing: Spacing.s2) {
                SectionLabel("Email address")
                TextField("you@example.com", text: $address).textFieldStyle(.roundedBorder).disabled(busy).accessibilityLabel("Email address")
            }
            VStack(alignment: .leading, spacing: Spacing.s2) {
                SectionLabel("App password")
                SecureField("App password", text: $password).textFieldStyle(.roundedBorder).disabled(busy).accessibilityLabel("App password")
                    .onSubmit { if canConnect { submit() } }
            }
            Text(SourceCopy.emailHint).font(HeronFont.callout).foregroundStyle(theme.ink2).fixedSize(horizontal: false, vertical: true)
            SheetError(message: store.sourceErrors["email"])
            HStack {
                if busy { ProgressView().controlSize(.small); Text("Signing in…").font(HeronFont.callout).foregroundStyle(theme.ink2) }
                Spacer()
                Button("Cancel") { dismiss() }.buttonStyle(HButtonStyle(.secondary)).keyboardShortcut(.cancelAction).disabled(busy)
                Button("Connect") { submit() }.buttonStyle(HButtonStyle(.primary)).disabled(!canConnect).keyboardShortcut(.defaultAction)
            }
        }
        .onAppear { store.clearSourceError("email") }
        .onDisappear { password = "" }
    }

    private func submit() {
        let request = SourceConnection.email(host: host.trimmingCharacters(in: .whitespacesAndNewlines),
                                             username: address.trimmingCharacters(in: .whitespacesAndNewlines),
                                             password: password, port: nil, tls: nil)
        Task {
            if await store.connect(request) { password = ""; dismiss() }
        }
    }
}

/// The sheet Disconnect opens: it says what Heron will forget, and changes nothing until the user confirms.
struct SourceDisconnectConfirmView: View {
    let source: ConnectionInfo
    let onCancel: () -> Void
    let onConfirm: () -> Void
    @Environment(\.heronTheme) private var theme

    var body: some View {
        SheetFrame(title: "Disconnect \(source.name)?") {
            Text(SourceCopy.forgetSentence(source)).foregroundStyle(theme.ink).fixedSize(horizontal: false, vertical: true)
            Text("You can connect it again later.").foregroundStyle(theme.ink2)
            HStack {
                Spacer()
                Button("Cancel", action: onCancel).buttonStyle(HButtonStyle(.secondary)).keyboardShortcut(.cancelAction)
                Button("Disconnect and forget", action: onConfirm).buttonStyle(HButtonStyle(.danger))
            }
        }
    }
}
