import Foundation

/// Runs the bundled core: `Contents/MacOS/heron-cli serve --no-open --port 0 --print-endpoint` (heron-v1.md item 40).
///
/// * The core prints one line, `{"port","token","background_owner","version"}`; that is the `CoreEndpoint`.
/// * If the core exits it is started once more (the port and token change, so `Status.restarted` carries the new endpoint).
/// * A second exit within `restartWindow` (60 seconds) stops it for good (`Status.stopped`); the app shows the
///   "Heron's core stopped" card and `restart()` is the user's button.
/// * On quit, `stop()` closes the core's stdin, which is how the core knows its app is gone, and waits briefly
///   before terminating it.
///
/// The executable, its arguments and the clock are injected, so tests run a fake core script and move time
/// without sleeping.
public final class CoreProcess: @unchecked Sendable {
    public struct Launch: Sendable, Equatable {
        public var executable: URL
        public var arguments: [String]
        /// Added to the app's environment for the core.
        public var environment: [String: String]

        public static let serveArguments = ["serve", "--no-open", "--port", "0", "--print-endpoint"]

        public init(executable: URL, arguments: [String] = Launch.serveArguments, environment: [String: String] = [:]) {
            self.executable = executable
            self.arguments = arguments
            self.environment = environment
        }

        /// The core inside the app bundle. `HERON_CLI_PATH` points a development build (`swift run`) at another binary.
        public static func bundled(bundle: Bundle = .main, environment: [String: String] = ProcessInfo.processInfo.environment) -> Launch {
            if let path = environment["HERON_CLI_PATH"], !path.isEmpty {
                return Launch(executable: URL(fileURLWithPath: path))
            }
            return Launch(executable: bundle.bundleURL.appendingPathComponent("Contents/MacOS/heron-cli"))
        }
    }

    public enum Status: Sendable, Equatable {
        case starting
        case running(CoreEndpoint)
        /// The core exited once and is running again, at a new endpoint.
        case restarted(CoreEndpoint)
        /// Two exits within the window (or the core can't start). It stays down until `restart()`.
        case stopped(details: String)

        public var endpoint: CoreEndpoint? {
            switch self {
            case .running(let e), .restarted(let e): e
            case .starting, .stopped: nil
            }
        }
    }

    private struct Running {
        var process: Process
        var stdin: FileHandle?
        var generation: Int
        var isRestart: Bool
        var gotEndpoint = false
        var outputClosed = false
        var exit: (status: Int32, reason: Process.TerminationReason)?
    }

    private let launch: Launch
    private let restartWindow: TimeInterval
    private let endpointTimeout: TimeInterval
    private let now: @Sendable () -> Date

    private let lock = NSLock()
    private var current: Running?
    private var generation = 0
    private var exits: [Date] = []
    private var stopping = false
    private var tail: [String] = []
    private var state: Status = .starting
    private var observers: [UUID: AsyncStream<Status>.Continuation] = [:]

    public init(launch: Launch, restartWindow: TimeInterval = 60, endpointTimeout: TimeInterval = 20,
                now: @escaping @Sendable () -> Date = Date.init) {
        self.launch = launch
        self.restartWindow = restartWindow
        self.endpointTimeout = endpointTimeout
        self.now = now
    }

    public var status: Status { lock.withLock { state } }

    /// The current status first, then every change.
    public func statuses() -> AsyncStream<Status> {
        let (stream, continuation) = AsyncStream<Status>.makeStream(bufferingPolicy: .unbounded)
        let id = UUID()
        lock.withLock {
            continuation.yield(state)
            observers[id] = continuation
        }
        continuation.onTermination = { [weak self] _ in
            self?.lock.withLock { _ = self?.observers.removeValue(forKey: id) }
        }
        return stream
    }

    // MARK: Control

    /// Starts the core. Does nothing when it is already running.
    public func start() {
        lock.withLock {
            guard current == nil, !stopping else { return }
            if case .stopped = state { return }
            spawn(isRestart: false)
        }
    }

    /// The user's "Restart Heron's core": forgets the exits and starts again, also after `stopped`.
    public func restart() {
        lock.withLock {
            guard !stopping else { return }
            exits.removeAll()
            tail.removeAll()
            if let old = current {
                generation += 1 // its exit is no longer news
                old.stdin?.closeFile()
                old.process.terminate()
                current = nil
            }
            publish(.starting)
            spawn(isRestart: false)
        }
    }

    /// The app is quitting: close the core's stdin so it exits by itself, give it `grace` seconds, then terminate it.
    public func stop(grace: TimeInterval = 2) {
        let (running, finished): (Running?, [AsyncStream<Status>.Continuation]) = lock.withLock {
            stopping = true
            generation += 1
            let r = current
            current = nil
            let all = Array(observers.values)
            observers.removeAll()
            return (r, all)
        }
        // Outside the lock: finishing a stream runs its `onTermination`, which takes the lock.
        finished.forEach { $0.finish() }
        guard let running else { return }
        try? running.stdin?.close()
        let deadline = Date().addingTimeInterval(grace)
        while running.process.isRunning && Date() < deadline { usleep(20_000) }
        if running.process.isRunning { running.process.terminate() }
    }

