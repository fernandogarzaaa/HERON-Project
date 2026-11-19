import XCTest
@testable import HeronCore

/// A clock the test moves by hand: each reading advances it by `step`.
final class StepClock: @unchecked Sendable {
    private let lock = NSLock()
    private var current = Date(timeIntervalSince1970: 1_800_000_000)
    private let step: TimeInterval
    init(step: TimeInterval) { self.step = step }
    func now() -> Date {
        lock.withLock {
            defer { current = current.addingTimeInterval(step) }
            return current
        }
    }
}

/// A fake core: a shell script that prints an endpoint line (port 9000 + how many times it has run) and then
/// exits at once, or waits for its stdin to close, as the real core does when its app goes away.
final class FakeCore {
    let directory: URL
    let script: URL

    enum Behaviour { case exitAtOnce, waitForStdin }

    init(_ behaviour: Behaviour) throws {
        directory = FileManager.default.temporaryDirectory.appendingPathComponent("heron-fake-core-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        script = directory.appendingPathComponent("heron-cli")
        let tail: String
        switch behaviour {
        case .exitAtOnce: tail = "exit 0"
        case .waitForStdin: tail = "cat > /dev/null\necho closed > \"$dir/stdin-closed\""
        }
        let body = """
        #!/bin/sh
        dir="\(directory.path)"
        n=$(cat "$dir/count" 2>/dev/null || echo 0)
        n=$((n+1))
        echo $n > "$dir/count"
        echo "$@" > "$dir/args"
        echo "{\\"port\\":$((9000+n)),\\"token\\":\\"token$n\\",\\"background_owner\\":true,\\"version\\":\\"0.0.0\\"}"
        \(tail)
        """
        try body.write(to: script, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: script.path)
    }

    deinit { try? FileManager.default.removeItem(at: directory) }

    var arguments: String? { try? String(contentsOf: directory.appendingPathComponent("args"), encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines) }
    var stdinClosed: Bool { FileManager.default.fileExists(atPath: directory.appendingPathComponent("stdin-closed").path) }
}

func isStopped(_ s: CoreProcess.Status?) -> Bool { if case .stopped = s { return true } else { return false } }
func isRestarted(_ s: CoreProcess.Status?) -> Bool { if case .restarted = s { return true } else { return false } }
func isRunning(_ s: CoreProcess.Status?) -> Bool { if case .running = s { return true } else { return false } }

/// Reads statuses until `done` says stop; fails after `timeout`. Take the stream (`process.statuses()`)
/// before `start()`, so no status is missed.
func collect(_ stream: AsyncStream<CoreProcess.Status>, until done: @escaping @Sendable ([CoreProcess.Status]) -> Bool,
             timeout: Duration = .seconds(20)) async throws -> [CoreProcess.Status] {
    try await withThrowingTaskGroup(of: [CoreProcess.Status].self) { group in
        group.addTask {
            var seen: [CoreProcess.Status] = []
            for await status in stream {
                seen.append(status)
                if done(seen) { break }
            }
            return seen
        }
        group.addTask {
            try await Task.sleep(for: timeout)
            throw CoreError.failed("timed out waiting for the core's status")
        }
        let first = try await group.next()!
        group.cancelAll()
        return first
    }
}

final class CoreProcessTests: XCTestCase {
    private func endpoint(_ n: Int) -> CoreEndpoint {
        CoreEndpoint(port: 9000 + n, token: "token\(n)", backgroundOwner: true, version: "0.0.0")
    }

    func testStartsTheCoreWithTheServeArgumentsAndReadsTheEndpointLine() async throws {
        let core = try FakeCore(.waitForStdin)
        let process = CoreProcess(launch: .init(executable: core.script))
        defer { process.stop(grace: 0.2) }
        let stream = process.statuses()
        process.start()
        let seen = try await collect(stream, until: { $0.last?.endpoint != nil })
        XCTAssertEqual(seen, [.starting, .running(endpoint(1))])
        XCTAssertEqual(core.arguments, "serve --no-open --port 0 --print-endpoint")
        XCTAssertEqual(CoreProcess.Launch.serveArguments, ["serve", "--no-open", "--port", "0", "--print-endpoint"])
    }

