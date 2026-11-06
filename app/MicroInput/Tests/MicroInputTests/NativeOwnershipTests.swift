import XCTest
@testable import MicroInput
import MicroInputCore

@MainActor final class NativeOwnershipTests: XCTestCase {
    private let native = UUID()
    private func reply(_ request: NativeOwnershipRequest, state: String = "released", native: UUID? = nil,
                       epoch: UInt64 = 1, ptt: Bool = true, capability: Bool = true) -> NativeOwnershipReply {
        NativeOwnershipReply(version: 1, requestId: request.requestId, clientInstance: request.clientInstance,
            generation: request.generation, nativeInstance: native ?? self.native, nativeGeneration: epoch,
            state: state, reason: "fixture", inputDrained: true, pttDrained: ptt, lightingDrained: true,
            capabilities: capability ? ["fenced-release-v1"] : [])
    }
    private func release(_ controller: NativeOwnershipController) {
        controller.setDesiredOwner("micro-input")
        let request = controller.beginRequest(microDrained: true)
        controller.receive(reply(request), for: request)
    }
    func testUnknownAndNativeNeverPermitExecution() {
        let controller = NativeOwnershipController(transport: nil)
        XCTAssertFalse(controller.allowsExecution)
        let request = controller.beginRequest(microDrained: true)
        controller.receive(reply(request, state: "native"), for: request)
        XCTAssertFalse(controller.allowsExecution)
        XCTAssertEqual(controller.state, .native(native, 1))
    }
    func testOnlyCompleteFencedReleaseOpensGate() {
        for (ptt, capability) in [(false, true), (true, false), (true, true)] {
            let controller = NativeOwnershipController(transport: nil)
            controller.setDesiredOwner("micro-input")
            let request = controller.beginRequest(microDrained: true)
            controller.receive(reply(request, ptt: ptt, capability: capability), for: request)
            XCTAssertEqual(controller.allowsExecution, ptt && capability)
        }
    }
    func testOldReplyAndFailureCannotOverwriteNewGeneration() {
        let controller = NativeOwnershipController(transport: nil)
        controller.setDesiredOwner("micro-input")
        let old = controller.beginRequest(microDrained: true)
        controller.invalidate("profile changed")
        let current = controller.beginRequest(microDrained: true)
        controller.receive(reply(current, state: "native", epoch: 3), for: current)
        controller.receive(reply(old), for: old)
        controller.failed(old)
        XCTAssertEqual(controller.state, .native(native, 3))
        XCTAssertFalse(controller.allowsExecution)
    }
    func testLossAndProfileChangeRequireNewRelease() {
        let controller = NativeOwnershipController(transport: nil)
        release(controller)
        XCTAssertTrue(controller.allowsExecution)
        controller.invalidate("profile changed")
        XCTAssertFalse(controller.allowsExecution)
        release(controller)
        let request = controller.beginRequest(microDrained: false)
        controller.failed(request)
        XCTAssertFalse(controller.allowsExecution)
    }
    func testForeignChallengeCannotRelease() {
        let controller = NativeOwnershipController(transport: nil)
        controller.setDesiredOwner("micro-input")
        let old = controller.beginRequest(microDrained: true)
        let current = controller.beginRequest(microDrained: true)
        controller.receive(reply(old), for: current)
        XCTAssertFalse(controller.allowsExecution)
    }
    func testNativeRestartAndOlderNativeEpochCannotRelease() {
        let controller = NativeOwnershipController(transport: nil)
        release(controller)
        let request = controller.beginRequest(microDrained: true)
        controller.receive(reply(request, native: UUID()), for: request)
        XCTAssertFalse(controller.allowsExecution)
        let next = controller.beginRequest(microDrained: true)
        controller.receive(reply(next, native: native, epoch: 4), for: next)
        XCTAssertFalse(controller.allowsExecution)
        let stable = controller.beginRequest(microDrained: true)
        controller.receive(reply(stable, epoch: 4), for: stable)
        XCTAssertTrue(controller.allowsExecution)
        let stale = controller.beginRequest(microDrained: true)
        controller.receive(reply(stale, epoch: 3), for: stale)
        XCTAssertFalse(controller.allowsExecution)
    }
    func testReleaseExpiresWithoutCallback() async throws {
        let controller = NativeOwnershipController(transport: nil)
        release(controller)
        XCTAssertTrue(controller.allowsExecution)
        try await Task.sleep(for: .milliseconds(1050))
        XCTAssertFalse(controller.allowsExecution)
        guard case .unknown = controller.state else { return XCTFail("expiry must actively invalidate state") }
    }
    func testNativeRequestCannotAcceptReleasedReply() {
        let controller = NativeOwnershipController(transport: nil)
        let request = controller.beginRequest(microDrained: true)
        controller.receive(reply(request), for: request)
        XCTAssertFalse(controller.allowsExecution)
    }
    func testStoreBlocksDirectActionPTTAndLightingWhileUnknown() async throws {
        let controller = NativeOwnershipController(transport: nil)
        let executor = RecordingHostActionExecutor()
        let store = Store(nativeOwnership: controller, backend: MicroInputCore.MicroInput(), actionExecutor: executor)
        XCTAssertFalse(store.dispatchOpen)
        let receipt = await store.executeHostAction(actionJSON: #"{"action_type":"open_app","bundle_id":"com.xd.cindy"}"#,
            bindingId: "AG13", profileId: "fixture")
        XCTAssertNil(receipt)
        XCTAssertTrue(executor.receipts.isEmpty)
        store.startHeldDictation(adapter: "cindy", binding: "AG10", profile: "fixture")
        XCTAssertFalse(store.pushToTalk.isActive)
        await store.beginLightingPreview()
        XCTAssertFalse(store.startingLightingPreview)
        XCTAssertNil(store.lightingPreviewToken)
        await store.shutdown()
    }
    func testNativePauseClearsQueuedWorkAndAdvancesHostGeneration() async {
        let controller = NativeOwnershipController(transport: nil)
        let store = Store(nativeOwnership: controller, backend: MicroInputCore.MicroInput())
        release(controller)
        let before = store.hostExecutionGeneration
        let request = controller.beginRequest(microDrained: true)
        controller.receive(reply(request, state: "native"), for: request)
        XCTAssertGreaterThan(store.hostExecutionGeneration, before)
        XCTAssertTrue(store.pendingHostActions.isEmpty)
        XCTAssertFalse(store.dispatchOpen)
        let drained = await store.drainForNativeOwnership()
        XCTAssertTrue(drained)
        await store.shutdown()
    }
    private final class PausedExecutor: HostActionExecuting {
        var pending: CheckedContinuation<Void, Never>?
        func execute(actionJSON: String, bindingId: String, profileId: String) async -> HostActionReceipt {
            await withCheckedContinuation { pending = $0 }
            return HostActionReceipt(timestampMs: 0, profileId: profileId, bindingId: bindingId,
                actionType: "open_app", target: "fixture", textByteLen: nil, outcome: "simulated", detail: "fixture")
        }
    }
    func testDrainWaitsForAlreadyAdmittedActionToFinish() async throws {
        let ownership = NativeOwnershipController(transport: nil)
        release(ownership)
        let executor = PausedExecutor()
        let store = Store(nativeOwnership: ownership, backend: MicroInputCore.MicroInput(), actionExecutor: executor)
        let action = Task { await store.executeHostAction(actionJSON: #"{"action_type":"open_app","bundle_id":"fixture"}"#,
            bindingId: "AG13", profileId: "fixture") }
        for _ in 0..<100 where executor.pending == nil { await Task.yield() }
        XCTAssertNotNil(executor.pending)
        ownership.invalidate("native requested")
        var finished = false
        let drain = Task { let result = await store.drainForNativeOwnership(); finished = true; return result }
        try await Task.sleep(for: .milliseconds(20))
        XCTAssertFalse(finished)
        XCTAssertEqual(store.inFlightHostActions, 1)
        executor.pending?.resume(); executor.pending = nil
        _ = await action.value
        let result = await drain.value
        XCTAssertTrue(result)
        XCTAssertEqual(store.inFlightHostActions, 0)
        await store.shutdown()
    }

