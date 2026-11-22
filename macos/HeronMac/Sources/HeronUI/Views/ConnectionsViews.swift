import SwiftUI
import AppKit
import HeronCore

/// The words on the Connections rows and sheets, in one place so a test can read them.
enum SourceCopy {
    static let scanSentence = "Reads your operating system, time zone, which developer tools are installed, the names of the folders in your home folder, and your Git name. It opens no files."
    static let documentsSentence = "Pick a folder of notes or documents. Heron reads the files in it and remembers what it learns. Folders that hold keys or passwords are refused."
    static let calendarHint = "Google Calendar: Settings › your calendar › “Secret address in iCal format”. iCloud: the calendar's public (subscription) link. Anyone with the link can read the calendar, so keep it private."
    static let emailHint = "Gmail and iCloud need an app-specific password, not your usual one. Create one in your account's security settings. Heron checks the sign-in before saving anything."

    /// What the confirm sheet says Heron will forget.
    static func forgetSentence(_ source: ConnectionInfo) -> String {
        switch source.id {
        case "documents": "Heron will stop reading your folders and forget the notes and facts it learned from them."
        case "calendar": "Heron will stop reading your calendar and forget the events and facts it learned from it."
        case "email": "Heron will delete its saved app password and forget the messages and facts it learned from it."
        case "computer": "Heron will forget what it learned by scanning this Mac."
        default: "Heron will forget what it learned from this source."
        }
    }

    static func connectTitle(_ id: String) -> String { id == "computer" ? "Scan" : "Connect…" }
}

/// Which sheet is open on the rows.
private enum SourceSheet: String, Identifiable {
    case calendar, email
    var id: String { rawValue }
}

/// The four sources as rows with Connect, Scan and Disconnect. Used by Settings › Connections and by the first-run
/// sources step, so both connect the same way.
struct SourceRows: View {
    let store: SettingsStore
    @Environment(\.heronTheme) private var theme
    @State private var sheet: SourceSheet?

    var body: some View {
        let rows = store.settings?.connections ?? []
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
        .sheet(item: $sheet) { which in
            switch which {
            case .calendar: CalendarConnectSheet(store: store)
            case .email: EmailConnectSheet(store: store)
            }
        }
        .sheet(item: Binding(get: { store.pendingDisconnect }, set: { if $0 == nil { store.cancelDisconnect() } })) { row in
            SourceDisconnectConfirmView(source: row, onCancel: { store.cancelDisconnect() },
                                        onConfirm: { Task { await store.confirmDisconnect() } })
        }
    }

    private func connect(_ id: String) {
        store.clearSourceError(id)
        switch id {
        case "documents": chooseFolder()
        case "calendar": sheet = .calendar
        case "email": sheet = .email
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
        SheetFrame(title: "Connect your calendar") {
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
        SheetFrame(title: "Connect your email") {
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