    func testAnExitIsRestartedOnceThenASecondExitWithinTheWindowStopsIt() async throws {
        let core = try FakeCore(.exitAtOnce)
        // A clock that never moves: both exits fall inside the 60 seconds.
        let process = CoreProcess(launch: .init(executable: core.script), restartWindow: 60, now: StepClock(step: 0).now)
        defer { process.stop(grace: 0.2) }
        let stream = process.statuses()
        process.start()
        let seen = try await collect(stream, until: { isStopped($0.last) })
        XCTAssertEqual(Array(seen.dropLast()), [.starting, .running(endpoint(1)), .restarted(endpoint(2))])
        guard case .stopped(let details) = seen.last else { return XCTFail("expected stopped, got \(String(describing: seen.last))") }
        XCTAssertTrue(details.contains("exited with status 0"), details)
        let count = try String(contentsOf: core.directory.appendingPathComponent("count"), encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines)
        XCTAssertEqual(count, "2", "the core was started exactly twice")
    }

    func testExitsMoreThanTheWindowApartKeepRestarting() async throws {
        let core = try FakeCore(.exitAtOnce)
        // Every clock reading is 61 seconds after the last, so no two exits are within 60 seconds.
        let process = CoreProcess(launch: .init(executable: core.script), restartWindow: 60, now: StepClock(step: 61).now)
        defer { process.stop(grace: 0.2) }
        let stream = process.statuses()
        process.start()
        let seen = try await collect(stream, until: { $0.filter { isRestarted($0) }.count >= 3 })
        XCTAssertFalse(seen.contains { isStopped($0) }, "\(seen)")
    }

    func testACoreThatCannotStartIsStopped() async throws {
        let missing = FileManager.default.temporaryDirectory.appendingPathComponent("heron-no-such-core-\(UUID().uuidString)")
        let process = CoreProcess(launch: .init(executable: missing), now: StepClock(step: 0).now)
        defer { process.stop(grace: 0.2) }
        let stream = process.statuses()
        process.start()
        let seen = try await collect(stream, until: { isStopped($0.last) })
        guard case .stopped(let details) = seen.last else { return XCTFail("expected stopped") }
        XCTAssertTrue(details.contains("couldn't start"), details)
    }

    func testQuittingClosesTheCoresStdin() async throws {
        let core = try FakeCore(.waitForStdin)
        let process = CoreProcess(launch: .init(executable: core.script))
        let stream = process.statuses()
        process.start()
        _ = try await collect(stream, until: { $0.last?.endpoint != nil })
        XCTAssertFalse(core.stdinClosed)
        process.stop(grace: 10) // returns as soon as the core has exited on its own
        XCTAssertTrue(core.stdinClosed, "the core saw its stdin close")
    }

    func testTheUserCanRestartAStoppedCore() async throws {
        let core = try FakeCore(.exitAtOnce)
        let process = CoreProcess(launch: .init(executable: core.script), now: StepClock(step: 0).now)
        defer { process.stop(grace: 0.2) }
        let first = process.statuses()
        process.start()
        _ = try await collect(first, until: { isStopped($0.last) })

        let after = process.statuses()
        process.restart()
        let seen = try await collect(after, until: { $0.contains { isRunning($0) } })
        XCTAssertTrue(seen.contains(.starting), "\(seen)")
        XCTAssertTrue(seen.contains { isRunning($0) }, "\(seen)")
    }

    func testDevelopmentBuildsCanPointAtAnotherBinary() {
        let launch = CoreProcess.Launch.bundled(environment: ["HERON_CLI_PATH": "/opt/heron/heron-cli"])
        XCTAssertEqual(launch.executable.path, "/opt/heron/heron-cli")
        XCTAssertEqual(launch.arguments, CoreProcess.Launch.serveArguments)
        let bundled = CoreProcess.Launch.bundled(environment: [:])
        XCTAssertEqual(bundled.executable, Bundle.main.bundleURL.appendingPathComponent("Contents/MacOS/heron-cli"))
    }
}
