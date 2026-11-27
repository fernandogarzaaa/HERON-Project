import SwiftUI
import AppKit
import HeronCore

/// The conversation: thread, activity track beside each reply, and the composer. Live approvals show as
/// a sheet from the main window.
struct ChatView: View {
    @Bindable var model: AppModel
    /// Open every reply's steps, as if "N steps" had been clicked. Snapshots use it to show tool output.
    var expandSteps = false
    @Environment(\.heronTheme) private var theme
    @Environment(\.heronTextScale) private var scale
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    private var chat: ChatStore { model.chat }

    var body: some View {
        VStack(spacing: 0) {
            if chat.messages.isEmpty && !chat.isRunning {
                firstConversation
            } else {
                thread
            }
            composer
        }
        .onExitCommand { Task { await chat.stop() } }
        .task(id: chat.answeredElsewhere) {
            guard chat.answeredElsewhere else { return }
            try? await Task.sleep(for: .seconds(3))
            chat.clearAnsweredElsewhere()
        }
    }

    // MARK: Empty state

    private var firstConversation: some View {
        VStack(spacing: Spacing.s5) {
            EmptyStateView(title: "What should we get done?",
                           text: "Heron works in \(model.state?.workspace ?? "~/Work"). It can read and write files there, run commands, search the web and remember what matters to you.")
            HStack(spacing: Spacing.s2) {
                starter("Prep my day") { Task { await model.prepMyDay() } }
                starter("Tidy my Downloads folder") { Task { await chat.send("Tidy my Downloads folder.") } }
                starter("Draft a reply to my latest email") { Task { await chat.send("Draft a reply to my latest email.") } }
            }
            .padding(.bottom, Spacing.s8)
        }
    }

    private func starter(_ title: String, action: @escaping () -> Void) -> some View {
        Button(title, action: action).buttonStyle(HButtonStyle(.secondary))
    }

    // MARK: Thread

    /// The transcript is the page's one scroll container. `defaultScrollAnchor(.bottom)` keeps it on the newest
    /// message while a reply streams in, and leaves it where it is once the user has scrolled up to read.
    /// Sending a message always jumps to the bottom.
    private var thread: some View {
        ScrollViewReader { proxy in
            HScroll {
                LazyVStack(alignment: .leading, spacing: Spacing.s5) {
                    ForEach(chat.messages) { message in
                        MessageRow(message: message, model: model, scale: scale, expandSteps: expandSteps).id(message.id)
                    }
                    if chat.answeredElsewhere {
                        Label("Answered from your phone", systemImage: "iphone").font(HeronFont.sub).foregroundStyle(theme.ink2)
                            .transition(.opacity)
                    }
                    if let line = chat.runStatusLine {
                        Text(line).font(HeronFont.sub).foregroundStyle(theme.ink3)
                    }
                    Color.clear.frame(height: 1).id(Self.bottomID)
                }
                .frame(maxWidth: Spacing.chatColumn, alignment: .leading)
                .padding(.horizontal, Spacing.s6).padding(.vertical, Spacing.s5)
                .frame(maxWidth: .infinity)
            }
            .defaultScrollAnchor(.bottom)
            .onChange(of: chat.messages.last?.id) { _, _ in
                guard chat.messages.last?.role == .user else { return }
                withAnimation(Motion.animation(Motion.std, reduceMotion: reduceMotion)) { proxy.scrollTo(Self.bottomID, anchor: .bottom) }
            }
        }
    }

    static let bottomID = "bottom"

    // MARK: Composer

