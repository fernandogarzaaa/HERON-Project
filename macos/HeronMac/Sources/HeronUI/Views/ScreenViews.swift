import SwiftUI
import AppKit
import HeronCore

// MARK: Shared scaffolding

/// A titled screen body: skeleton rows while loading, an empty state, or the content.
struct ScreenScaffold<Empty: View, Content: View>: View {
    let loading: Bool
    let failure: String?
    let isEmpty: Bool
    let empty: Empty
    @ViewBuilder var content: Content
    @Environment(\.heronTheme) private var theme

    var body: some View {
        if let failure {
            EmptyStateView(title: "Couldn't load this", text: failure)
        } else if loading && isEmpty {
            SkeletonRows().frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        } else if isEmpty {
            empty
        } else {
            HScroll { content.padding(.horizontal, Spacing.s6).padding(.top, Spacing.s5).padding(.bottom, Spacing.s6) }
        }
    }
}

extension LoadState {
    var isLoading: Bool { self == .loading }
    var failureText: String? { if case .failed(let m) = self { m } else { nil } }
}

/// Rows in a card, separated by hairlines.
struct RowGroup<Content: View>: View {
    @ViewBuilder var content: Content
    @Environment(\.heronTheme) private var theme
    var body: some View {
        VStack(spacing: 0) { content }
            .background(RoundedRectangle(cornerRadius: Radius.card, style: .continuous).fill(theme.surface))
            .overlay(RoundedRectangle(cornerRadius: Radius.card, style: .continuous).stroke(theme.cardBorder, lineWidth: theme.cardBorderWidth))
            .clipShape(RoundedRectangle(cornerRadius: Radius.card, style: .continuous))
    }
}

struct RowDivider: View {
    @Environment(\.heronTheme) private var theme
    var body: some View { Rectangle().fill(theme.hairline).frame(height: 1) }
}

extension ParkedApproval {
    /// A parked item as an approval request, so the sheet's impact lines can describe it.
    var asRequest: ApprovalRequest {
        ApprovalRequest(id: "parked-\(id)", tool: tool, summary: summary, risk: risk, detail: detail, alwaysRule: nil,
                        canAlways: false, canSession: false, tainted: tainted, outsideSource: nil, reason: nil, sessionID: sessionID)
    }
}

// MARK: Today

struct TodayView: View {
    @Bindable var model: AppModel
    @Environment(\.heronTheme) private var theme

    private var today: TodayStore { model.today }

    var body: some View {
        ScreenScaffold(loading: today.loadState.isLoading, failure: today.loadState.failureText, isEmpty: today.summary == nil,
                       empty: EmptyStateView(title: "Nothing to show yet", text: "Today fills in once Heron has loaded your calendar and tasks.")) {
            let now = Date()
            VStack(alignment: .leading, spacing: Spacing.s5) {
                VStack(alignment: .leading, spacing: Spacing.s1) {
                    Text(today.greeting(now: now)).font(HeronFont.large).foregroundStyle(theme.ink).accessibilityAddTraits(.isHeader)
                    Text(today.subtitle(now: now)).foregroundStyle(theme.ink2)
                }
                prepCard
                HStack(alignment: .top, spacing: Spacing.s5) {
                    eventsPanel.frame(maxWidth: .infinity, alignment: .topLeading)
                    VStack(alignment: .leading, spacing: Spacing.s5) { waitingPanel; duePanel }
                        .frame(maxWidth: .infinity, alignment: .topLeading)
                }
            }
        }
    }

    private var prepCard: some View {
        HStack(spacing: Spacing.s4) {
            AvatarView(style: model.preferences.avatarStyle, state: model.avatar.state, name: model.agentName, size: 44)
            VStack(alignment: .leading, spacing: Spacing.s1) {
                Text("Prep my day").font(HeronFont.title3).foregroundStyle(theme.ink)
                Text("Heron reads today's calendar, your due tasks and anything waiting, then suggests an order for the day. It can draft replies and add tasks, and asks before saving any.")
                    .font(HeronFont.callout).foregroundStyle(theme.ink2).fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: Spacing.s4)
            Button("Prep my day") { Task { await model.prepMyDay() } }.buttonStyle(HButtonStyle(.primary))
        }
        .heronCard()
    }

