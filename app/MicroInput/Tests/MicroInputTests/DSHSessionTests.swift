import XCTest
import Foundation
import MicroInputCore
@testable import MicroInput

private func dshRow(_ id: String = "session-a", pending: UInt64 = 0) -> DSHSessionRow {
    DSHSessionRow(sessionId: id, workspaceId: "workspace-\(id)", title: "Same name", active: true, running: true, pendingInput: pending)
}
private func dshSnapshot(generation: UInt64 = 1, cursor: UInt64 = 0, changes: [DSHSessionChange] = [],
                         rows: [DSHSessionRow] = [dshRow(), dshRow("session-b")]) -> DSHSessionSnapshot {
    DSHSessionSnapshot(hostId: "host-a", generation: generation, sessionIds: ["session-a", "session-b", "cold"],
        cursor: cursor, rows: rows, unavailableSessionIds: ["cold"], changes: changes, selected: nil)
}
private actor DSHTestPeer: DSHSessionServing {
    var value = dshSnapshot()
    var reply: DSHOpenReply?
    var reads = 0
    var connections = 0
    var closes = 0
    var opens: [String] = []
    var heldConnect = false
    var connectWaiting: CheckedContinuation<Void, Never>?
    var held = false
    var heldOpen = false
    var openWaiting: CheckedContinuation<Void, Never>?
    var heldClose = false
    var closeWaiting: CheckedContinuation<Void, Never>?
    var waiting: CheckedContinuation<Void, Never>?
    func connect() async throws -> DSHSessionSnapshot {
        connections += 1
        let captured = value
        if heldConnect { await withCheckedContinuation { connectWaiting = $0 } }
        return captured
    }
    func read() async throws -> DSHSessionSnapshot {
        reads += 1
        let captured = value
        if held { await withCheckedContinuation { waiting = $0 } }
        return captured
    }
    func open(_ selection: DSHSelection) async throws -> DSHOpenReply {
        opens.append(selection.sessionId)
        let captured = reply ?? DSHOpenReply(hostId: value.hostId, generation: value.generation, status: .confirmed, selected: selection.sessionId)
        if heldOpen { await withCheckedContinuation { openWaiting = $0 } }
        return captured
    }
    func close() async {
        closes += 1
        if heldClose { await withCheckedContinuation { closeWaiting = $0 } }
    }
    func set(_ value: DSHSessionSnapshot) { self.value = value }
    func setReply(_ value: DSHOpenReply) { reply = value }
    func counts() -> (Int, Int, Int, Int) { (connections, reads, opens.count, closes) }
    func opened() -> [String] { opens }
    func holdConnect() { heldConnect = true }
    func hasConnectWaiter() -> Bool { connectWaiting != nil }
    func releaseConnect() { heldConnect = false; connectWaiting?.resume(); connectWaiting = nil }
    func hold() { held = true }
    func hasWaiter() -> Bool { waiting != nil }
    func release() { held = false; waiting?.resume(); waiting = nil }
    func holdOpen() { heldOpen = true }
    func hasOpenWaiter() -> Bool { openWaiting != nil }
    func releaseOpen() { heldOpen = false; openWaiting?.resume(); openWaiting = nil }
    func holdClose() { heldClose = true }
    func hasCloseWaiter() -> Bool { closeWaiting != nil }
    func releaseClose() { heldClose = false; closeWaiting?.resume(); closeWaiting = nil }
}

