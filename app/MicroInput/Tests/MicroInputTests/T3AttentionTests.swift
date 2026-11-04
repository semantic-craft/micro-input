import XCTest
import MicroInputCore
@testable import MicroInput

private let t3Time = "2026-11-04T10:00:00.000Z"
private let t3Earlier = "2026-11-04T09:00:00.000Z"
private func t3Thread(_ id: String = "thread-a", changes: [String: Any] = [:]) -> [String: Any] {
    var value: [String: Any] = ["id": id, "title": "Synthetic title", "status": "idle", "updatedAt": t3Time,
        "interactionMode": "default", "lineage": ["relationshipToParent": NSNull(), "parentThreadId": NSNull()],
        "latestRunId": "run-a", "latestRunCompletedAt": NSNull(), "activeRunId": NSNull(), "activityRunStatus": NSNull(),
        "pendingRuntimeRequest": NSNull(), "hasActionableProposedPlan": false, "archivedAt": NSNull(),
        "deletedAt": NSNull(), "lastVisitedAt": NSNull(), "pendingBackgroundTasks": []]
    for (key, change) in changes { value[key] = change }
    return value
}
private func t3Request(_ id: String = "request-a", kind: String = "user_input", blocking: Bool = true) -> [String: Any] {
    ["id": id, "kind": kind, "createdAt": t3Time, "blocking": blocking]
}
private func t3Data(_ threads: [[String: Any]], sequence: Int = 1) throws -> Data {
    try JSONSerialization.data(withJSONObject: ["schemaVersion": 2, "snapshotSequence": sequence, "threads": threads])
}
private func t3Snapshot(_ threads: [[String: Any]], sequence: Int = 1) throws -> T3ReadSnapshot {
    T3ReadSnapshot(environmentID: "environment-a", shell: try T3Shell.decode(t3Data(threads, sequence: sequence)))
}
private func t3Items(_ threads: [[String: Any]]) throws -> [T3AttentionItem] {
    var observed: [T3AttentionEvent: Date] = [:]
    return try t3Snapshot(threads).shell.attention(environmentID: "environment-a", now: Date(), observed: &observed)
}