    private var eventsPanel: some View {
        VStack(alignment: .leading, spacing: Spacing.s2) {
            SectionLabel("Today's events")
            if let events = today.summary?.events, !events.isEmpty {
                RowGroup {
                    ForEach(Array(events.enumerated()), id: \.element.id) { i, e in
                        if i > 0 { RowDivider() }
                        HStack(alignment: .firstTextBaseline, spacing: Spacing.s3) {
                            Text(Fmt.time(e.start)).font(HeronFont.body).monospacedDigit().foregroundStyle(theme.ink2).frame(width: 48, alignment: .leading)
                            VStack(alignment: .leading, spacing: 1) {
                                Text(e.title).foregroundStyle(theme.ink)
                                if let place = e.place { Text(place).font(HeronFont.callout).foregroundStyle(theme.ink2) }
                            }
                            Spacer()
                        }
                        .padding(.horizontal, Spacing.s4).padding(.vertical, 10)
                        .accessibilityElement(children: .combine)
                    }
                }
            } else {
                Text("Nothing on the calendar today.").foregroundStyle(theme.ink2)
            }
        }
    }

    private var waitingPanel: some View {
        VStack(alignment: .leading, spacing: Spacing.s2) {
            SectionLabel("Waiting for you")
            if model.approvals.parked.isEmpty && model.approvals.live.isEmpty {
                Text("Nothing is waiting for you.").foregroundStyle(theme.ink2)
            } else {
                RowGroup {
                    ForEach(Array(model.approvals.parked.enumerated()), id: \.element.id) { i, p in
                        if i > 0 { RowDivider() }
                        HStack {
                            Circle().fill(theme.waitingMark).frame(width: 8, height: 8).accessibilityHidden(true)
                            Text(p.summary).foregroundStyle(theme.ink).lineLimit(1)
                            Spacer()
                            Button("Review") { model.review(parked: p.id) }.buttonStyle(HButtonStyle(.link))
                        }
                        .padding(.horizontal, Spacing.s4).padding(.vertical, 10)
                    }
                    ForEach(model.approvals.live) { r in
                        RowDivider()
                        HStack {
                            Circle().fill(theme.waitingMark).frame(width: 8, height: 8).accessibilityHidden(true)
                            Text(r.summary).foregroundStyle(theme.ink).lineLimit(1)
                            Spacer()
                            Button("Review") { model.review(live: r) }.buttonStyle(HButtonStyle(.link))
                        }
                        .padding(.horizontal, Spacing.s4).padding(.vertical, 10)
                    }
                }
            }
        }
    }

    private var duePanel: some View {
        VStack(alignment: .leading, spacing: Spacing.s2) {
            SectionLabel("Due")
            if let due = today.summary?.dueTasks, !due.isEmpty {
                RowGroup {
                    ForEach(Array(due.enumerated()), id: \.element.id) { i, t in
                        if i > 0 { RowDivider() }
                        HStack {
                            Text(t.title).foregroundStyle(theme.ink).lineLimit(1)
                            Spacer()
                            HTag(t.overdue ? "Overdue" : "Today", tone: t.overdue ? .danger : .neutral)
                            Button("Done") {
                                Task {
                                    if let task = model.tasks.tasks.first(where: { $0.id == t.id }) { await model.tasks.toggle(task) } else { await model.tasks.load() }
                                    await model.today.load()
                                }
                            }.buttonStyle(HButtonStyle(.link))
                        }
                        .padding(.horizontal, Spacing.s4).padding(.vertical, 10)
                    }
                }
            } else {
                Text("No tasks are due.").foregroundStyle(theme.ink2)
            }
        }
    }
}

// MARK: Inbox

struct InboxView: View {
    @Bindable var model: AppModel
    @Environment(\.heronTheme) private var theme

