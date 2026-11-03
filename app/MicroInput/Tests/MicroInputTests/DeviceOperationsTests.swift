import XCTest
@testable import MicroInput
import MicroInputCore

@MainActor private final class HeldExecutor: HostActionExecuting {
    var pending: CheckedContinuation<Void, Never>?
    func execute(actionJSON: String, bindingId: String, profileId: String) async -> HostActionReceipt {
        await withCheckedContinuation { pending = $0 }
        return await RecordingHostActionExecutor().execute(actionJSON: actionJSON, bindingId: bindingId, profileId: profileId)
    }
    func release() { pending?.resume(); pending = nil }
}
final class DeviceOperationsTests: XCTestCase {
    @MainActor func testNativeWriteCapabilityIsClosed() async throws {
        let core = MicroInputCore.MicroInput()
        let caps = try JSONDecoder().decode(DeviceOperationCapabilities.self, from: Data(core.deviceOperationCapabilities().utf8))
        XCTAssertFalse(caps.apply); XCTAssertFalse(caps.restore)
        XCTAssertThrowsError(try core.beginDeviceDrain())
        await core.shutdown()
    }
    #if MI_SYNTHETIC_BACKEND
    @MainActor private func prepared(_ dir: URL, executor: HostActionExecuting? = nil) async throws -> (MicroInputCore.MicroInput, Store, String) {
        let core = MicroInputCore.MicroInput.syntheticConfigWriter()
        let store = Store(backend: core, documentURL: dir.appendingPathComponent("workbench.json"), actionExecutor: executor ?? RecordingHostActionExecutor())
        await store.loadWorkbench()
        try await core.setSyntheticDiscovery(state: .twoDevices)
        await store.refresh(); store.selectDevice("synthetic-interface-0"); await store.connect("synthetic-interface-0")
        await store.readIdentity(); await store.readDeviceConfig(); await store.createDraftFromSnapshot()
        await store.assign("device-key"); await store.previewApply(); await store.prepareApplyBackup()
        let token = try XCTUnwrap(store.preparedApplyBackup?.token, store.status)
        return (core, store, token)
    }
    @MainActor func testHighLevelApplyRestoreAndConsumedToken() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: dir) }
        let (core, store, token) = try await prepared(dir)
        await store.performDeviceOperation(token: token)
        XCTAssertEqual(store.operationResult?.phase, "Verified", store.status)
        XCTAssertFalse(store.hostOperationPaused)
        let id = try XCTUnwrap(store.operationResult?.operation_id)
        await store.prepareDeviceRestore(id)
        let restore = try XCTUnwrap(store.restorePreview, store.status)
        XCTAssertNotEqual(restore.token, token)
        await store.performDeviceOperation(token: restore.token, restore: true)
        XCTAssertEqual(store.operationResult?.phase, "Verified", store.status)
        XCTAssertEqual(store.operationRows.count, 2)
        let count = await core.syntheticRequests().count
        let drain = try core.beginDeviceDrain()
        do { _ = try await core.startDeviceApply(token: restore.token, drainToken: drain, restore: true); XCTFail("replayed consumed plan") } catch {}
        core.abandonDeviceDrain(token: drain)
        let after = await core.syntheticRequests().count
        XCTAssertEqual(count, after)
        await store.shutdown()
    }
    @MainActor func testHeldExecutorDrainsAndLateReceiptCannotPublish() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: dir) }
        let executor = HeldExecutor()
        let (core, store, token) = try await prepared(dir, executor: executor)
        let host = Task { await store.executeHostAction(actionJSON: "{}", bindingId: "AG01", profileId: "synthetic") }
        while executor.pending == nil { await Task.yield() }
        let count = await core.syntheticRequests().count
        let operation = Task { await store.performDeviceOperation(token: token) }
        while !store.hostOperationPaused { await Task.yield() }
        try await Task.sleep(nanoseconds: 30_000_000)
        let whileHeld = await core.syntheticRequests().count
        XCTAssertEqual(count, whileHeld)
        XCTAssertThrowsError(try core.beginHostExecution())
        executor.release()
        let receipt = await host.value; XCTAssertNil(receipt)
        await operation.value
        XCTAssertEqual(store.operationResult?.phase, "Verified", store.status)
        XCTAssertNil(store.lastHostActionReceipt)
        XCTAssertEqual(core.hostExecutionsPending(), 0)
        await store.shutdown()
    }
    @MainActor func testRoutingQueueDrainsAndDiscardsPendingActions() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: dir) }
        let executor = HeldExecutor()
        let (core, store, token) = try await prepared(dir, executor: executor)
        let state = try XCTUnwrap(store.routing)
        XCTAssertTrue(store.enqueueHostAction(json: "{}", binding: "AG01", profile: "synthetic", state: state, requiresEnabled: false))
        while executor.pending == nil { await Task.yield() }
        XCTAssertEqual(core.hostExecutionsPending(), 1)
        XCTAssertTrue(store.enqueueHostAction(json: "{}", binding: "AG02", profile: "synthetic", state: state, requiresEnabled: false))
        let count = await core.syntheticRequests().count
        let operation = Task { await store.performDeviceOperation(token: token) }
        while !store.hostOperationPaused { await Task.yield() }
        try await Task.sleep(nanoseconds: 30_000_000)
        let whileHeld = await core.syntheticRequests().count
        XCTAssertEqual(count, whileHeld)
        XCTAssertThrowsError(try core.beginHostExecution())
        XCTAssertTrue(store.pendingHostActions.isEmpty)
        XCTAssertFalse(store.dispatchOpen)
        executor.release()
        await operation.value
        XCTAssertEqual(store.operationResult?.phase, "Verified", store.status)
        XCTAssertNil(store.lastHostActionReceipt)
        XCTAssertEqual(core.hostExecutionsPending(), 0)
        await store.shutdown()
    }
    @MainActor func testDrainTimeoutAndCancellationSendNothing() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: dir) }
        let (core, store, token) = try await prepared(dir)
        let lease = try core.beginHostExecution()
        let before = await core.syntheticRequests().count
        store.drainTimeout = 0.02
        await store.performDeviceOperation(token: token)
        XCTAssertEqual(store.operationResult?.phase, "NotSent")
        let after = await core.syntheticRequests().count; XCTAssertEqual(before, after)
        store.drainTimeout = 2
        let task = Task { await store.performDeviceOperation(token: token) }
        while !store.deviceOperationRunning { await Task.yield() }
        store.cancelActiveDeviceOperation()
        await task.value
        XCTAssertEqual(store.operationResult?.phase, "NotSent")
        let cancelled = await core.syntheticRequests().count; XCTAssertEqual(before, cancelled)
        core.finishHostExecution(lease: lease)
        await store.performDeviceOperation(token: token)
        XCTAssertEqual(store.operationResult?.phase, "Verified", store.status)
        await store.shutdown()
    }
    @MainActor func testRestorePlanCannotSurviveReconnectOrCancellation() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: dir) }
        let (core, store, token) = try await prepared(dir)
        await store.performDeviceOperation(token: token)
        let id = try XCTUnwrap(store.operationResult?.operation_id)
        await store.prepareDeviceRestore(id)
        let cancelled = try XCTUnwrap(store.restorePreview?.token)
        store.discardDeviceRestore()
        var drain = try core.beginDeviceDrain()
        do { _ = try await core.startDeviceApply(token: cancelled, drainToken: drain, restore: true); XCTFail("cancelled restore executed") } catch {}
        core.abandonDeviceDrain(token: drain)
        await store.prepareDeviceRestore(id)
        let stale = try XCTUnwrap(store.restorePreview?.token)
        await core.disconnect(); _ = try await core.connect(id: "synthetic-interface-0")
        let before = await core.syntheticRequests().count
        drain = try core.beginDeviceDrain()
        do { _ = try await core.startDeviceApply(token: stale, drainToken: drain, restore: true); XCTFail("old session plan executed") } catch {}
        core.abandonDeviceDrain(token: drain)
        let after = await core.syntheticRequests().count; XCTAssertEqual(before, after)
        await store.shutdown()
    }
    @MainActor func testLocalRepairAfterDisconnectAndReopenSendsNothing() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: dir) }
        let (core, store, token) = try await prepared(dir)
        try core.setSyntheticLocalSaveFailure(enabled: true)
        await store.performDeviceOperation(token: token)
        XCTAssertEqual(store.operationResult?.phase, "VerifiedLocalSaveFailed", store.status)
        let id = try XCTUnwrap(store.operationResult?.operation_id)
        await core.disconnect()
        let requests = await core.syntheticRequests().count
        await store.shutdown()
        let reopenedCore = MicroInputCore.MicroInput()
        let reopened = Store(backend: reopenedCore, documentURL: dir.appendingPathComponent("workbench.json"))
        await reopened.loadWorkbench(); await reopened.refreshDeviceOperations()
        XCTAssertEqual(reopened.operationRows.count, 1)
        XCTAssertFalse(reopened.operationCapabilities.apply)
        XCTAssertEqual(reopened.operationRows.first?.phase, "VerifiedLocalSaveFailed")
        await reopened.repairLocalOperation(id)
        XCTAssertEqual(reopened.operationRows.first?.phase, "Verified")
        let after = await core.syntheticRequests().count; XCTAssertEqual(requests, after)
        await reopened.repairLocalOperation(id)
        XCTAssertEqual(reopened.operationRows.first?.phase, "Verified")
        await reopened.shutdown()
    }
    #endif
}