    // MARK: Launch (call with the lock held)

    private func spawn(isRestart: Bool) {
        generation += 1
        let gen = generation
        let process = Process()
        process.executableURL = launch.executable
        process.arguments = launch.arguments
        process.environment = ProcessInfo.processInfo.environment.merging(launch.environment) { _, new in new }
        let stdin = Pipe(), stdout = Pipe(), stderr = Pipe()
        process.standardInput = stdin
        process.standardOutput = stdout
        process.standardError = stderr
        process.terminationHandler = { [weak self] p in
            self?.exited(generation: gen, status: p.terminationStatus, reason: p.terminationReason)
        }
        do {
            try process.run()
        } catch {
            current = nil
            recordExit(details: "Heron's core couldn't start (\(launch.executable.path)): \(error.localizedDescription)", isRestart: isRestart)
            return
        }
        current = Running(process: process, stdin: stdin.fileHandleForWriting, generation: gen, isRestart: isRestart)
        // Both pipes are read until the core closes them. After the endpoint line the core goes on writing logs,
        // and a pipe nobody reads fills up and blocks the core.
        read(stdout.fileHandleForReading, generation: gen, isOutput: true)
        read(stderr.fileHandleForReading, generation: gen, isOutput: false)
        let timeout = endpointTimeout
        DispatchQueue.global().asyncAfter(deadline: .now() + timeout) { [weak self] in
            self?.endpointTimedOut(generation: gen)
        }
    }

    private func read(_ handle: FileHandle, generation gen: Int, isOutput: Bool) {
        Thread.detachNewThread { [weak self] in
            var buffer = Data()
            while true {
                let chunk = handle.availableData
                if chunk.isEmpty { break }
                buffer.append(chunk)
                while let newline = buffer.firstIndex(of: 0x0A) {
                    let line = String(decoding: buffer[buffer.startIndex..<newline], as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
                    buffer.removeSubrange(buffer.startIndex...newline)
                    self?.handle(line: line, generation: gen, isOutput: isOutput)
                }
            }
            if isOutput { self?.outputClosed(generation: gen) }
        }
    }

    private func handle(line: String, generation gen: Int, isOutput: Bool) {
        guard !line.isEmpty else { return }
        lock.withLock {
            tail.append(line)
            if tail.count > 20 { tail.removeFirst(tail.count - 20) }
            guard isOutput, gen == generation, var running = current, !running.gotEndpoint,
                  let endpoint = try? CoreWire.endpoint(line: line) else { return }
            running.gotEndpoint = true
            current = running
            publish(running.isRestart ? .restarted(endpoint) : .running(endpoint))
        }
    }

    private func endpointTimedOut(generation gen: Int) {
        let process: Process? = lock.withLock {
            guard gen == generation, let running = current, !running.gotEndpoint else { return nil }
            tail.append("No endpoint line within \(Int(endpointTimeout)) seconds.")
            return running.process
        }
        process?.terminate()
    }

    /// The process ended. Its last words may still be in the pipe (the endpoint line of a core that exits at
    /// once), so the exit is handled when stdout has been read to its end, or after a second if something else
    /// holds the pipe open.
    private func exited(generation gen: Int, status: Int32, reason: Process.TerminationReason) {
        lock.withLock {
            guard gen == generation, !stopping, var running = current else { return }
            running.exit = (status, reason)
            current = running
        }
        finishExitIfReady(generation: gen)
        DispatchQueue.global().asyncAfter(deadline: .now() + 1) { [weak self] in
            self?.finishExit(generation: gen)
        }
    }

    private func outputClosed(generation gen: Int) {
        lock.withLock {
            guard gen == generation, var running = current else { return }
            running.outputClosed = true
            current = running
        }
        finishExitIfReady(generation: gen)
    }

    private func finishExitIfReady(generation gen: Int) {
        let ready = lock.withLock { () -> Bool in
            guard gen == generation, let running = current else { return false }
            return running.exit != nil && running.outputClosed
        }
        if ready { finishExit(generation: gen) }
    }

    private func finishExit(generation gen: Int) {
        lock.withLock {
            guard gen == generation, !stopping, let running = current, let exit = running.exit else { return }
            current = nil
            generation += 1 // this run is over; a late callback for it is ignored
            running.stdin?.closeFile()
            let how = exit.reason == .uncaughtSignal ? "was stopped by signal \(exit.status)" : "exited with status \(exit.status)"
            recordExit(details: "Heron's core \(how).", isRestart: running.isRestart)
        }
    }

    /// An exit, or a start that failed. The first within the window restarts the core, the second stops it.
    private func recordExit(details: String, isRestart: Bool) {
        let t = now()
        exits = exits.filter { t.timeIntervalSince($0) < restartWindow } + [t]
        if exits.count >= 2 {
            let log = tail.suffix(5).joined(separator: "\n")
            publish(.stopped(details: log.isEmpty ? details : details + "\n" + log))
        } else {
            spawn(isRestart: true)
        }
    }

    private func publish(_ status: Status) {
        state = status
        observers.values.forEach { $0.yield(status) }
    }
}