    private var inbox: InboxStore { model.inbox }

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Picker("Filter", selection: Bindable(inbox).filter) {
                    ForEach(InboxStore.Filter.allCases, id: \.self) { Text($0.rawValue).tag($0) }
                }
                .pickerStyle(.segmented).labelsHidden().frame(maxWidth: 260)
                Spacer()
                Button("Mark all as read") { Task { await inbox.markAllRead() } }.buttonStyle(HButtonStyle(.link))
                    .disabled(inbox.unreadCount == 0)
            }
            .padding(.horizontal, Spacing.s4).padding(.vertical, Spacing.s2)
            Rectangle().fill(theme.hairline).frame(height: 1)
            if inbox.isEmpty && !inbox.loadState.isLoading {
                EmptyStateView(title: "Nothing waiting for you",
                               text: "Approvals Heron couldn't ask about, suggestions and schedule results land here.",
                               actionTitle: "New schedule") { model.go(.schedules) }
            } else {
                HStack(spacing: 0) {
                    list.frame(width: 360)
                    Rectangle().fill(theme.hairline).frame(width: 1)
                    detail.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                }
            }
        }
    }

    private var list: some View {
        List(selection: Bindable(inbox).selection) {
            if inbox.showsWaiting && inbox.approvals.waitingCount > 0 {
                Section {
                    ForEach(inbox.approvals.live) { r in
                        row(title: r.summary, summary: r.reason ?? r.detail, time: nil, dot: theme.waitingMark).tag(InboxStore.Selection.live(r.id))
                    }
                    ForEach(inbox.approvals.parked) { p in
                        row(title: p.summary, summary: "From “\(p.origin)”", time: Fmt.relative(p.timestamp), dot: theme.waitingMark)
                            .tag(InboxStore.Selection.parked(p.id))
                    }
                } header: { SectionLabel("Waiting for you") }
            }
            if inbox.showsDelivered && !inbox.suggestions.isEmpty {
                Section {
                    ForEach(inbox.suggestions) { s in
                        row(title: s.title, summary: s.why, time: Fmt.relative(s.timestamp), dot: s.status == "new" ? theme.accent : nil)
                            .tag(InboxStore.Selection.suggestion(s.id))
                    }
                } header: { SectionLabel("Suggestions") }
            }
            if inbox.showsDelivered && !inbox.items.isEmpty {
                Section {
                    ForEach(inbox.items) { i in
                        row(title: i.title, summary: i.body, time: Fmt.relative(i.timestamp), dot: i.read ? nil : theme.accent)
                            .tag(InboxStore.Selection.delivered(i.id))
                    }
                } header: { SectionLabel("Delivered") }
            }
        }
        .listStyle(.inset).scrollContentBackground(.hidden)
    }

    private func row(title: String, summary: String, time: String?, dot: Color?) -> some View {
        HStack(alignment: .top, spacing: Spacing.s2) {
            Circle().fill(dot ?? .clear).frame(width: 8, height: 8).padding(.top, 6).accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 1) {
                Text(title).font(HeronFont.body.weight(.semibold)).foregroundStyle(theme.ink).lineLimit(1)
                Text(summary).font(HeronFont.callout).foregroundStyle(theme.ink2).lineLimit(1)
            }
            Spacer(minLength: 0)
            if let time { Text(time).font(HeronFont.sub).foregroundStyle(theme.ink3) }
        }
        .padding(.vertical, 2)
    }

    @ViewBuilder private var detail: some View {
        switch inbox.selection {
        case .parked(let id):
            if let p = inbox.approvals.parked.first(where: { $0.id == id }) { parkedDetail(p) } else { placeholder }
        case .live(let id):
            if let r = inbox.approvals.live.first(where: { $0.id == id }) {
                ApprovalSheetView(request: r, avatarStyle: model.preferences.avatarStyle, agentName: model.agentName) { decision in
                    Task { await inbox.approvals.answer(r, decision) }
                }.padding(Spacing.s6)
            } else { placeholder }
        case .suggestion(let id):
            if let s = inbox.suggestions.first(where: { $0.id == id }) { suggestionDetail(s) } else { placeholder }
        case .delivered(let id):
            if let i = inbox.items.first(where: { $0.id == id }) {
                VStack(alignment: .leading, spacing: Spacing.s3) {
                    Text(i.title).font(HeronFont.title2).foregroundStyle(theme.ink)
                    Text(i.body).foregroundStyle(theme.ink)
                }.padding(Spacing.s6).textSelection(.enabled)
            } else { placeholder }
        case nil: placeholder
        }
    }

    private var placeholder: some View {
        Text("Choose something from the list.").foregroundStyle(theme.ink3).frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func parkedDetail(_ p: ParkedApproval) -> some View {
        let request = p.asRequest
        return VStack(alignment: .leading, spacing: Spacing.s4) {
            Text(ApprovalCopy.title(request)).font(HeronFont.title2).foregroundStyle(theme.ink)
            Text("Heron wanted to do this at \(Fmt.time(p.timestamp)) during “\(p.origin)”. Nobody was here to answer, so it skipped this step and carried on.")
                .foregroundStyle(theme.ink2).fixedSize(horizontal: false, vertical: true)
            Text(p.detail.isEmpty ? p.summary : p.detail).font(HeronFont.mono).foregroundStyle(theme.ink)
                .frame(maxWidth: .infinity, alignment: .leading).padding(Spacing.s3)
                .background(RoundedRectangle(cornerRadius: Radius.control, style: .continuous).fill(theme.raised)).textSelection(.enabled)
            VStack(alignment: .leading, spacing: Spacing.s2) {
                ForEach(Array(ApprovalCopy.impact(request).enumerated()), id: \.offset) { _, line in
                    Label(line.text, systemImage: line.symbol).foregroundStyle(theme.ink)
                }
            }
            HStack {
                Button("Dismiss") { Task { await inbox.approvals.answerParked(p, .dismiss) } }.buttonStyle(HButtonStyle(.secondary))
                Button(approveTitle(p)) { Task { await inbox.approvals.answerParked(p, .approve) } }
                    .buttonStyle(HButtonStyle(p.isHighRisk ? .danger : .primary))
                Button("Open “\(p.origin)”") { Task { await model.openConversation(p.sessionID) } }.buttonStyle(HButtonStyle(.link))
            }
            Text("Approving runs this one step now, with the job's permissions. It doesn't change what the job may do next time.")
                .font(HeronFont.callout).foregroundStyle(theme.ink3)
        }
        .padding(Spacing.s6)
    }

    private func approveTitle(_ p: ParkedApproval) -> String {
        let name = ApprovalCopy.primaryTitle(p.asRequest)
        return name == "Allow" ? "Approve" : "Approve and \(name.lowercased())"
    }

    private func suggestionDetail(_ s: Suggestion) -> some View {
        VStack(alignment: .leading, spacing: Spacing.s4) {
            Text(s.title).font(HeronFont.title2).foregroundStyle(theme.ink)
            Text(s.why).foregroundStyle(theme.ink2)
            HStack {
                Button("Not now") { Task { await inbox.answer(s, .dismiss) } }.buttonStyle(HButtonStyle(.secondary))
                Button("Do it") {
                    Task {
                        await inbox.answer(s, .accept)
                        model.chat.startNewConversation()
                        model.selection = .chat
                        await model.chat.send(s.prompt, title: s.title)
                    }
                }.buttonStyle(HButtonStyle(.primary))
            }
        }.padding(Spacing.s6)
    }
}

