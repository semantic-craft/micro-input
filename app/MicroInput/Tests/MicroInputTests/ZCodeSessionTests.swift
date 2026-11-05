import XCTest
import Foundation
import Darwin
import MicroInputCore
@testable import MicroInput

private let zcodeA = ZCodeTaskAddress(workspacePath: "/workspace/a", workspaceIdentity: "a", taskId: "same-id")
private let zcodeB = ZCodeTaskAddress(workspacePath: "/workspace/b", workspaceIdentity: "b", taskId: "same-id")
private func zcodeSnapshot(rows: [ZCodeTaskAddress] = [zcodeA, zcodeB], generation: UInt64 = 1,
                           selected: ZCodeTaskAddress? = nil) -> ZCodeSessionSnapshot {
    ZCodeSessionSnapshot(instanceId: "instance-a", generation: generation, scopes: [zcodeA.scope, zcodeB.scope],
                         rows: rows.map { ZCodeTaskRow(address: $0, title: "Same name", liveStatus: "idle") }, selected: selected)
}

private actor ZCodeTestPeer: ZCodeSessionServing {
    var value = zcodeSnapshot()
    var reply: ZCodeOpenReply?
    var failed = false
    var connections = 0
    var reads = 0
    var opens: [ZCodeTaskAddress] = []
    var held = false
    var waiting: CheckedContinuation<Void, Never>?
    func connect() async throws -> ZCodeSessionSnapshot { connections += 1; return value }
    func read() async throws -> ZCodeSessionSnapshot {
        reads += 1
        let captured = value
        if held { await withCheckedContinuation { waiting = $0 } }
        if failed { throw ZCodeFailure.unavailable }
        return captured
    }
    func open(_ selection: ZCodeSelection) async throws -> ZCodeOpenReply {
        if failed { throw ZCodeFailure.unavailable }
        guard value.rows.contains(where: { $0.address == selection.address }) else { throw ZCodeFailure.stale }
        opens.append(selection.address)
        return reply ?? ZCodeOpenReply(instanceId: value.instanceId, generation: value.generation,
                                      status: .confirmed, selected: selection.address)
    }
    func close() async {}
    func setValue(_ value: ZCodeSessionSnapshot) { self.value = value }
    func setReply(_ reply: ZCodeOpenReply?) { self.reply = reply }
    func setFailed() { failed = true }
    func hold() { held = true }
    func release() { held = false; waiting?.resume(); waiting = nil }
    func hasWaiter() -> Bool { waiting != nil }
    func counts() -> (Int, Int, Int) { (connections, reads, opens.count) }
    func opened() -> [ZCodeTaskAddress] { opens }
}