final class T3AttentionModelTests: XCTestCase {
    func testPendingAndNativeNonblockingQuestionSemantics() throws {
        XCTAssertEqual(try t3Items([t3Thread(changes: ["pendingRuntimeRequest": t3Request()])]).first?.event.reason, .request)
        XCTAssertTrue(try t3Items([t3Thread(changes: ["status": "running", "pendingRuntimeRequest": t3Request(blocking: false)])]).isEmpty)
        XCTAssertEqual(try t3Items([t3Thread(changes: ["status": "waiting", "pendingRuntimeRequest": t3Request(blocking: false)])]).count, 1)
        XCTAssertTrue(try t3Items([t3Thread(changes: ["pendingRuntimeRequest": t3Request(kind: "auth_refresh")])]).isEmpty)
        XCTAssertEqual(try t3Items([t3Thread(changes: ["status": "running", "pendingRuntimeRequest": t3Request(kind: "command")])]).count, 1)
    }
    func testUnreadFailureAndCompletionUseNativeVisits() throws {
        let completed = ["status": "completed", "latestRunCompletedAt": t3Time]
        XCTAssertTrue(try t3Items([t3Thread(changes: completed)]).isEmpty)
        XCTAssertEqual(try t3Items([t3Thread(changes: completed.merging(["lastVisitedAt": t3Earlier]) { _, b in b })]).first?.event.reason, .completion)
        XCTAssertTrue(try t3Items([t3Thread(changes: completed.merging(["lastVisitedAt": t3Time]) { _, b in b })]).isEmpty)
        XCTAssertEqual(try t3Items([t3Thread(changes: ["status": "failed"])]).first?.event.reason, .failure)
        var old = t3Thread(); old.removeValue(forKey: "lastVisitedAt")
        XCTAssertThrowsError(try t3Items([old]))
    }
    func testActiveRunAndAwaitedBackgroundSuppressOldCompletion() throws {
        let completed: [String: Any] = ["status": "completed", "latestRunCompletedAt": t3Time, "lastVisitedAt": t3Earlier]
        for active in ["queued", "preparing", "starting", "running", "waiting"] {
            XCTAssertTrue(try t3Items([t3Thread(changes: completed.merging(["status": active]) { _, b in b })]).isEmpty)
        }
        XCTAssertTrue(try t3Items([t3Thread(changes: completed.merging(["pendingBackgroundTasks": [["kind": "command", "awaited": true]]]) { _, b in b })]).isEmpty)
        XCTAssertEqual(try t3Items([t3Thread(changes: completed.merging(["pendingBackgroundTasks": [["kind": "command"]]]) { _, b in b })]).count, 1)
    }
    func testSubagentApprovalRollsUpButQuestionAndNativeChildDoNot() throws {
        let parent = t3Thread("parent")
        let child = t3Thread("child", changes: ["lineage": ["relationshipToParent": "subagent", "parentThreadId": "parent"],
            "activityRunStatus": "waiting", "pendingRuntimeRequest": t3Request(kind: "command"), "creationSource": "user"])
        let items = try t3Items([parent, child])
        XCTAssertEqual(items.map(\.event.identity.threadID), ["parent"])
        XCTAssertTrue(try XCTUnwrap(items.first).event.eventID.contains("child"))
        XCTAssertTrue(try t3Items([parent, child.merging(["pendingRuntimeRequest": t3Request()]) { _, b in b }]).isEmpty)
        XCTAssertTrue(try t3Items([parent, child.merging(["creationSource": "provider"]) { _, b in b }]).isEmpty)
        XCTAssertTrue(try t3Items([parent, child.merging(["activityRunStatus": NSNull()]) { _, b in b }]).isEmpty)
    }
    func testSideChatQuestionRollsUpAndParentCyclesTerminate() throws {
        let side = t3Thread("side", changes: ["lineage": ["sideChat": true, "parentThreadId": "parent"], "pendingRuntimeRequest": t3Request()])
        XCTAssertEqual(try t3Items([t3Thread("parent"), side]).map(\.event.identity.threadID), ["parent"])
        let cyclic = t3Thread("cycle", changes: ["lineage": ["relationshipToParent": "subagent", "parentThreadId": "cycle"], "activityRunStatus": "waiting", "pendingRuntimeRequest": t3Request(kind: "command")])
        XCTAssertTrue(try t3Items([cyclic]).isEmpty)
    }
    func testHiddenRowsAndPlanReady() throws {
        let pending: [String: Any] = ["pendingRuntimeRequest": t3Request()]
        for hidden: [String: Any] in [["archivedAt": t3Time], ["deletedAt": t3Time]] {
            XCTAssertTrue(try t3Items([t3Thread(changes: pending.merging(hidden) { _, b in b })]).isEmpty)
        }
        XCTAssertEqual(try t3Items([t3Thread(changes: ["interactionMode": "plan", "status": "completed", "hasActionableProposedPlan": true])]).first?.event.reason, .plan)
        XCTAssertEqual(try t3Items([t3Thread(changes: pending.merging(["lineage": ["relationshipToParent": "fork"]]) { _, b in b })]).count, 1)
    }
    func testRoundAdvancesOnlyAfterDeliveryAndNewRequestReenters() throws {
        let items = try t3Items([t3Thread("a", changes: ["pendingRuntimeRequest": t3Request("r1")]), t3Thread("b", changes: ["pendingRuntimeRequest": t3Request("r2")])])
        var round = T3AttentionRound()
        XCTAssertEqual(round.next(items), items[0].event)
        XCTAssertEqual(round.next(items), items[0].event)
        round.record(items[0].event, items: items)
        XCTAssertEqual(round.next(items), items[1].event)
        round.record(items[1].event, items: items)
        XCTAssertEqual(round.next(items), items[0].event)
        let replacement = try t3Items([t3Thread("a", changes: ["pendingRuntimeRequest": t3Request("r3")]), t3Thread("b", changes: ["pendingRuntimeRequest": t3Request("r2")])])
        XCTAssertEqual(round.next(replacement), replacement[0].event)
    }
    func testSnoozeWakesForRequestsGuestAndFreshFailureButNotCompletion() throws {
        let snooze: [String: Any] = ["snoozedUntil": "2099-11-04T10:00:00Z", "snoozedAt": t3Earlier]
        for wake: [String: Any] in [["pendingRuntimeRequest": t3Request()], ["pendingGuestMessageCount": 1], ["status": "failed", "latestRunCompletedAt": t3Time]] {
            XCTAssertEqual(try t3Items([t3Thread(changes: snooze.merging(wake) { _, b in b })]).count, 1)
        }
        XCTAssertTrue(try t3Items([t3Thread(changes: snooze.merging(["status": "completed", "latestRunCompletedAt": t3Time, "lastVisitedAt": t3Earlier]) { _, b in b })]).isEmpty)
        XCTAssertTrue(try t3Items([t3Thread(changes: snooze.merging(["status": "failed", "latestRunCompletedAt": t3Earlier]) { _, b in b })]).isEmpty)
    }
    func testCurrentRequestPrecedesOlderUnreadCompletion() throws {
        let items = try t3Items([t3Thread("old", changes: ["status": "completed", "latestRunCompletedAt": t3Earlier, "lastVisitedAt": "2026-11-03T09:00:00Z"]),
                                t3Thread("current", changes: ["pendingRuntimeRequest": t3Request()])])
        XCTAssertEqual(items.map(\.event.identity.threadID), ["current", "old"])
    }
    func testDuplicateIdentitySchemaAndDeepLinkBoundary() throws {
        XCTAssertThrowsError(try t3Items([t3Thread(), t3Thread()]))
        XCTAssertThrowsError(try T3Shell.decode(Data(#"{"schemaVersion":3,"snapshotSequence":1,"threads":[]}"#.utf8)))
        let url = try XCTUnwrap(T3ThreadIdentity(environmentID: "env/x", threadID: "t?prompt=x#fragment").deepLink)
        XCTAssertNil(URLComponents(url: url, resolvingAgainstBaseURL: false)?.query)
        XCTAssertNil(URLComponents(url: url, resolvingAgainstBaseURL: false)?.fragment)
        XCTAssertTrue(url.absoluteString.contains("env%2Fx"))
    }
}

@MainActor private final class T3TestReader: T3Reading {
    var value: T3ReadSnapshot
    var failure: T3Failure?
    var hold = false
    var waiter: CheckedContinuation<Void, Never>?
    var closes = 0, connections = 0
    var failures: [T3Failure] = []
    init(_ value: T3ReadSnapshot) { self.value = value }
    func connect() async throws -> T3ReadSnapshot { connections += 1; return value }
    func read() async throws -> T3ReadSnapshot {
        if hold { await withCheckedContinuation { waiter = $0 } }
        if !failures.isEmpty { throw failures.removeFirst() }
        if let failure { throw failure }
        return value
    }
    func close() async -> Bool { closes += 1; return true }
}
@MainActor final class T3AttentionControllerTests: XCTestCase {
    private func ready(_ controller: T3AttentionController) async {
        for _ in 0..<1000 {
            if controller.state == .ready { return }
            try? await Task.sleep(nanoseconds: 1_000_000)
        }
        XCTFail("Connection did not become ready")
    }
    func testFrozenTargetDisappearsWithoutSelectingAnother() async throws {
        let client = T3TestReader(try t3Snapshot([t3Thread("a", changes: ["pendingRuntimeRequest": t3Request()]), t3Thread("b", changes: ["pendingRuntimeRequest": t3Request()])]))
        var opens = 0
        let controller = T3AttentionController(client: client, pollNanoseconds: 60_000_000_000, opener: { _ in opens += 1; return true })
        XCTAssertFalse(controller.enabled)
        XCTAssertNil(controller.selectNext())
        controller.setEnabled(true); await ready(controller)
        let selected = try XCTUnwrap(controller.selectNext())
        client.value = try t3Snapshot([t3Thread("b", changes: ["pendingRuntimeRequest": t3Request()])], sequence: 2)
        let result = await controller.open(selected, admitted: { true })
        XCTAssertEqual(result, .stale); XCTAssertEqual(opens, 0)
        await controller.shutdown()
    }
    func testFailureDoesNotAdvanceAndDeliveryNeverClaimsConfirmation() async throws {
        let client = T3TestReader(try t3Snapshot([t3Thread("a", changes: ["pendingRuntimeRequest": t3Request()]), t3Thread("b", changes: ["pendingRuntimeRequest": t3Request()])]))
        var allow = false
        let controller = T3AttentionController(client: client, pollNanoseconds: 60_000_000_000, opener: { _ in allow })
        controller.setEnabled(true); await ready(controller)
        let selected = controller.selectNext()
        let failed = await controller.open(selected, admitted: { true })
        XCTAssertEqual(failed, .failed); XCTAssertEqual(controller.selectNext(), selected)
        allow = true
        let delivered = await controller.open(selected, admitted: { true })
        XCTAssertEqual(delivered, .delivered); XCTAssertEqual(controller.selectNext()?.event.identity.threadID, "b")
        await controller.shutdown()
    }
    func testAdmissionAndDisconnectDuringRefreshPreventOpen() async throws {
        let client = T3TestReader(try t3Snapshot([t3Thread(changes: ["pendingRuntimeRequest": t3Request()])]))
        var opens = 0, admission = true
        let controller = T3AttentionController(client: client, pollNanoseconds: 60_000_000_000, opener: { _ in opens += 1; return true })
        controller.setEnabled(true); await ready(controller)
        client.hold = true
        let task = Task { await controller.open(controller.selectNext(), admitted: { admission }) }
        while client.waiter == nil { await Task.yield() }
        admission = false; client.waiter?.resume(); client.waiter = nil
        let staleResult = await task.value
        XCTAssertEqual(staleResult, .stale)
        XCTAssertEqual(opens, 0)
        client.hold = false; client.failure = .transport
        let result = await controller.open(controller.selectNext(), admitted: { true })
        XCTAssertEqual(result, .failed); XCTAssertTrue(controller.items.isEmpty); XCTAssertNil(controller.selectNext())
        await controller.shutdown()
    }
}

private func t3Connection() -> T3ConnectionInfo {
    T3ConnectionInfo(environmentID: "environment-a", origin: URL(string: "http://127.0.0.1:43210")!,
                     executable: URL(fileURLWithPath: "/bin/sh"), home: FileManager.default.temporaryDirectory)
}
private actor T3ClientProbe {
    var issueCount = 0, revokeCount = 0, httpCount = 0
    var holdIssue = false, holdHTTP = false, failRevoke = false
    var issueWaiter: CheckedContinuation<Void, Never>?, httpWaiter: CheckedContinuation<Void, Never>?
    var httpFailure: T3Failure?
    var headerCorrect = false
    var rows: [[String: Any]] = []
    func setRows(_ value: [[String: Any]]) { rows = value }
    func configure(holdIssue: Bool = false, holdHTTP: Bool = false, failRevoke: Bool = false, httpFailure: T3Failure? = nil) {
        self.holdIssue = holdIssue; self.holdHTTP = holdHTTP; self.failRevoke = failRevoke; self.httpFailure = httpFailure
    }
    func process(_ info: T3ConnectionInfo, _ arguments: [String]) async throws -> T3ProcessOutput {
        if arguments.contains("issue") {
            issueCount += 1
            if holdIssue { await withCheckedContinuation { issueWaiter = $0 } }
            let data = try JSONSerialization.data(withJSONObject: ["sessionId": "synthetic-session", "token": "synthetic-read-token",
                "scopes": ["orchestration:read"], "expiresAt": "2099-11-04T10:00:00Z"])
            return T3ProcessOutput(status: 0, stdout: data)
        }
        revokeCount += 1
        return T3ProcessOutput(status: failRevoke ? 1 : 0, stdout: Data())
    }
    func http(_ request: URLRequest) async throws -> Data {
        httpCount += 1
        headerCorrect = request.value(forHTTPHeaderField: "Authorization") == "Bearer synthetic-read-token" &&
            request.value(forHTTPHeaderField: "x-t3-orchestration-protocol") == "2" && request.httpMethod == "GET" && request.httpBody == nil
        if holdHTTP { await withCheckedContinuation { httpWaiter = $0 } }
        if let httpFailure { throw httpFailure }
        return try t3Data(rows)
    }
    func releaseIssue() { issueWaiter?.resume(); issueWaiter = nil }
    func releaseHTTP() { holdHTTP = false; httpWaiter?.resume(); httpWaiter = nil }
    func counts() -> [Int] { [issueCount, revokeCount, httpCount] }
    func waitingIssue() -> Bool { issueWaiter != nil }
    func waitingHTTP() -> Bool { httpWaiter != nil }
}
final class T3AttentionClientTests: XCTestCase {
    func testStrictOriginRejectsRedirectTargetsAndAmbiguousHosts() throws {
        for origin in ["https://127.0.0.1:80", "http://localhost:80", "http://127.0.0.1.evil:80", "http://127.0.0.1:80/path",
                       "http://user:secret@127.0.0.1:80", "http://127.0.0.1:80?target=external", "http://10.0.0.1:80"] {
            XCTAssertThrowsError(try T3ConnectionInfo.validateOrigin(origin))
        }
        XCTAssertNoThrow(try T3ConnectionInfo.validateOrigin("http://127.0.0.1:1234"))
        XCTAssertNoThrow(try T3ConnectionInfo.validateOrigin("http://[::1]:1234"))
    }
    func testReadOnlyHeadersAndRevokeAreOwnedByClient() async throws {
        let probe = T3ClientProbe()
        let client = T3AttentionClient(discovery: t3Connection, process: probe.process, http: probe.http)
        _ = try await client.connect()
        let headers = await probe.headerCorrect
        XCTAssertTrue(headers)
        let closed = await client.close(), counts = await probe.counts()
        XCTAssertTrue(closed); XCTAssertEqual(counts, [1, 1, 1])
        do { _ = try await client.read(); XCTFail("Closed connection read") } catch { XCTAssertEqual(error as? T3Failure, .transport) }
    }
    func testCancelledLateIssuanceStillRevokes() async throws {
        let probe = T3ClientProbe(); await probe.configure(holdIssue: true)
        let client = T3AttentionClient(discovery: t3Connection, process: probe.process, http: probe.http)
        let task = Task { try await client.connect() }
        while !(await probe.waitingIssue()) { await Task.yield() }
        task.cancel()
        _ = await client.close()
        await probe.releaseIssue()
        do { _ = try await task.value; XCTFail("Cancelled issue became usable") } catch { }
        let counts = await probe.counts()
        XCTAssertEqual(counts, [1, 1, 0])
    }
    func testFailureRevokesAndNeverExportsRawResponse() async throws {
        let probe = T3ClientProbe(); await probe.configure(httpFailure: .authDenied)
        let client = T3AttentionClient(discovery: t3Connection, process: probe.process, http: probe.http)
        do { _ = try await client.connect(); XCTFail("Failed HTTP became ready") }
        catch { XCTAssertEqual(error as? T3Failure, .authDenied); XCTAssertFalse(String(describing: error).contains("synthetic-read-token")) }
        let counts = await probe.counts()
        XCTAssertEqual(counts, [1, 1, 1])
    }
    func testSimultaneousReadsShareOneRequestAndCloseInvalidatesIt() async throws {
        let probe = T3ClientProbe()
        let client = T3AttentionClient(discovery: t3Connection, process: probe.process, http: probe.http)
        _ = try await client.connect()
        await probe.configure(holdHTTP: true)
        let first = Task { try await client.read() }
        while !(await probe.waitingHTTP()) { await Task.yield() }
        let second = Task { try await client.read() }
        for _ in 0..<10 { await Task.yield() }
        let counts = await probe.counts()
        XCTAssertEqual(counts[2], 2)
        _ = await client.close(); await probe.releaseHTTP()
        for task in [first, second] {
            do { _ = try await task.value; XCTFail("Read survived close") } catch { }
        }
    }
    func testRevokeFailureIsReported() async throws {
        let probe = T3ClientProbe(); await probe.configure(failRevoke: true)
        let client = T3AttentionClient(discovery: t3Connection, process: probe.process, http: probe.http)
        _ = try await client.connect()
        let closed = await client.close()
        XCTAssertFalse(closed)
    }
    func testPrivateProcessDrainsBothPipesAndBoundsOutput() async throws {
        let result = try await T3PrivateProcess.run(t3Connection(), ["-c", "i=0; while [ $i -lt 1000 ]; do printf x; printf y >&2; i=$((i+1)); done"])
        XCTAssertEqual(result.status, 0); XCTAssertEqual(result.stdout.count, 1000)
        do {
            _ = try await T3PrivateProcess.run(t3Connection(), ["-c", "while :; do printf 'synthetic-overflow-output'; done"])
            XCTFail("Unbounded stdout")
        } catch { XCTAssertEqual(error as? T3Failure, .oversized) }
    }
    func testOpenCompletionIgnoresLateCallback() async {
        let result: Bool = await withCheckedContinuation { continuation in
            let completion = T3OpenCompletion(continuation)
            completion.finish(false); completion.finish(true)
        }
        XCTAssertFalse(result)
    }
}

@MainActor private final class T3RacingReader: T3Reading {
    let initial: T3ReadSnapshot, older: T3ReadSnapshot, newer: T3ReadSnapshot
    var waiter: CheckedContinuation<Void, Never>?
    var reads = 0, closes = 0
    init() throws {
        let rows = [t3Thread(changes: ["pendingRuntimeRequest": t3Request()])]
        initial = try t3Snapshot(rows, sequence: 1); older = try t3Snapshot(rows, sequence: 2); newer = try t3Snapshot(rows, sequence: 3)
    }
    func connect() async throws -> T3ReadSnapshot { initial }
    func read() async throws -> T3ReadSnapshot {
        reads += 1
        if reads == 1 { await withCheckedContinuation { waiter = $0 }; return older }
        return newer
    }
    func close() async -> Bool { closes += 1; return true }
}
extension T3AttentionControllerTests {
    func testOlderPollFinishingAfterOpenDoesNotRetireHealthyConnection() async throws {
        let reader = try T3RacingReader()
        let controller = T3AttentionController(client: reader, pollNanoseconds: 1_000_000, opener: { _ in true })
        controller.setEnabled(true); await ready(controller)
        while reader.waiter == nil { await Task.yield() }
        let closesBefore = reader.closes
        let result = await controller.open(controller.selectNext(), admitted: { true })
        XCTAssertEqual(result, .delivered)
        reader.waiter?.resume(); reader.waiter = nil
        try await Task.sleep(nanoseconds: 10_000_000)
        XCTAssertEqual(controller.state, .ready); XCTAssertEqual(controller.items.count, 1); XCTAssertEqual(reader.closes, closesBefore)
        await controller.shutdown()
    }
    func testRealBridgeMenuAndBoundActionUseSameLeaseAndPrivateReceipt() async throws {
        let client = T3TestReader(try t3Snapshot([t3Thread(changes: ["pendingRuntimeRequest": t3Request()])]))
        var opens = 0
        let controller = T3AttentionController(client: client, pollNanoseconds: 60_000_000_000, opener: { _ in opens += 1; return true })
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let core = MicroInputCore.MicroInput()
        let store = Store(backend: core, documentURL: directory.appendingPathComponent("workbench.json"), t3Attention: controller)
        await store.loadWorkbench(); await store.installCodingAgentPresets()
        let profile = try XCTUnwrap(store.profile?.id.text)
        await store.setHostActiveProfile(profile)
        await store.assign("t3-next-attention")
        await store.setHostExecutionEnabled(true)
        controller.setEnabled(true); await ready(controller)
        let (json, resolvedProfile, state) = try XCTUnwrap(store.resolveHostAction("AG00"))
        XCTAssertTrue(json.contains("next-attention"))
        store.enqueueHostAction(json: json, binding: "AG00", profile: resolvedProfile, state: state)
        await store.hostExecutionTask?.value
        XCTAssertEqual(opens, 1); XCTAssertEqual(core.hostExecutionsPending(), 0)
        store.openNextT3Attention(); await store.hostExecutionTask?.value
        XCTAssertEqual(opens, 2); XCTAssertEqual(store.lastHostActionReceipt?.outcome, "delivered")
        let receipt = try JSONEncoder().encode(XCTUnwrap(store.lastHostActionReceipt))
        let text = String(decoding: receipt, as: UTF8.self)
        for privateValue in ["thread-a", "environment-a", "request-a", "Synthetic title", "t3code://"] { XCTAssertFalse(text.contains(privateValue)) }
        client.hold = true
        store.openNextT3Attention()
        while client.waiter == nil { await Task.yield() }
        XCTAssertEqual(core.hostExecutionsPending(), 1)
        let execution = try XCTUnwrap(store.hostExecutionTask)
        store.systemWillSleep()
        client.waiter?.resume(); client.waiter = nil
        await execution.value
        XCTAssertEqual(opens, 2); XCTAssertEqual(core.hostExecutionsPending(), 0)
        await store.shutdown()
    }
}

extension T3AttentionClientTests {
    func testRealHTTPRefusesRedirectWithoutForwardingHeader() async throws {
        let server = Process(), output = Pipe()
        server.executableURL = URL(fileURLWithPath: "/usr/bin/python3")
        server.environment = ["PATH": "/usr/bin:/bin"]
        server.standardOutput = output; server.standardError = FileHandle.nullDevice
        server.arguments = ["-u", "-c", """
        import socket,select
        listener=socket.socket()
        listener.bind(('127.0.0.1',0)); listener.listen(2)
        print(listener.getsockname()[1],flush=True)
        connection,_=listener.accept(); connection.recv(8192)
        reply='HTTP/1.1 302 Found\\r\\nLocation: http://127.0.0.1:%s/forwarded\\r\\nContent-Length: 0\\r\\nConnection: close\\r\\n\\r\\n' % listener.getsockname()[1]
        connection.sendall(reply.encode()); connection.close()
        print('forwarded' if select.select([listener],[],[],1)[0] else 'not-forwarded',flush=True)
        listener.close()
        """]
        try server.run()
        defer { if server.isRunning { server.terminate() }; try? output.fileHandleForReading.close() }
        let port = try XCTUnwrap(Int(String(decoding: output.fileHandleForReading.availableData, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)))
        var request = URLRequest(url: URL(string: "http://127.0.0.1:\(port)/shell")!)
        request.setValue("Bearer synthetic-only", forHTTPHeaderField: "Authorization")
        do { _ = try await T3PrivateHTTP.read(request); XCTFail("Followed redirect") }
        catch { XCTAssertEqual(error as? T3Failure, .transport) }
        let result = String(decoding: output.fileHandleForReading.availableData, as: UTF8.self)
        XCTAssertEqual(result.trimmingCharacters(in: .whitespacesAndNewlines), "not-forwarded")
    }
    func testInstalledT3ReadOnlyProbeWhenExplicitlyEnabled() async throws {
        guard ProcessInfo.processInfo.environment["MI_T3_READONLY_PROBE"] == "1" else { throw XCTSkip("Real local T3 requires explicit probe selection") }
        let client = T3AttentionClient()
        do {
            let snapshot = try await client.connect()
            XCTAssertEqual(snapshot.shell.schemaVersion, 2)
            var observed: [T3AttentionEvent: Date] = [:]
            let items = snapshot.shell.attention(environmentID: snapshot.environmentID, now: Date(), observed: &observed)
            print("T3 read-only probe: schema=2 threads=\(snapshot.shell.threads.count) attention=\(items.count)")
            let closed = await client.close()
            XCTAssertTrue(closed, "Read-only session revocation failed")
        } catch {
            _ = await client.close()
            throw error as? T3Failure ?? T3Failure.transport
        }
    }
}
extension T3AttentionControllerTests {
    func testNewRequestWithServerSequenceRollbackClearsExecutableTargets() async throws {
        let rows = [t3Thread(changes: ["pendingRuntimeRequest": t3Request()])]
        let reader = T3TestReader(try t3Snapshot(rows, sequence: 10))
        var opened = false
        let controller = T3AttentionController(client: reader, pollNanoseconds: 60_000_000_000, opener: { _ in opened = true; return true })
        controller.setEnabled(true); await ready(controller)
        reader.value = try t3Snapshot(rows, sequence: 9)
        let result = await controller.open(controller.selectNext(), admitted: { true })
        XCTAssertEqual(result, .failed); XCTAssertFalse(opened); XCTAssertTrue(controller.items.isEmpty)
        XCTAssertEqual(controller.state, .unavailable(.incompatible))
        await controller.shutdown()
    }
}
extension T3AttentionControllerTests {
    func testExpiryRevokesBeforeRenewingAndKeepsExplicitEnablement() async throws {
        let reader = T3TestReader(try t3Snapshot([t3Thread(changes: ["pendingRuntimeRequest": t3Request()])]))
        let controller = T3AttentionController(client: reader, pollNanoseconds: 1_000_000, opener: { _ in true })
        controller.setEnabled(true); await ready(controller)
        let previousCloses = reader.closes
        reader.failures = [.expired]
        for _ in 0..<1000 where reader.connections < 2 { try await Task.sleep(nanoseconds: 1_000_000) }
        XCTAssertEqual(reader.connections, 2); XCTAssertEqual(reader.closes, previousCloses + 1)
        XCTAssertTrue(controller.enabled); XCTAssertEqual(controller.state, .ready)
        await controller.shutdown()
    }
    func testPollAndOpenCanAcceptTheSameCoalescedSnapshot() async throws {
        let probe = T3ClientProbe()
        await probe.setRows([t3Thread(changes: ["pendingRuntimeRequest": t3Request()])])
        let client = T3AttentionClient(discovery: t3Connection, process: probe.process, http: probe.http)
        var opens = 0
        let controller = T3AttentionController(client: client, pollNanoseconds: 10_000_000, opener: { _ in opens += 1; return true })
        controller.setEnabled(true); await ready(controller)
        await probe.configure(holdHTTP: true)
        while !(await probe.waitingHTTP()) { await Task.yield() }
        let task = Task { await controller.open(controller.selectNext(), admitted: { true }) }
        for _ in 0..<50 { await Task.yield() }
        await probe.releaseHTTP()
        let result = await task.value
        XCTAssertEqual(result, .delivered); XCTAssertEqual(opens, 1); XCTAssertEqual(controller.state, .ready)
        await controller.shutdown()
    }
}

@MainActor private final class T3DelayedExpiryReader: T3Reading {
    let snapshot: T3ReadSnapshot
    var waiter: CheckedContinuation<Void, Never>?
    var connections = 0, closes = 0, reads = 0
    init() throws { snapshot = try t3Snapshot([t3Thread(changes: ["pendingRuntimeRequest": t3Request()])]) }
    func connect() async throws -> T3ReadSnapshot { connections += 1; return snapshot }
    func read() async throws -> T3ReadSnapshot {
        reads += 1
        if reads == 1 {
            await withCheckedContinuation { waiter = $0 }
            throw T3Failure.expired
        }
        return snapshot
    }
    func close() async -> Bool { closes += 1; return true }
}
extension T3AttentionControllerTests {
    func testExpiredOldPollCannotOverwriteDisabledConnection() async throws {
        let reader = try T3DelayedExpiryReader()
        let controller = T3AttentionController(client: reader, pollNanoseconds: 1_000_000, opener: { _ in true })
        controller.setEnabled(true); await ready(controller)
        while reader.waiter == nil { await Task.yield() }
        let closes = reader.closes
        controller.setEnabled(false)
        reader.waiter?.resume(); reader.waiter = nil
        try await Task.sleep(nanoseconds: 10_000_000)
        XCTAssertFalse(controller.enabled); XCTAssertEqual(controller.state, .disabled)
        XCTAssertTrue(controller.items.isEmpty); XCTAssertEqual(reader.connections, 1)
        XCTAssertEqual(reader.closes, closes + 1)
        await controller.shutdown()
    }
    func testExpiredOldPollCannotCloseNewGeneration() async throws {
        let reader = try T3DelayedExpiryReader()
        let controller = T3AttentionController(client: reader, pollNanoseconds: 1_000_000, opener: { _ in true })
        controller.setEnabled(true); await ready(controller)
        while reader.waiter == nil { await Task.yield() }
        controller.setEnabled(false); controller.setEnabled(true)
        reader.waiter?.resume(); reader.waiter = nil
        await ready(controller)
        XCTAssertTrue(controller.enabled); XCTAssertEqual(reader.connections, 2)
        XCTAssertEqual(controller.items.count, 1)
        await controller.shutdown()
    }
}
extension T3AttentionClientTests {
    func testLateCancelledIssuanceCannotHideFailedRevocationWithEmptyClose() async throws {
        let probe = T3ClientProbe(); await probe.configure(holdIssue: true, failRevoke: true)
        let client = T3AttentionClient(discovery: t3Connection, process: probe.process, http: probe.http)
        let task = Task { try await client.connect() }
        while !(await probe.waitingIssue()) { await Task.yield() }
        task.cancel()
        _ = await client.close()
        await probe.releaseIssue()
        do { _ = try await task.value; XCTFail("Cancelled issue became usable") }
        catch { XCTAssertEqual(error as? T3Failure, .revocation) }
        for _ in 0..<2 {
            let closed = await client.close()
            XCTAssertFalse(closed, "An empty close must retain unresolved revocation")
        }
        do { _ = try await client.connect(); XCTFail("Issued another credential while revocation was unresolved") }
        catch { XCTAssertEqual(error as? T3Failure, .revocation) }
        let counts = await probe.counts()
        XCTAssertEqual(counts, [1, 1, 0])
    }
}
extension T3AttentionControllerTests {
    func testDisableDuringIssuanceShowsFailedRevocationAfterCancelledLoop() async throws {
        let probe = T3ClientProbe(); await probe.configure(holdIssue: true, failRevoke: true)
        let client = T3AttentionClient(discovery: t3Connection, process: probe.process, http: probe.http)
        let controller = T3AttentionController(client: client, opener: { _ in true })
        controller.setEnabled(true)
        while !(await probe.waitingIssue()) { await Task.yield() }
        controller.setEnabled(false)
        await probe.releaseIssue()
        for _ in 0..<1000 where controller.state != .unavailable(.revocation) { try await Task.sleep(nanoseconds: 1_000_000) }
        XCTAssertFalse(controller.enabled); XCTAssertTrue(controller.items.isEmpty)
        XCTAssertEqual(controller.state, .unavailable(.revocation))
        await controller.shutdown()
        XCTAssertEqual(controller.state, .unavailable(.revocation))
    }
}