// MARK: Memory

struct MemoryView: View {
    @Bindable var model: AppModel
    @Environment(\.heronTheme) private var theme
    @Environment(\.openSettings) private var openSettings
    @State private var editingID: Int?
    @State private var draft = ""
    @State private var addText = ""
    @FocusState private var searchFocused: Bool

    private var memory: MemoryStore { model.memory }

    var body: some View {
        VStack(spacing: 0) {
            ScreenScaffold(loading: memory.loadState.isLoading, failure: memory.loadState.failureText,
                           isEmpty: memory.isFirstUse || memory.noResults,
                           empty: emptyState) {
                VStack(alignment: .leading, spacing: Spacing.s5) {
                    VStack(alignment: .leading, spacing: Spacing.s2) {
                        Text("What Heron knows about you").font(HeronFont.title).foregroundStyle(theme.ink).accessibilityAddTraits(.isHeader)
                        Text("It's kept in a file on this Mac. Heron uses confirmed memories in every conversation. New ones it learned from email or the web wait here until you confirm them.")
                            .foregroundStyle(theme.ink2)
                    }
                    controls
                    if !memory.toCheck.isEmpty { section("To check", memory.toCheck, checking: true) }
                    if !memory.confirmed.isEmpty { section("Confirmed", memory.confirmed, checking: false) }
                }
            }
            if let m = memory.lastForgotten {
                HStack {
                    Text("Forgot “\(m.content.prefix(40))”").foregroundStyle(theme.ink)
                    Spacer()
                    Button("Undo") { Task { await memory.undoForget() } }.buttonStyle(HButtonStyle(.link))
                }
                .padding(Spacing.s3).background(theme.raised).transition(.opacity)
                .task(id: m.id) { try? await Task.sleep(for: .seconds(5)); memory.clearUndo() }
            }
        }
    }

