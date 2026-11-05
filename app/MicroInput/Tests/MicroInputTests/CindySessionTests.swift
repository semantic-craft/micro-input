import Darwin
import Foundation
import MicroInputCore
import XCTest
@testable import MicroInput

private let cindyAlpha1 = CindyTaskAddress(workspacePath: "/workspace/alpha", sessionId: "sess-alpha-1")
private let cindyAlpha2 = CindyTaskAddress(workspacePath: "/workspace/alpha", sessionId: "sess-alpha-2")
private let cindyBeta1 = CindyTaskAddress(workspacePath: "/workspace/beta", sessionId: "sess-beta-1")

private func makeCindySnapshot(
    rows: [CindyTaskAddress] = [cindyAlpha1, cindyAlpha2, cindyBeta1],
    generation: UInt64 = 1,
    ownerEpoch: String = "owner-1:1",
    selected: CindyTaskAddress? = nil
) -> CindySessionSnapshot {
    CindySessionSnapshot(
        instanceId: "cindy-instance-a",
        generation: generation,
        ownerEpoch: ownerEpoch,
        scopes: [
            CindyScope(workspacePath: "/workspace/alpha"),
            CindyScope(workspacePath: "/workspace/beta"),
        ],
        rows: rows.map {
            CindyTaskRow(
                address: $0,
                title: "Daily Standup",
                phase: "running",
                attention: false,
                pinned: false
            )
        },
        selected: selected
    )
}

private actor CindyTestPeer: CindySessionServing {
    var snapshot = makeCindySnapshot()
    var replyOverride: CindyOpenReply?
    var failure: CindyFailure?
    var connections = 0
    var reads = 0
    var opens: [CindyTaskAddress] = []

    func connect() async -> Result<CindySessionSnapshot, CindyFailure> {
        connections += 1
        if let failure { return .failure(failure) }
        return .success(snapshot)
    }

    func read() async -> Result<CindySessionSnapshot, CindyFailure> {
        reads += 1
        if let failure { return .failure(failure) }
        return .success(snapshot)
    }

    func open(_ selection: CindySelection) async -> CindyOpenReply {
        if failure != nil {
            return CindyOpenReply(
                instanceId: snapshot.instanceId,
                generation: snapshot.generation,
                ownerEpoch: snapshot.ownerEpoch,
                status: .unavailable,
                delivered: nil,
                selected: nil,
                deepLink: nil
            )
        }
        if let replyOverride {
            if replyOverride.status == .confirmed || replyOverride.status == .delivered {
                opens.append(selection.address)
            }
            return replyOverride
        }
        guard snapshot.rows.contains(where: { $0.address == selection.address }) else {
            return CindyOpenReply(
                instanceId: snapshot.instanceId,
                generation: snapshot.generation,
                ownerEpoch: snapshot.ownerEpoch,
                status: .stale,
                delivered: nil,
                selected: nil,
                deepLink: nil
            )
        }
        opens.append(selection.address)
        return CindyOpenReply(
            instanceId: snapshot.instanceId,
            generation: snapshot.generation,
            ownerEpoch: snapshot.ownerEpoch,
            status: .confirmed,
            delivered: selection.address,
            selected: selection.address,
            deepLink: selection.address.deepLink
        )
    }

    func close() async {}

    func setSnapshot(_ value: CindySessionSnapshot) { snapshot = value }
    func setReplyOverride(_ value: CindyOpenReply?) { replyOverride = value }
    func setFailure(_ value: CindyFailure?) { failure = value }
    func counts() -> (Int, Int, Int) { (connections, reads, opens.count) }
    func opened() -> [CindyTaskAddress] { opens }
}