@MainActor
final class DSHSessionTests: XCTestCase {
    func testDisabledAndUnscopedSelectionsNeverReadOrOpen() async throws {
        let peer = DSHTestPeer()
        let actual = DSHSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        await actual.refresh()
        var counts = await peer.counts()
        XCTAssertEqual(counts.0 + counts.1 + counts.2, 0)
        actual.setEnabled(true); await actual.refresh()
        let valid = try XCTUnwrap(actual.selection(for: "session-a"))
        let invalid = DSHSelection(sessionId: "outside", workspaceId: valid.workspaceId,
            hostId: valid.hostId, generation: valid.generation, localEpoch: valid.localEpoch)
        let before = await peer.counts()
        let result = await actual.open(invalid, admitted: { true })
        counts = await peer.counts()
        XCTAssertEqual(result, .stale)
        XCTAssertEqual(before.1, counts.1)
        XCTAssertEqual(counts.2, 0)
        XCTAssertNil(actual.selection(for: "cold"))
        XCTAssertEqual(actual.unavailableSessionIds, ["cold"])
        await actual.shutdown()
    }
    func testSameNameTargetsNavigateByExactIDAndWorkspace() async throws {
        let peer = DSHTestPeer()
        let controller = DSHSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true); await controller.refresh()
        for id in ["session-b", "session-a"] {
            let selection = try XCTUnwrap(controller.selection(for: id))
            let result = await controller.open(selection, admitted: { true })
            XCTAssertEqual(result, .confirmed)
            XCTAssertEqual(controller.selected, id)
        }
        let opened = await peer.opened()
        XCTAssertEqual(opened, ["session-b", "session-a"])
        let original = try XCTUnwrap(controller.selection(for: "session-a"))
        let wrongWorkspace = DSHSelection(sessionId: original.sessionId, workspaceId: "other", hostId: original.hostId,
            generation: original.generation, localEpoch: original.localEpoch)
        let result = await controller.open(wrongWorkspace, admitted: { true })
        XCTAssertEqual(result, .stale)
        await controller.shutdown()
    }
    func testWrongSelectedIDCannotConfirm() async {
        let peer = DSHTestPeer()
        let controller = DSHSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true); await controller.refresh()
        await peer.setReply(DSHOpenReply(hostId: "host-a", generation: 1, status: .confirmed, selected: "session-b"))
        let result = await controller.open(controller.selection(for: "session-a"), admitted: { true })
        XCTAssertEqual(result, .unconfirmed)
        XCTAssertNil(controller.selected)
        await controller.shutdown()
    }
    func testPendingOnlyRoundTripIsNotCollapsedByPolling() async {
        let peer = DSHTestPeer()
        let controller = DSHSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true); await controller.refresh()
        let changes = [DSHSessionChange(sequence: 1, row: dshRow(pending: 1)), DSHSessionChange(sequence: 2, row: dshRow())]
        await peer.set(dshSnapshot(cursor: 2, changes: changes))
        await controller.refresh()
        XCTAssertEqual(controller.state, .ready)
        XCTAssertEqual(controller.lastChanges.map(\.row.pendingInput), [1, 0])
        XCTAssertEqual(controller.cursor, 2)
        XCTAssertEqual(controller.rows.first?.pendingInput, 0)
        await controller.shutdown()
    }
    func testJournalGapInvalidatesTargets() async {
        let peer = DSHTestPeer()
        let controller = DSHSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true); await controller.refresh()
        let old = controller.selection(for: "session-a")
        await peer.set(dshSnapshot(cursor: 2, changes: [DSHSessionChange(sequence: 2, row: dshRow())]))
        await controller.refresh()
        XCTAssertEqual(controller.state, .unavailable(.gap))
        XCTAssertTrue(controller.rows.isEmpty)
        let result = await controller.open(old, admitted: { true })
        XCTAssertEqual(result, .stale)
        await controller.shutdown()
    }
    func testGenerationChangeAndDisableDiscardOldSelections() async {
        let peer = DSHTestPeer()
        let controller = DSHSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true); await controller.refresh()
        let old = controller.selection(for: "session-a")
        await peer.set(dshSnapshot(generation: 2)); await controller.refresh()
        XCTAssertEqual(controller.state, .unavailable(.stale))
        controller.setEnabled(true); await controller.refresh()
        let result = await controller.open(old, admitted: { true })
        XCTAssertEqual(result, .stale)
        await controller.shutdown()
        let counts = await peer.counts()
        XCTAssertEqual(counts.2, 0)
        XCTAssertGreaterThan(counts.3, 0)
    }
    func testDisableDiscardsLateMetadata() async {
        let peer = DSHTestPeer()
        let controller = DSHSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true); await controller.refresh(); await peer.hold()
        let pending = Task { await controller.refresh() }
        for _ in 0..<1000 { if await peer.hasWaiter() { break }; await Task.yield() }
        let started = await peer.hasWaiter(); XCTAssertTrue(started)
        controller.setEnabled(false); await peer.release(); await pending.value
        XCTAssertEqual(controller.state, .disabled)
        XCTAssertTrue(controller.rows.isEmpty)
        await controller.shutdown()
    }
    func testLateOldReadCannotReplaceNewConnection() async {
        let peer = DSHTestPeer()
        let controller = DSHSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true); await controller.refresh(); await peer.hold()
        let oldRead = Task { await controller.refresh() }
        for _ in 0..<1000 { if await peer.hasWaiter() { break }; await Task.yield() }
        let waiting = await peer.hasWaiter(); XCTAssertTrue(waiting)
        controller.setEnabled(false)
        await peer.set(dshSnapshot(generation: 2))
        controller.setEnabled(true)
        for _ in 0..<1000 { if controller.state == .ready { break }; await Task.yield() }
        XCTAssertEqual(controller.selection(for: "session-a")?.generation, 2)
        await peer.release(); await oldRead.value
        XCTAssertEqual(controller.state, .ready)
        XCTAssertEqual(controller.selection(for: "session-a")?.generation, 2)
        await controller.shutdown()
    }
    func testLateOpenCannotConfirmAcrossReconnect() async {
        let peer = DSHTestPeer()
        let controller = DSHSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true); await controller.refresh(); await peer.holdOpen()
        let selection = controller.selection(for: "session-a")
        let oldOpen = Task { await controller.open(selection, admitted: { true }) }
        for _ in 0..<1000 { if await peer.hasOpenWaiter() { break }; await Task.yield() }
        let waiting = await peer.hasOpenWaiter(); XCTAssertTrue(waiting)
        controller.setEnabled(false); await peer.set(dshSnapshot(generation: 2))
        controller.setEnabled(true)
        for _ in 0..<1000 { if controller.state == .ready { break }; await Task.yield() }
        await peer.releaseOpen()
        let result = await oldOpen.value
        XCTAssertEqual(result, .stale)
        XCTAssertNil(controller.selected)
        XCTAssertNil(controller.lastResult)
        XCTAssertEqual(controller.selection(for: "session-a")?.generation, 2)
        await controller.shutdown()
    }
    func testReconnectWaitsForLateSubscribeBeforeClosingItsSubscription() async {
        let peer = DSHTestPeer()
        let controller = DSHSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        await peer.holdConnect()
        controller.setEnabled(true)
        for _ in 0..<1000 { if await peer.hasConnectWaiter() { break }; await Task.yield() }
        let waiting = await peer.hasConnectWaiter(); XCTAssertTrue(waiting)
        let before = await peer.counts()
        controller.setEnabled(false)
        await peer.set(dshSnapshot(generation: 2))
        controller.setEnabled(true)
        for _ in 0..<100 { await Task.yield() }
        let during = await peer.counts()
        XCTAssertEqual(during.0, before.0)
        XCTAssertEqual(during.3, before.3)
        XCTAssertEqual(controller.state, .connecting)
        await peer.releaseConnect()
        await controller.refresh()
        XCTAssertEqual(controller.state, .ready)
        XCTAssertEqual(controller.selection(for: "session-a")?.generation, 2)
        await controller.shutdown()
    }
    func testReconnectWaitsForPreviousClose() async {
        let peer = DSHTestPeer()
        let controller = DSHSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true); await controller.refresh(); await peer.holdClose()
        let before = await peer.counts()
        controller.setEnabled(false); controller.setEnabled(true)
        for _ in 0..<1000 { if await peer.hasCloseWaiter() { break }; await Task.yield() }
        let waiting = await peer.hasCloseWaiter(); XCTAssertTrue(waiting)
        let during = await peer.counts()
        XCTAssertEqual(before.0, during.0)
        XCTAssertEqual(controller.state, .connecting)
        await peer.releaseClose(); await controller.refresh()
        XCTAssertEqual(controller.state, .ready)
        await controller.shutdown()
    }
    func testMalformedPartitionOrRepeatedSequenceRejectsWholeState() throws {
        XCTAssertThrowsError(try dshSnapshot(rows: [dshRow(), dshRow()]).validate())
        XCTAssertThrowsError(try dshSnapshot(cursor: 2, changes: [
            DSHSessionChange(sequence: 1, row: dshRow()), DSHSessionChange(sequence: 1, row: dshRow())
        ]).validate(after: 0))
    }
    func testStoreQueueUsesHostLeaseAndRedactsIdentity() async throws {
        let peer = DSHTestPeer()
        let controller = DSHSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let core = MicroInputCore.MicroInput(), executor = RecordingHostActionExecutor()
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: core,
            documentURL: directory.appendingPathComponent("workbench.json"), actionExecutor: executor, dshSessions: controller)
        await store.loadWorkbench(); await store.installCodingAgentPresets()
        await store.setHostActiveProfile(try XCTUnwrap(store.profile?.id.text))
        controller.setEnabled(true); await controller.refresh()
        let selection = try XCTUnwrap(controller.selection(for: "session-a"))
        XCTAssertTrue(store.openDSHSession(selection)); await store.hostExecutionTask?.value
        XCTAssertEqual(store.lastHostActionReceipt?.outcome, "confirmed")
        XCTAssertEqual(store.lastHostActionReceipt?.actionType, "dsh_open")
        XCTAssertEqual(core.hostExecutionsPending(), 0)
        XCTAssertTrue(executor.receipts.isEmpty)
        let data = try JSONEncoder().encode(XCTUnwrap(store.lastHostActionReceipt))
        let text = String(decoding: data, as: UTF8.self)
        for value in ["session-a", "workspace-", "host-a", "Same name"] { XCTAssertFalse(text.contains(value)) }
        controller.setEnabled(false)
        XCTAssertTrue(store.openDSHSession(selection)); await store.hostExecutionTask?.value
        XCTAssertEqual(store.lastHostActionReceipt?.outcome, "stale")
        let opened = await peer.opened(); XCTAssertEqual(opened, ["session-a"])
        await store.shutdown()
    }
}

final class DSHGrantTests: XCTestCase {
    func testGrantRequiresPrivateFileAndParent() throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("dsh-grant-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: folder) }
        let file = folder.appendingPathComponent("grant.json")
        let grant = DSHGrant(version: 1, socketPath: "/private/socket", token: String(repeating: "a", count: 32),
            hostId: "host-a", generation: 1, sessionIds: ["session-a"])
        try JSONEncoder().encode(grant).write(to: file)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
        XCTAssertEqual(try DSHGrant.load(file), grant)
        try FileManager.default.setAttributes([.posixPermissions: 0o644], ofItemAtPath: file.path)
        XCTAssertThrowsError(try DSHGrant.load(file))
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: folder.path)
        XCTAssertThrowsError(try DSHGrant.load(file))
        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: folder.path)
        let link = folder.appendingPathComponent("link.json")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: file)
        XCTAssertThrowsError(try DSHGrant.load(link))
    }
}