@MainActor
final class ZCodeSessionTests: XCTestCase {
    func testDisabledConnectionAndOutOfScopeSelectionHaveZeroCalls() async throws {
        let peer = ZCodeTestPeer()
        let controller = ZCodeSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        await controller.refresh()
        let disabled = await peer.counts()
        XCTAssertEqual(disabled.0 + disabled.1 + disabled.2, 0)
        controller.setEnabled(true); await controller.refresh()
        let valid = try XCTUnwrap(controller.selection(for: zcodeA))
        let invalid = ZCodeSelection(address: ZCodeTaskAddress(workspacePath: "/outside", taskId: "same-id"),
                                     instanceId: valid.instanceId, generation: valid.generation, localEpoch: valid.localEpoch)
        let before = await peer.counts()
        let result = await controller.open(invalid, admitted: { true })
        let after = await peer.counts()
        XCTAssertEqual(result, .stale)
        XCTAssertEqual(before.1, after.1)
        XCTAssertEqual(after.2, 0)
        await controller.shutdown()
    }
    func testSameTitleAndSameTaskIDRemainDistinctAcrossWorkspaces() async throws {
        let peer = ZCodeTestPeer()
        let controller = ZCodeSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true); await controller.refresh()
        XCTAssertEqual(controller.rows.count, 2)
        for address in [zcodeB, zcodeA] {
            let selection = try XCTUnwrap(controller.selection(for: address))
            let result = await controller.open(selection, admitted: { true })
            XCTAssertEqual(result, .confirmed)
            XCTAssertEqual(controller.selected, address)
        }
        let opened = await peer.opened()
        XCTAssertEqual(opened, [zcodeB, zcodeA])
        await controller.shutdown()
    }
    func testWrongSelectedCannotConfirm() async throws {
        let peer = ZCodeTestPeer()
        let controller = ZCodeSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true); await controller.refresh()
        await peer.setReply(ZCodeOpenReply(instanceId: "instance-a", generation: 1, status: .confirmed, selected: zcodeB))
        let result = await controller.open(controller.selection(for: zcodeA), admitted: { true })
        XCTAssertEqual(result, .unconfirmed)
        XCTAssertNil(controller.selected)
        await controller.shutdown()
    }
    func testGenerationChangeAndDisconnectionClearTargetsAndRejectOldSelection() async throws {
        let peer = ZCodeTestPeer()
        let controller = ZCodeSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true); await controller.refresh()
        let old = try XCTUnwrap(controller.selection(for: zcodeA))
        await peer.setValue(zcodeSnapshot(generation: 2))
        await controller.refresh()
        XCTAssertEqual(controller.state, .unavailable(.stale))
        XCTAssertTrue(controller.rows.isEmpty)
        let stale = await controller.open(old, admitted: { true })
        XCTAssertEqual(stale, .stale)
        controller.setEnabled(true); await controller.refresh()
        XCTAssertEqual(controller.state, .ready)
        let rejected = await controller.open(old, admitted: { true })
        XCTAssertEqual(rejected, .stale)
        await peer.setFailed(); await controller.refresh()
        XCTAssertEqual(controller.state, .unavailable(.unavailable))
        XCTAssertTrue(controller.rows.isEmpty)
        let counts = await peer.counts()
        XCTAssertEqual(counts.2, 0)
        await controller.shutdown()
    }
    func testDeletionDoesNotOpenAnotherSameNameTask() async throws {
        let peer = ZCodeTestPeer()
        let controller = ZCodeSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true); await controller.refresh()
        let selection = try XCTUnwrap(controller.selection(for: zcodeA))
        await peer.setValue(zcodeSnapshot(rows: [zcodeB]))
        let result = await controller.open(selection, admitted: { true })
        XCTAssertEqual(result, .stale)
        let opened = await peer.opened()
        XCTAssertTrue(opened.isEmpty)
        XCTAssertTrue(controller.rows.isEmpty)
        await controller.shutdown()
    }
    func testDisableDiscardsLateMetadata() async throws {
        let peer = ZCodeTestPeer()
        let controller = ZCodeSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true); await controller.refresh()
        await peer.hold()
        let pending = Task { await controller.refresh() }
        for _ in 0..<1000 {
            if await peer.hasWaiter() { break }
            await Task.yield()
        }
        let started = await peer.hasWaiter()
        XCTAssertTrue(started)
        controller.setEnabled(false)
        await peer.release(); await pending.value
        XCTAssertEqual(controller.state, .disabled)
        XCTAssertTrue(controller.rows.isEmpty)
        XCTAssertNil(controller.selected)
        await controller.shutdown()
    }
    func testScopeViolationAndDuplicateIdentityRejectWholeSnapshot() async {
        for rows in [[zcodeA, zcodeA], [ZCodeTaskAddress(workspacePath: "/outside", taskId: "same-id")]] {
            let peer = ZCodeTestPeer()
            await peer.setValue(zcodeSnapshot(rows: rows))
            let controller = ZCodeSessionController(client: peer, pollNanoseconds: 60_000_000_000)
            controller.setEnabled(true); await controller.refresh()
            XCTAssertEqual(controller.state, .unavailable(.invalidReply))
            XCTAssertTrue(controller.rows.isEmpty)
            await controller.shutdown()
        }
    }
    func testDeniedHostAdmissionDoesNotSend() async {
        let peer = ZCodeTestPeer()
        let controller = ZCodeSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true); await controller.refresh()
        let result = await controller.open(controller.selection(for: zcodeA), admitted: { false })
        XCTAssertEqual(result, .stale)
        let counts = await peer.counts()
        XCTAssertEqual(counts.2, 0)
        await controller.shutdown()
    }
    func testStoreQueueUsesLeaseAndRedactsFullIdentity() async throws {
        let peer = ZCodeTestPeer()
        let controller = ZCodeSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let core = MicroInputCore.MicroInput()
        let executor = RecordingHostActionExecutor()
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: core,
                          documentURL: directory.appendingPathComponent("workbench.json"),
                          actionExecutor: executor, zcodeSessions: controller)
        await store.loadWorkbench(); await store.installCodingAgentPresets()
        let profile = try XCTUnwrap(store.profile?.id.text)
        await store.setHostActiveProfile(profile)
        controller.setEnabled(true); await controller.refresh()
        let selection = try XCTUnwrap(controller.selection(for: zcodeB))
        XCTAssertTrue(store.openZCodeSession(selection))
        await store.hostExecutionTask?.value
        XCTAssertEqual(store.lastHostActionReceipt?.outcome, "confirmed")
        XCTAssertEqual(store.lastHostActionReceipt?.actionType, "zcode_open")
        XCTAssertEqual(core.hostExecutionsPending(), 0)
        XCTAssertTrue(executor.receipts.isEmpty)
        let receipt = try JSONEncoder().encode(XCTUnwrap(store.lastHostActionReceipt))
        let text = String(decoding: receipt, as: UTF8.self)
        for value in ["same-id", "Same name", "/workspace/", "instance-a"] { XCTAssertFalse(text.contains(value)) }
        controller.setEnabled(false)
        XCTAssertTrue(store.openZCodeSession(selection))
        await store.hostExecutionTask?.value
        let opened = await peer.opened()
        XCTAssertEqual(opened, [zcodeB])
        XCTAssertEqual(store.lastHostActionReceipt?.outcome, "stale")
        XCTAssertEqual(core.hostExecutionsPending(), 0)
        await store.shutdown()
    }
    func testOpenResponseFromAnotherInstanceInvalidatesConnection() async {
        let peer = ZCodeTestPeer()
        let controller = ZCodeSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true); await controller.refresh()
        await peer.setReply(ZCodeOpenReply(instanceId: "replacement", generation: 1, status: .confirmed, selected: zcodeA))
        let result = await controller.open(controller.selection(for: zcodeA), admitted: { true })
        XCTAssertEqual(result, .stale)
        XCTAssertEqual(controller.state, .unavailable(.stale))
        XCTAssertTrue(controller.rows.isEmpty)
        await controller.shutdown()
    }
}

final class ZCodeGrantTests: XCTestCase {
    func testGrantRequiresPrivateRegularFileAndPrivateParent() throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("zcode-grant-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: folder) }
        let file = folder.appendingPathComponent("grant.json")
        let grant = ZCodeGrant(version: 1, socketPath: "/private/socket", token: String(repeating: "a", count: 32),
                               instanceId: "instance-a", generation: 1, scopes: [zcodeA.scope])
        try JSONEncoder().encode(grant).write(to: file)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
        XCTAssertEqual(try ZCodeGrant.load(file), grant)
        try FileManager.default.setAttributes([.posixPermissions: 0o644], ofItemAtPath: file.path)
        XCTAssertThrowsError(try ZCodeGrant.load(file))
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: folder.path)
        XCTAssertThrowsError(try ZCodeGrant.load(file))
        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: folder.path)
        let link = folder.appendingPathComponent("link.json")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: file)
        XCTAssertThrowsError(try ZCodeGrant.load(link))
    }
}