    private var composer: some View {
        VStack(spacing: Spacing.s2) {
            if let error = chat.composerError {
                ErrorCardView(card: error, model: model).frame(maxWidth: Spacing.chatColumn)
            }
            HStack(alignment: .bottom, spacing: Spacing.s2) {
                TextField(chat.composerPlaceholder, text: Bindable(chat).composer, axis: .vertical)
                    .textFieldStyle(.plain).lineLimit(1...6)
                    .disabled(chat.liveApproval != nil)
                    .onSubmit { Task { await chat.send() } }
                    .accessibilityLabel("Message")
                if chat.isRunning {
                    Button { Task { await chat.stop() } } label: { Image(systemName: "stop.circle.fill").font(.title2) }
                        .buttonStyle(.plain).foregroundStyle(chat.isStopping ? theme.ink3 : theme.accent)
                        .disabled(chat.isStopping)
                        .accessibilityLabel(chat.isStopping ? "Stopping Heron" : "Stop Heron").help(chat.isStopping ? "Stopping…" : "Stop (Esc)")
                } else {
                    Button { Task { await chat.send() } } label: { Image(systemName: "arrow.up.circle.fill").font(.title2) }
                        .buttonStyle(.plain).foregroundStyle(chat.canSend ? theme.accent : theme.ink3)
                        .disabled(!chat.canSend)
                        .accessibilityLabel("Send").help("Send (Return)")
                }
            }
            .padding(Spacing.s3)
            .background(RoundedRectangle(cornerRadius: Radius.card, style: .continuous).fill(theme.raised))
            .overlay(RoundedRectangle(cornerRadius: Radius.card, style: .continuous).stroke(theme.cardBorder, lineWidth: theme.cardBorderWidth))
            .frame(maxWidth: Spacing.chatColumn)
            HStack {
                Label(model.state?.workspace ?? "~/Work", systemImage: "folder").labelStyle(.titleAndIcon)
                Spacer()
                Text("Esc stops Heron")
            }
            .font(HeronFont.sub).foregroundStyle(theme.ink3).frame(maxWidth: Spacing.chatColumn)
        }
        .padding(Spacing.s4)
        .frame(maxWidth: .infinity)
    }
}

// MARK: Messages

private struct MessageRow: View {
    let message: ChatMessage
    @Bindable var model: AppModel
    let scale: CGFloat
    let expandSteps: Bool
    @Environment(\.heronTheme) private var theme

    var body: some View {
        switch message.role {
        case .user:
            HStack {
                Spacer(minLength: Spacing.s12)
                Text(message.text).font(HeronFont.reading(scale: scale)).foregroundStyle(theme.ink)
                    .padding(.horizontal, Spacing.s3).padding(.vertical, Spacing.s2)
                    .background(RoundedRectangle(cornerRadius: Radius.card, style: .continuous).fill(theme.raised))
                    .textSelection(.enabled)
            }
            .accessibilityElement(children: .combine)
            .accessibilityLabel("You: \(message.text)")
        case .assistant:
            HStack(alignment: .top, spacing: Spacing.s3) {
                AvatarView(style: model.preferences.avatarStyle, state: message.finished ? .idle : model.avatar.state, name: model.agentName, size: 28,
                           image: model.preferences.avatarStyle == .image ? NSImage(contentsOf: DisplayPreferences.avatarImageURL) : nil)
                VStack(alignment: .leading, spacing: Spacing.s2) {
                    if !message.steps.isEmpty { ActivityTrack(steps: message.steps, finished: message.finished, startsExpanded: expandSteps) }
                    if !message.text.isEmpty {
                        Text(message.text).font(HeronFont.reading(scale: scale)).foregroundStyle(theme.ink)
                            .fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
                    }
                    if let error = message.error { ErrorCardView(card: error, model: model) }
                }
            }
            .accessibilityElement(children: .contain)
        }
    }
}

/// The thin vertical track of the steps a run took. It collapses to "3 steps" once the reply finishes.
struct ActivityTrack: View {
    let steps: [ChatStep]
    let finished: Bool
    @Environment(\.heronTheme) private var theme
    @State private var expanded: Bool