    @ViewBuilder private var emptyState: some View {
        if memory.noResults {
            EmptyStateView(title: "No memories match “\(memory.query)”", text: "Try a shorter word, or clear the search.",
                           actionTitle: "Clear search") { Task { await memory.clearSearch() } }
        } else {
            EmptyStateView(title: "Heron doesn't know you yet",
                           text: "Tell it a few things about you, your work and the people you mention, so it needs less explaining.",
                           actionTitle: "Tell Heron about you") { model.openSettings(.heron); openSettings() }
        }
    }

    private var controls: some View {
        HStack {
            Picker("Filter", selection: Bindable(memory).filter) {
                ForEach(MemoryStore.Filter.allCases, id: \.self) { Text($0.rawValue).tag($0) }
            }.pickerStyle(.segmented).labelsHidden()
            HStack(spacing: Spacing.s1) {
                Image(systemName: "magnifyingglass").foregroundStyle(theme.ink3)
                TextField("Search", text: Bindable(memory).query).textFieldStyle(.plain).focused($searchFocused)
                    .onSubmit { Task { await memory.load() } }
                    .accessibilityLabel("Search memories")
            }
            .padding(6).frame(width: 180)
            .background(RoundedRectangle(cornerRadius: Radius.control, style: .continuous).fill(theme.raised))
            Button("Search") { searchFocused = true }.keyboardShortcut("f", modifiers: .command).opacity(0).frame(width: 0)
        }
    }

    private func section(_ title: String, _ items: [Memory], checking: Bool) -> some View {
        VStack(alignment: .leading, spacing: Spacing.s2) {
            SectionLabel(title)
            RowGroup {
                ForEach(Array(items.enumerated()), id: \.element.id) { i, m in
                    if i > 0 { RowDivider() }
                    memoryRow(m, checking: checking)
                }
            }
        }
    }

    private func memoryRow(_ m: Memory, checking: Bool) -> some View {
        VStack(alignment: .leading, spacing: Spacing.s1) {
            if editingID == m.id {
                TextField("Memory", text: $draft, axis: .vertical).textFieldStyle(.roundedBorder)
                HStack {
                    Button("Save") { Task { await memory.edit(m, content: draft); editingID = nil } }.buttonStyle(HButtonStyle(.primary))
                    Button("Cancel") { editingID = nil }.buttonStyle(HButtonStyle(.secondary))
                }
            } else {
                Text(m.content).foregroundStyle(theme.ink).fixedSize(horizontal: false, vertical: true)
                HStack(spacing: Spacing.s2) {
                    if checking { HTag("Not used until you confirm", tone: .waiting) }
                    Text(sourceLine(m, checking: checking)).font(HeronFont.sub).foregroundStyle(theme.ink3)
                    Spacer()
                    if checking { Button("Confirm") { Task { await memory.confirm(m) } }.buttonStyle(HButtonStyle(.secondary)) }
                    Button("Edit…") { draft = m.content; editingID = m.id }.buttonStyle(HButtonStyle(.link))
                    Button("Forget") { Task { await memory.forget(m) } }.buttonStyle(HButtonStyle(.link))
                }
            }
        }
        .padding(.horizontal, Spacing.s4).padding(.vertical, 10)
    }

    private func sourceLine(_ m: Memory, checking: Bool) -> String {
        let source: String = switch m.source {
        case "user": "You told Heron"
        case "email": "From email"
        case "calendar": "From your calendar"
        case "documents": "From documents"
        case "agent", "auto": "Heron noticed"
        default: "From \(m.source)"
        }
        return checking ? "\(source) · \(m.kind) · \(Fmt.relative(m.createdAt))" : "\(source) · used \(m.uses) time\(m.uses == 1 ? "" : "s")"
    }
}

// MARK: Tasks

struct TasksView: View {
    @Bindable var model: AppModel
    @Environment(\.heronTheme) private var theme
    @State private var showDone = false

    private var tasks: TasksStore { model.tasks }