    private actor StablePeer: NativeOwnershipTransport {
        let instance = UUID()
        var requests: [NativeOwnershipRequest] = []
        func exchange(_ request: NativeOwnershipRequest) async throws -> NativeOwnershipReply {
            requests.append(request)
            return NativeOwnershipReply(version: 1, requestId: request.requestId,
                clientInstance: request.clientInstance, generation: request.generation,
                nativeInstance: instance, nativeGeneration: 1,
                state: request.desiredOwner == "native" ? "native" : "released", reason: "fixture",
                inputDrained: true, pttDrained: true, lightingDrained: true, capabilities: ["fenced-release-v1"])
        }
    }
    func testHeartbeatRenewalKeepsCurrentExecutionGenerationAndShutdownReturnsNative() async throws {
        let peer = StablePeer()
        let ownership = NativeOwnershipController(transport: peer)
        ownership.setDesiredOwner("micro-input")
        ownership.drain = { true }
        var changes = 0
        ownership.changed = { changes += 1 }
        ownership.start()
        try await Task.sleep(for: .milliseconds(1150))
        XCTAssertTrue(ownership.allowsExecution)
        XCTAssertEqual(changes, 1, "renewal must not cancel ongoing PTT or queued work")
        let requests = await peer.requests
        XCTAssertGreaterThanOrEqual(requests.count, 3)
        XCTAssertEqual(Set(requests.map(\.generation)).count, 1)
        XCTAssertEqual(Set(requests.map(\.requestId)).count, requests.count)
        await ownership.returnToNative()
        let final = await peer.requests.last
        XCTAssertEqual(final?.desiredOwner, "native")
        XCTAssertEqual(final?.microDrained, true)
        XCTAssertFalse(ownership.allowsExecution)
    }
    func testUnconfirmedCleanupNeverSendsFinalNativeTakeover() async {
        let peer = StablePeer()
        let ownership = NativeOwnershipController(transport: peer)
        ownership.drain = { false }
        await ownership.returnToNative()
        let requests = await peer.requests
        XCTAssertTrue(requests.isEmpty)
    }