    init(steps: [ChatStep], finished: Bool, startsExpanded: Bool = false) {
        self.steps = steps
        self.finished = finished
        self._expanded = State(initialValue: startsExpanded)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if finished && !expanded {
                Button { withAnimation(Motion.std) { expanded = true } } label: {
                    Label("\(steps.count) step\(steps.count == 1 ? "" : "s")", systemImage: "chevron.right").font(HeronFont.sub)
                }
                .buttonStyle(.plain).foregroundStyle(theme.ink2)
                .accessibilityLabel("\(steps.count) steps. Show")
            } else {
                if finished {
                    Button { withAnimation(Motion.std) { expanded = false } } label: {
                        Label("Hide steps", systemImage: "chevron.down").font(HeronFont.sub)
                    }.buttonStyle(.plain).foregroundStyle(theme.ink2)
                }
                HStack(alignment: .top, spacing: Spacing.s3) {
                    Rectangle().fill(theme.hairline).frame(width: 2)
                    VStack(alignment: .leading, spacing: Spacing.s2) {
                        ForEach(steps) { step in StepRow(step: step) }
                    }
                }
                .padding(.top, Spacing.s1)
                .accessibilityElement(children: .contain)
                .accessibilityLabel("Steps Heron took")
            }
        }
    }

    static func symbol(for tool: String) -> String {
        switch tool {
        case "upcoming_events", "calendar": "calendar"
        case "read_file", "write_file", "edit_file": "doc.text"
        case "list_dir", "find_files": "folder"
        case "shell", "run_command": "terminal"
        case "send_email", "read_email", "search_email": "envelope"
        case "web_search", "search_files": "magnifyingglass"
        default: "wrench.and.screwdriver"
        }
    }
}

private struct StepRow: View {
    let step: ChatStep
    @Environment(\.heronTheme) private var theme

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: Spacing.s2) {
            if step.status == .waiting {
                Circle().fill(theme.waitingMark).frame(width: 8, height: 8).accessibilityHidden(true)
            } else {
                Image(systemName: ActivityTrack.symbol(for: step.name)).foregroundStyle(theme.ink2).frame(width: 16)
            }
            // Tool output can be long: it wraps, and the transcript (the page's one scroll view) scrolls.
            Text(step.sentence).foregroundStyle(step.status == .waiting ? theme.waiting : theme.ink)
                .fixedSize(horizontal: false, vertical: true)
            if step.untrusted { HTag("Outside content", tone: .outside, symbol: "globe") }
            if step.status == .failed { HTag("Failed", tone: .danger) }
            Spacer(minLength: Spacing.s2)
            Text([step.name, step.durationMs.map { "\($0) ms" }].compactMap { $0 }.joined(separator: " · "))
                .font(HeronFont.mono).foregroundStyle(theme.ink3)
        }
        .font(HeronFont.callout)
        .accessibilityElement(children: .combine)
    }
}

/// An error under a reply: a `dangerSoft` card with a title, text and actions.
struct ErrorCardView: View {
    let card: ChatErrorCard
    @Bindable var model: AppModel
    @Environment(\.heronTheme) private var theme
    @Environment(\.openSettings) private var openSettings

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s2) {
            Label(card.title, systemImage: "exclamationmark.triangle").font(HeronFont.title3).foregroundStyle(theme.danger)
            Text(card.text).foregroundStyle(theme.ink).fixedSize(horizontal: false, vertical: true)
            if !card.actions.isEmpty {
                HStack { ForEach(Array(card.actions.enumerated()), id: \.offset) { _, a in button(a) } }
            }
        }
        .padding(Spacing.s4)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: Radius.card, style: .continuous).fill(theme.dangerSoft))
        .accessibilityElement(children: .contain)
    }

    @ViewBuilder private func button(_ action: ChatErrorAction) -> some View {
        switch action {
        case .copyCommand(let c):
            Button("Copy command") { NSPasteboard.general.clearContents(); NSPasteboard.general.setString(c, forType: .string) }
                .buttonStyle(HButtonStyle(.secondary))
        case .tryAgain:
            Button("Try again") { Task { await model.chat.retry() } }
                .buttonStyle(HButtonStyle(.secondary))
        case .continueUntested(let provider, let version):
            Button("Continue with \(version)") {
                Task {
                    if let p = model.settings.providers.first(where: { $0.displayName == provider || $0.id == provider }) { await model.settings.acceptVersion(p) }
                }
            }.buttonStyle(HButtonStyle(.primary))
        case .modelSettings:
            Button("Model settings") { model.openSettings(.model); openSettings() }.buttonStyle(HButtonStyle(.secondary))
        case .stopReply:
            Button("Stop the current reply") { Task { await model.chat.stop() } }.buttonStyle(HButtonStyle(.secondary))
        case .chooseFolder:
            Button("Choose folder…") { model.openSettings(.general); openSettings() }.buttonStyle(HButtonStyle(.secondary))
        case .showCause:
            Button("Show the cause") { model.openSettings(.permissions); openSettings() }.buttonStyle(HButtonStyle(.secondary))
        }
    }
}
