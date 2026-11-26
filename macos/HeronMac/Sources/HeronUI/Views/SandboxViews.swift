import SwiftUI
import HeronCore

/// The words of Settings › Permissions › Sandbox. Public so tests check them directly.
public enum SandboxCopy {
    public static let trustButton = "Trust the Rust toolchain as it is now"

    public static func toolchainLine(_ status: ToolchainStatus) -> String {
        switch status.state {
        case .trusted:
            return "The Rust toolchain is the one you trusted."
        case .noBaseline:
            return "Heron hasn't recorded the Rust toolchain yet. It records it the first time a sandboxed command runs."
        case .changed:
            let what = status.files.isEmpty ? "files" : status.files.joined(separator: ", ")
            let when = status.during.map { " while “\($0)” was running" } ?? ""
            return "The Rust toolchain changed\(when): \(what). Until you trust it again, sandboxed commands that run it ask first."
        }
    }

    public static func egressSentence(_ mode: String) -> String {
        mode == "open"
            ? "Open: a sandboxed command can reach any host on the internet."
            : "Allowlist: a sandboxed command reaches only the hosts below. Anything else is refused."
    }
}

struct SandboxSection: View {
    let store: SettingsStore
    @State private var hostDraft = ""
    @State private var portDraft = ""
    @Environment(\.heronTheme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s4) {
            egress
            hosts
            ports
            toolchain
        }
    }

    private var egress: some View {
        VStack(alignment: .leading, spacing: Spacing.s2) {
            SectionLabel("Network from the sandbox")
            Picker("Network from the sandbox", selection: Binding(get: { store.settings?.egress ?? "allowlist" },
                                                                 set: { mode in Task { await store.setEgress(mode) } })) {
                Text("Allowlist").tag("allowlist")
                Text("Open").tag("open")
            }
            .pickerStyle(.segmented).labelsHidden().frame(maxWidth: 280).disabled(store.settings == nil || store.isChangingSandbox)
            Text(SandboxCopy.egressSentence(store.settings?.egress ?? "allowlist"))
                .font(HeronFont.callout).foregroundStyle(theme.ink2).fixedSize(horizontal: false, vertical: true)
            if let message = store.sandboxError { errorBanner(message, label: "Couldn't change the sandbox.") }
        }
    }

    private var hosts: some View {
        VStack(alignment: .leading, spacing: Spacing.s2) {
            SectionLabel("Allowed hosts")
            let list = store.settings?.egressAllow ?? []
            if list.isEmpty {
                Text("No hosts. A sandboxed command reaches nothing while the mode is Allowlist.").foregroundStyle(theme.ink2)
            } else {
                RowGroup {
                    ForEach(Array(list.enumerated()), id: \.element) { i, host in
                        if i > 0 { RowDivider() }
                        HStack {
                            Text(host).font(HeronFont.mono).foregroundStyle(theme.ink)
                            Spacer()
                            Button("Remove") { Task { await store.removeEgressHost(host) } }
                                .buttonStyle(HButtonStyle(.secondary)).disabled(store.isChangingSandbox)
                                .accessibilityLabel("Remove the host \(host)")
                        }.padding(.horizontal, Spacing.s4).padding(.vertical, 10)
                    }
                }
            }
            HStack {
                TextField("github.com or *.github.com", text: $hostDraft).textFieldStyle(.roundedBorder).frame(maxWidth: 320)
                    .accessibilityLabel("Host to allow").onSubmit(addHost)
                Button("Add", action: addHost).buttonStyle(HButtonStyle(.secondary))
                    .disabled(hostDraft.trimmingCharacters(in: .whitespaces).isEmpty || store.isChangingSandbox)
            }
        }
    }

    private var ports: some View {
        VStack(alignment: .leading, spacing: Spacing.s2) {
            SectionLabel("Local ports a command can't reach")
            let list = store.settings?.denyLocalPorts ?? []
            if list.isEmpty {
                Text("None beyond Heron's own.").foregroundStyle(theme.ink2)
            } else {
                RowGroup {
                    ForEach(Array(list.enumerated()), id: \.element) { i, port in
                        if i > 0 { RowDivider() }
                        HStack {
                            Text(String(port)).font(HeronFont.mono).monospacedDigit().foregroundStyle(theme.ink)
                            Spacer()
                            Button("Remove") { Task { await store.removeDeniedPort(port) } }
                                .buttonStyle(HButtonStyle(.secondary)).disabled(store.isChangingSandbox)
                                .accessibilityLabel("Remove port \(port)")
                        }.padding(.horizontal, Spacing.s4).padding(.vertical, 10)
                    }
                }
            }
            HStack {
                TextField("Port, such as 5432", text: $portDraft).textFieldStyle(.roundedBorder).frame(maxWidth: 200)
                    .accessibilityLabel("Port to deny").onSubmit(addPort)
                Button("Add", action: addPort).buttonStyle(HButtonStyle(.secondary))
                    .disabled(portDraft.trimmingCharacters(in: .whitespaces).isEmpty || store.isChangingSandbox)
            }
        }
    }

    private var toolchain: some View {
        VStack(alignment: .leading, spacing: Spacing.s2) {
            SectionLabel("Rust toolchain")
            if let status = store.toolchain {
                HStack(alignment: .top, spacing: Spacing.s3) {
                    Image(systemName: status.state == .changed ? "exclamationmark.triangle" : "checkmark.shield")
                        .foregroundStyle(status.state == .changed ? theme.danger : theme.ok)
                    Text(SandboxCopy.toolchainLine(status)).foregroundStyle(theme.ink).fixedSize(horizontal: false, vertical: true)
                }
                .padding(Spacing.s3).frame(maxWidth: .infinity, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: Radius.card, style: .continuous).fill(status.state == .changed ? theme.dangerSoft : theme.okSoft))
                .accessibilityElement(children: .combine)
                if status.state == .changed {
                    Button(SandboxCopy.trustButton) { Task { await store.trustToolchain() } }
                        .buttonStyle(HButtonStyle(.primary)).disabled(store.isTrustingToolchain)
                }
            } else if store.toolchainError == nil {
                SkeletonRows(count: 1)
            }
            if let message = store.toolchainError { errorBanner(message, label: "Couldn't check the toolchain.") }
        }
    }

    private func errorBanner(_ message: String, label: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: Spacing.s2) {
            Image(systemName: "exclamationmark.triangle").foregroundStyle(theme.danger)
            Text(message).foregroundStyle(theme.ink).fixedSize(horizontal: false, vertical: true)
        }
        .padding(Spacing.s3).frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: Radius.control, style: .continuous).fill(theme.dangerSoft))
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(label) \(message)")
    }

    private func addHost() {
        let draft = hostDraft
        Task { if await store.addEgressHost(draft) { hostDraft = "" } }
    }

    private func addPort() {
        let draft = portDraft
        Task { if await store.addDeniedPort(draft) { portDraft = "" } }
    }
}