@MainActor
final class CindySessionTests: XCTestCase {
    func testDisabledAndOutOfScopeHaveZeroMetadataReads() async throws {
        let peer = CindyTestPeer()
        let controller = CindySessionController(client: peer, pollNanoseconds: 60_000_000_000)
        await controller.refreshNow()
        let disabledCounts = await peer.counts()
        XCTAssertEqual(disabledCounts.0 + disabledCounts.1 + disabledCounts.2, 0)

        controller.setEnabled(true)
        await controller.refreshNow()
        let valid = try XCTUnwrap(controller.selection(for: cindyAlpha1))
        let outOfScope = CindySelection(
            address: CindyTaskAddress(workspacePath: "/workspace/secret", sessionId: "sess-secret-1"),
            instanceId: valid.instanceId,
            generation: valid.generation,
            ownerEpoch: valid.ownerEpoch,
            localEpoch: valid.localEpoch
        )
        let before = await peer.counts()
        let result = await controller.open(outOfScope, admitted: { true })
        let after = await peer.counts()
        XCTAssertEqual(result, .stale)
        XCTAssertEqual(before.1, after.1)
        XCTAssertEqual(after.2, 0)
        await controller.shutdown()
    }

    func testSameNameTasksRemainDistinctWithinAndAcrossWorkspaces() async throws {
        let peer = CindyTestPeer()
        let controller = CindySessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true)
        await controller.refreshNow()
        XCTAssertEqual(controller.rows.count, 3)

        for target in [cindyAlpha2, cindyBeta1, cindyAlpha1] {
            let selection = try XCTUnwrap(controller.selection(for: target))
            let result = await controller.open(selection, admitted: { true })
            XCTAssertEqual(result, .confirmed)
            XCTAssertEqual(controller.delivered, target)
            XCTAssertEqual(controller.selected, target)
            XCTAssertEqual(controller.lastDeepLink, target.deepLink)
        }