    var body: some View {
        ScreenScaffold(loading: tasks.loadState.isLoading, failure: tasks.loadState.failureText, isEmpty: tasks.tasks.isEmpty,
                       empty: EmptyStateView(title: "No open tasks", text: "Add one here, or ask Heron in a conversation to keep track of something.",
                                             actionTitle: "Add a task") { Task { await tasks.add() } }) {
            let now = Date()
            VStack(alignment: .leading, spacing: Spacing.s5) {
                VStack(alignment: .leading, spacing: Spacing.s2) {
                    SectionLabel("Yours")
                    RowGroup {
                        ForEach(Array(tasks.yours.enumerated()), id: \.element.id) { i, t in
                            if i > 0 { RowDivider() }
                            taskRow(t, now: now)
                        }
                        if !tasks.yours.isEmpty { RowDivider() }
                        HStack {
                            Image(systemName: "plus").foregroundStyle(theme.ink3)
                            TextField("Add a task", text: Bindable(tasks).newTitle).textFieldStyle(.plain)
                                .onSubmit { Task { await tasks.add() } }.accessibilityLabel("Add a task")
                        }.padding(.horizontal, Spacing.s4).padding(.vertical, 10)
                    }
                }
                if !tasks.heronDoing.isEmpty {
                    VStack(alignment: .leading, spacing: Spacing.s2) {
                        SectionLabel("Heron is doing")
                        RowGroup {
                            ForEach(Array(tasks.heronDoing.enumerated()), id: \.element.id) { i, t in
                                if i > 0 { RowDivider() }
                                HStack {
                                    VStack(alignment: .leading, spacing: 1) {
                                        Text(t.title).foregroundStyle(theme.ink)
                                        if let p = t.progress { Text(p).font(HeronFont.callout).foregroundStyle(theme.ink2).monospacedDigit() }
                                    }
                                    Spacer()
                                    HTag("Working", tone: .accent)
                                }.padding(.horizontal, Spacing.s4).padding(.vertical, 10)
                            }
                        }
                    }
                }
                if !tasks.doneThisWeek.isEmpty {
                    VStack(alignment: .leading, spacing: Spacing.s2) {
                        Button { showDone.toggle() } label: {
                            Label("Done this week (\(tasks.doneThisWeek.count))", systemImage: showDone ? "chevron.down" : "chevron.right")
                                .font(HeronFont.subLabel).foregroundStyle(theme.ink3)
                        }.buttonStyle(.plain)
                        if showDone {
                            RowGroup {
                                ForEach(Array(tasks.doneThisWeek.enumerated()), id: \.element.id) { i, t in
                                    if i > 0 { RowDivider() }
                                    HStack {
                                        Button { Task { await tasks.toggle(t) } } label: { Image(systemName: "checkmark.circle.fill").foregroundStyle(theme.ok) }
                                            .buttonStyle(.plain).accessibilityLabel("Mark \(t.title) as not done")
                                        Text(t.title).strikethrough().foregroundStyle(theme.ink3)
                                        Spacer()
                                    }.padding(.horizontal, Spacing.s4).padding(.vertical, 10)
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    private func taskRow(_ t: TaskItem, now: Date) -> some View {
        let tag = DueTag.make(due: t.due, now: now)
        return HStack {
            Button { Task { await tasks.toggle(t) } } label: { Image(systemName: "circle").foregroundStyle(theme.ink3) }
                .buttonStyle(.plain).accessibilityLabel("Mark \(t.title) as done")
            Text(t.title).foregroundStyle(theme.ink)
            Spacer()
            HTag(tag.title, tone: tag == .overdue ? .danger : tag == .today ? .accent : .neutral)
        }
        .padding(.horizontal, Spacing.s4).padding(.vertical, 10)
    }
}

// MARK: Schedules

struct SchedulesView: View {
    @Bindable var model: AppModel
    @Environment(\.heronTheme) private var theme

    private var schedules: SchedulesStore { model.schedules }

    var body: some View {
        ScreenScaffold(loading: schedules.loadState.isLoading, failure: schedules.loadState.failureText, isEmpty: schedules.schedules.isEmpty,
                       empty: EmptyStateView(title: "No schedules yet",
                                             text: "Have Heron brief you every morning or watch for an email, without being asked each time.",
                                             actionTitle: "New schedule") { model.newSchedule() }) {
            VStack(alignment: .leading, spacing: Spacing.s5) {
                Text("Things Heron does at a set time or when something happens, while Heron is running. Scheduled runs can read and search, and change nothing unless you allow more.")
                    .foregroundStyle(theme.ink2)
                group("Active", schedules.active)
                group("Held", schedules.held)
            }
        }
    }

    @ViewBuilder private func group(_ title: String, _ items: [ScheduleItem]) -> some View {
        if !items.isEmpty {
            VStack(alignment: .leading, spacing: Spacing.s2) {
                SectionLabel(title)
                RowGroup {
                    ForEach(Array(items.enumerated()), id: \.element.id) { i, s in
                        if i > 0 { RowDivider() }
                        row(s)
                    }
                }
            }
        }
    }

    private func row(_ s: ScheduleItem) -> some View {
        HStack(alignment: .top, spacing: Spacing.s3) {
            VStack(alignment: .leading, spacing: 2) {
                Text(s.name).font(HeronFont.body.weight(.semibold)).foregroundStyle(theme.ink)
                Text("\(s.trigger) · \(s.permissionsNote)").font(HeronFont.callout).foregroundStyle(theme.ink2)
                if let held = s.heldReason { Text(held).font(HeronFont.callout).foregroundStyle(theme.waiting).fixedSize(horizontal: false, vertical: true) }
                HStack(spacing: Spacing.s3) {
                    if let last = s.lastRun { Text("Last: \(Fmt.relative(last))") }
                    if let next = s.nextRun { Text("Next: \(Fmt.format("EEE HH:mm", next))") }
                    if s.tokens > 0 { Text("\(Fmt.tokens(s.tokens)) tokens").monospacedDigit() }
                }.font(HeronFont.sub).foregroundStyle(theme.ink3)
            }
            Spacer()
            if s.enabled {
                Button("Run now") { Task { await schedules.runNow(s) } }.buttonStyle(HButtonStyle(.secondary))
            } else {
                Button("Turn on") { Task { await schedules.setEnabled(s, true) } }.buttonStyle(HButtonStyle(.secondary))
            }
        }
        .padding(.horizontal, Spacing.s4).padding(.vertical, 10)
        .accessibilityElement(children: .contain)
    }
}

// MARK: Activity

struct ActivityView: View {
    @Bindable var model: AppModel
    @Environment(\.heronTheme) private var theme

    private var activity: ActivityStore { model.activity }

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Picker("Filter", selection: Bindable(activity).filter) {
                    ForEach(ActivityStore.Filter.allCases, id: \.self) { Text($0.rawValue).tag($0) }
                }.pickerStyle(.segmented).labelsHidden().frame(maxWidth: 260)
                Spacer()
                if activity.log.intact { Label("Log intact", systemImage: "checkmark.seal").font(HeronFont.sub).foregroundStyle(theme.ok) }
                else { Label("Log was changed", systemImage: "exclamationmark.triangle").font(HeronFont.sub).foregroundStyle(theme.danger) }
            }
            .padding(.horizontal, Spacing.s4).padding(.vertical, Spacing.s2)
            Rectangle().fill(theme.hairline).frame(height: 1)
            ScreenScaffold(loading: activity.loadState.isLoading, failure: activity.loadState.failureText, isEmpty: activity.log.entries.isEmpty,
                           empty: EmptyStateView(title: "No activity yet", text: "Every action Heron takes is listed here, with who allowed it.")) {
                RowGroup {
                    ForEach(Array(activity.entries.enumerated()), id: \.element.id) { i, e in
                        if i > 0 { RowDivider() }
                        HStack(alignment: .top, spacing: Spacing.s3) {
                            Text(Fmt.time(e.timestamp)).monospacedDigit().foregroundStyle(theme.ink3).frame(width: 48, alignment: .leading)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(ActivityStore.sentence(e)).foregroundStyle(theme.ink).lineLimit(2)
                                if !e.provider.isEmpty {
                                    Text("\(e.provider)\(e.cliVersion.isEmpty ? "" : " \(e.cliVersion)")").font(HeronFont.mono).foregroundStyle(theme.ink3)
                                }
                            }
                            Spacer()
                            HTag(ActivityStore.decisionLabel(e), tone: e.isRefused ? .danger : .neutral)
                        }
                        .padding(.horizontal, Spacing.s4).padding(.vertical, 10)
                        .accessibilityElement(children: .combine)
                    }
                }
            }
        }
    }
}