    func testUnbridgedDesktopAllowsExecutionUntilBridgeAppearsAndProfileSyncsWorkbench() async throws {
        let tempDir = FileManager.default.temporaryDirectory.appendingPathComponent("mi-unbridged-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: tempDir) }
        let sockPath = tempDir.appendingPathComponent("cindy-ownership.sock").path
        let docURL = tempDir.appendingPathComponent("workbench.json")

        let ownership = NativeOwnershipController(transport: UnixNativeOwnershipTransport(path: sockPath), allowUnbridgedDesktop: true)
        XCTAssertTrue(ownership.allowsExecution)
        XCTAssertEqual(ownership.summary, "本地宿主控制就绪（Cindy 原生桥未启用）")
        ownership.invalidate("routing changed")
        XCTAssertTrue(ownership.allowsExecution, "Unbridged desktop mode must not block execution on routing invalidation")

        let executor = RecordingHostActionExecutor()
        let store = Store(nativeOwnership: ownership, backend: MicroInputCore.MicroInput(), documentURL: docURL, actionExecutor: executor)
        await store.loadWorkbench()
        await store.prepareDesktopProfiles()
        await store.ensureDefaultAgentLinks()
        XCTAssertGreaterThanOrEqual(store.routing?.app_profile_links.count ?? 0, DesktopAgent.all.count)

        let agProfile = try XCTUnwrap(store.hostProfiles.first { $0.name == "Antigravity" })
        await store.setHostActiveProfile(agProfile.id.text, syncWorkbench: true)
        XCTAssertEqual(store.profile?.id.text, agProfile.id.text, "Selecting host active profile must sync Workbench profileIndex")

        // Once the bridge path appears on disk, fenced ownership immediately takes effect.
        FileManager.default.createFile(atPath: sockPath, contents: Data())
        XCTAssertFalse(ownership.allowsExecution)
        await store.shutdown()
    }

    func testProtectedOaiLayerLocksUIAndCindyCodexProfileHandoff() async throws {
        let tempDir = FileManager.default.temporaryDirectory.appendingPathComponent("mi-protected-oai-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: tempDir) }
        let docURL = tempDir.appendingPathComponent("workbench.json")

        let ownership = NativeOwnershipController(transport: nil)
        let executor = RecordingHostActionExecutor()
        let store = Store(nativeOwnership: ownership, backend: MicroInputCore.MicroInput(), documentURL: docURL, actionExecutor: executor)
        await store.loadWorkbench()
        await store.installCodingAgentPresets()

        let cindy = try XCTUnwrap(store.hostProfiles.first { $0.name == "Cindy" })
        let codex = try XCTUnwrap(store.hostProfiles.first { $0.name == "Codex" })
        let antigravity = try XCTUnwrap(store.hostProfiles.first { $0.name == "Antigravity" })
        let zcode = try XCTUnwrap(store.hostProfiles.first { $0.name == "ZCode" })
        let dsh = try XCTUnwrap(store.hostProfiles.first { $0.name == "DeepSeek Coder (DSH)" })

        let codexIdx = try XCTUnwrap(store.snapshot?.profiles.firstIndex { $0.id.text == codex.id.text })
        store.selectProfile(codexIdx)
        await store.createProtectedOaiLayer()

        XCTAssertTrue(store.isProtectedLayerSelected)
        XCTAssertFalse(store.canEditCurrentLayer)
        XCTAssertNil(store.selectedBinding)

        let searchAction = try XCTUnwrap(store.catalog.first { $0.id == "search" })
        let saved = await store.saveAction(searchAction)
        XCTAssertFalse(saved)
        let settingsSaved = await store.saveControlSettings(mode: "logical_sectors", sectors: "4")
        XCTAssertFalse(settingsSaved)

        await store.editLighting(zone: "backlight", field: "color", value: "#00FFAA", allLayers: false)
        await store.beginLightingPreview()
        XCTAssertNil(store.lightingPreviewToken)
        let exportedLayer = try? await store.presetText(scope: "layer")
        XCTAssertNil(exportedLayer)

        let compact = try store.compileCompactHostKeymap(profileId: codex.id.text)
        XCTAssertLessThan(compact.utf8.count, 1024)
        XCTAssertTrue(compact.contains("KV_OAI_AG00"))
        XCTAssertTrue(compact.contains("VENDOR"))

        await store.setHostActiveProfile(cindy.id.text)
        XCTAssertEqual(ownership.desiredOwner, "native")
        XCTAssertFalse(ownership.allowsExecution)

        await store.setHostActiveProfile(codex.id.text)
        XCTAssertEqual(ownership.desiredOwner, "native")
        XCTAssertFalse(ownership.allowsExecution)

        for workbenchProfile in [antigravity, zcode, dsh] {
            await store.setHostActiveProfile(workbenchProfile.id.text)
            XCTAssertEqual(ownership.desiredOwner, "micro-input")
            XCTAssertFalse(ownership.allowsExecution)
            release(ownership)
            XCTAssertTrue(ownership.allowsExecution)
        }

        await store.setHostActiveProfile(cindy.id.text)
        XCTAssertEqual(ownership.desiredOwner, "native")
        XCTAssertFalse(ownership.allowsExecution)
        await store.shutdown()
    }

    func testRestartedControllerResumesPersistedFenceIdentityAndGeneration() throws {
        let tempDir = FileManager.default.temporaryDirectory.appendingPathComponent("mi-fence-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: tempDir) }
        let sockPath = tempDir.appendingPathComponent("cindy-ownership.sock").path
        let fencePath = sockPath + ".fence"
        let persistedClient = UUID()
        let fenceJSON = #"{"clientInstance":"\#(persistedClient.uuidString)","generation":13}"#
        FileManager.default.createFile(atPath: fencePath, contents: Data(fenceJSON.utf8), attributes: [.posixPermissions: 0o600])

        let restarted = NativeOwnershipController(transport: UnixNativeOwnershipTransport(path: sockPath), allowUnbridgedDesktop: true)
        restarted.setDesiredOwner("micro-input")
        let req = restarted.beginRequest(microDrained: true)
        XCTAssertEqual(req.clientInstance, persistedClient, "Restarted Micro Input must resume its own persisted fence clientInstance instead of colliding as different_client")
        XCTAssertGreaterThan(req.generation, 13, "Restarted Micro Input must advance generation past persisted fence")
    }

    func testUnbridgedDesktopRecoversWhenBridgeSocketDisappearsAndRefencesOnReappear() async throws {
        let tempDir = FileManager.default.temporaryDirectory.appendingPathComponent("mi-unbridged-recover-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: tempDir) }
        let sockPath = tempDir.appendingPathComponent("cindy-ownership.sock").path

        let ownership = NativeOwnershipController(transport: UnixNativeOwnershipTransport(path: sockPath), allowUnbridgedDesktop: true)
        ownership.setDesiredOwner("micro-input")
        ownership.drain = { true }
        XCTAssertTrue(ownership.allowsExecution)

        // Simulate Cindy bridge appearing on disk and exchanging a handshake (setting bridgeObserved and nativeIdentity).
        FileManager.default.createFile(atPath: sockPath, contents: Data())
        ownership.start()
        try await Task.sleep(for: .milliseconds(100))
        let req = ownership.beginRequest(microDrained: true)
        ownership.receive(reply(req, state: "released"), for: req)
        XCTAssertTrue(ownership.allowsExecution)

        // Simulate Cindy exiting and removing cindy-ownership.sock; subsequent exchange fails or socket vanishes.
        try FileManager.default.removeItem(atPath: sockPath)
        let failedReq = ownership.beginRequest(microDrained: false)
        ownership.failed(failedReq)
        try await Task.sleep(for: .milliseconds(600))

        XCTAssertTrue(ownership.allowsExecution, "Once cindy-ownership.sock is removed after Cindy exits, unbridged desktop execution must automatically recover instead of locking out forever")
        XCTAssertEqual(ownership.summary, "本地宿主控制就绪（Cindy 原生桥未启用）")

        // When Cindy launches again and recreates cindy-ownership.sock, fenced ownership must immediately take effect again.
        FileManager.default.createFile(atPath: sockPath, contents: Data())
        XCTAssertFalse(ownership.allowsExecution, "Recreating cindy-ownership.sock must immediately re-fence execution until a new release receipt arrives")
        ownership.stop()
    }

}