        let opened = await peer.opened()
        XCTAssertEqual(opened, [cindyAlpha2, cindyBeta1, cindyAlpha1])
        await controller.shutdown()
    }

    func testDeliveredAndConfirmedAreStrictlySeparated() async throws {
        let peer = CindyTestPeer()
        let controller = CindySessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true)
        await controller.refreshNow()

        await peer.setReplyOverride(
            CindyOpenReply(
                instanceId: "cindy-instance-a",
                generation: 1,
                ownerEpoch: "owner-1:1",
                status: .delivered,
                delivered: cindyAlpha2,
                selected: nil,
                deepLink: cindyAlpha2.deepLink
            )
        )
        let firstSelection = try XCTUnwrap(controller.selection(for: cindyAlpha2))
        let deliveredResult = await controller.open(firstSelection, admitted: { true })
        XCTAssertEqual(deliveredResult, .delivered)
        XCTAssertEqual(controller.delivered, cindyAlpha2)
        XCTAssertNil(controller.selected)
        XCTAssertEqual(controller.lastDeepLink, "cindy://session/sess-alpha-2")

        await peer.setReplyOverride(nil)
        let secondSelection = try XCTUnwrap(controller.selection(for: cindyAlpha2))
        let confirmedResult = await controller.open(secondSelection, admitted: { true })
        XCTAssertEqual(confirmedResult, .confirmed)
        XCTAssertEqual(controller.delivered, cindyAlpha2)
        XCTAssertEqual(controller.selected, cindyAlpha2)
        await controller.shutdown()
    }

    func testWrongSelectedReadbackDowngradesToUnconfirmed() async throws {
        let peer = CindyTestPeer()
        let controller = CindySessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true)
        await controller.refreshNow()

        await peer.setReplyOverride(
            CindyOpenReply(
                instanceId: "cindy-instance-a",
                generation: 1,
                ownerEpoch: "owner-1:1",
                status: .confirmed,
                delivered: cindyAlpha2,
                selected: cindyAlpha1,
                deepLink: cindyAlpha2.deepLink
            )
        )
        let selection = try XCTUnwrap(controller.selection(for: cindyAlpha2))
        let result = await controller.open(selection, admitted: { true })
        XCTAssertEqual(result, .unconfirmed)
        XCTAssertEqual(controller.delivered, cindyAlpha2)
        XCTAssertNil(controller.selected)
        await controller.shutdown()
    }

    func testOwnerEpochOrGenerationChangeInvalidatesOldSelection() async throws {
        let peer = CindyTestPeer()
        let controller = CindySessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true)
        await controller.refreshNow()

        let oldSelection = try XCTUnwrap(controller.selection(for: cindyAlpha1))
        await peer.setSnapshot(makeCindySnapshot(generation: 2, ownerEpoch: "owner-2:2"))
        await controller.refreshNow()
        XCTAssertEqual(controller.state, .ready)
        XCTAssertEqual(controller.ownerEpoch, "owner-2:2")

        let staleResult = await controller.open(oldSelection, admitted: { true })
        XCTAssertEqual(staleResult, .stale)
        let opened = await peer.opened()
        XCTAssertTrue(opened.isEmpty)
        await controller.shutdown()
    }

    func testStoreQueueRecordsDeliveredWithoutHistoryVisitAndConfirmedWithHistoryVisit() async throws {
        let peer = CindyTestPeer()
        let controller = CindySessionController(client: peer, pollNanoseconds: 60_000_000_000)
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let core = MicroInputCore.MicroInput()
        let executor = RecordingHostActionExecutor()
        let store = Store(
            nativeOwnership: NativeOwnershipController(isolated: true),
            backend: core,
            documentURL: directory.appendingPathComponent("workbench.json"),
            actionExecutor: executor,
            cindySessions: controller
        )
        await store.loadWorkbench()
        await store.installCodingAgentPresets()
        let profile = try XCTUnwrap(store.profile?.id.text)
        await store.setHostActiveProfile(profile)
        controller.setEnabled(true)
        await controller.refreshNow()

        await peer.setReplyOverride(
            CindyOpenReply(
                instanceId: "cindy-instance-a",
                generation: 1,
                ownerEpoch: "owner-1:1",
                status: .delivered,
                delivered: cindyAlpha2,
                selected: nil,
                deepLink: cindyAlpha2.deepLink
            )
        )
        let deliveredSelection = try XCTUnwrap(controller.selection(for: cindyAlpha2))
        XCTAssertTrue(store.openCindySession(deliveredSelection))
        await store.hostExecutionTask?.value
        XCTAssertEqual(store.lastHostActionReceipt?.actionType, "cindy_open")
        XCTAssertEqual(store.lastHostActionReceipt?.outcome, "delivered")
        XCTAssertNotEqual(
            store.navigationHistory.current,
            .session(adapter: "cindy", session: "sess-alpha-2")
        )

        await peer.setReplyOverride(nil)
        let confirmedSelection = try XCTUnwrap(controller.selection(for: cindyAlpha2))
        XCTAssertTrue(store.openCindySession(confirmedSelection))
        await store.hostExecutionTask?.value
        XCTAssertEqual(store.lastHostActionReceipt?.actionType, "cindy_open")
        XCTAssertEqual(store.lastHostActionReceipt?.outcome, "confirmed")
        XCTAssertEqual(
            store.navigationHistory.current,
            .session(adapter: "cindy", session: "sess-alpha-2")
        )
        await store.shutdown()
    }
}

final class CindyGrantTests: XCTestCase {
    func testGrantRequiresPrivateRegularFileAndPrivateParent() throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("cindy-grant-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: folder) }
        let file = folder.appendingPathComponent("cindy-grant.json")
        let grant = CindyGrant(
            version: 1,
            socketPath: "/private/tmp/cindy.sock",
            token: String(repeating: "c", count: 32),
            instanceId: "cindy-instance-a",
            generation: 1,
            ownerEpoch: "owner-1:1",
            scopes: [CindyScope(workspacePath: "/workspace/alpha")]
        )
        try JSONEncoder().encode(grant).write(to: file)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
        XCTAssertEqual(try CindyGrant.load(path: file.path), grant)

        try FileManager.default.setAttributes([.posixPermissions: 0o644], ofItemAtPath: file.path)
        XCTAssertThrowsError(try CindyGrant.load(path: file.path))

        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: folder.path)
        XCTAssertThrowsError(try CindyGrant.load(path: file.path))

        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: folder.path)
        let link = folder.appendingPathComponent("link.json")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: file)
        XCTAssertThrowsError(try CindyGrant.load(path: link.path))
    }
}
