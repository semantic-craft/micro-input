#if MI_SYNTHETIC_BACKEND
import Foundation
import MicroInputCore

extension Store {
    func prepareNativeOwnershipScenario() async {
        guard let core = backend as? MicroInputCore.MicroInput,
              let executor = actionExecutor as? RecordingHostActionExecutor else { return }
        let instance = UUID()
        var checks: [String: Bool] = [:]
        let peerPath = ProcessInfo.processInfo.environment["MI_OWNERSHIP_FIXTURE_SOCKET"]
        var pairedEvents: [[String: Any]] = []
        var phase = "native"
        func control(_ object: [String: Any]) async throws -> [String: Any] {
            guard let peerPath else { return [:] }
            let data = try JSONSerialization.data(withJSONObject: object)
            let response = try await UnixNativeOwnershipTransport(path: peerPath + ".control").exchangeJSON(data)
            guard let result = try JSONSerialization.jsonObject(with: response) as? [String: Any] else {
                throw BridgeError.Failed(message: "invalid fixture reply")
            }
            return result
        }
        func reply(_ request: NativeOwnershipRequest, state: String) -> NativeOwnershipReply {
            NativeOwnershipReply(version: 1, requestId: request.requestId, clientInstance: request.clientInstance,
                generation: request.generation, nativeInstance: instance, nativeGeneration: 1, state: state,
                reason: "synthetic peer", inputDrained: true, pttDrained: true, lightingDrained: true,
                capabilities: ["fenced-release-v1"])
        }
        func exchange(_ state: String) async throws {
            nativeOwnership.setDesiredOwner(state == "native" ? "native" : "micro-input")
            let request = nativeOwnership.beginRequest(microDrained: true)
            let response: NativeOwnershipReply
            if let peerPath { response = try await UnixNativeOwnershipTransport(path: peerPath).exchange(request) }
            else { response = reply(request, state: state) }
            nativeOwnership.receive(response, for: request)
        }
        func emit(wide: Bool) async throws {
            for kind in (wide ? ["wide", "encoder"] : ["encoder"]) {
                let binding = kind == "wide" ? "AG10" : "AG14"
                let before = executor.receipts.count
                let eventID = "\(phase)-\(kind)"
                let native = try await control(["eventId": eventID, "control": kind])
                inputBoundary.released.remove(binding)
                let inputs = kind == "wide"
                    ? [("AG10", true), ("AG11", true), ("AG10", false), ("AG11", false)]
                    : [("AG14", true), ("AG14", false)]
                for (key, pressed) in inputs { try await core.emitSyntheticBindingEvent(bindingId: key, pressed: pressed) }
                for _ in 0..<200 where !inputBoundary.released.contains(binding) {
                    try await Task.sleep(for: .milliseconds(5))
                }
                guard inputBoundary.released.contains(binding) else { throw BridgeError.Failed(message: "input not consumed") }
                await hostExecutionTask?.value
                if peerPath != nil {
                    guard native["eventId"] as? String == eventID, let count = native["nativeBusinessActions"] as? Int else {
                        throw BridgeError.Failed(message: "native event receipt missing")
                    }
                    pairedEvents.append(["event_id": eventID, "native_actions": count,
                        "micro_actions": executor.receipts.count - before])
                }
            }
        }
        do {
            await installCodingAgentPresets()
            await setHostExecutionEnabled(true)
            try await core.setSyntheticDiscovery(state: .reconnectedDevice)
            try await core.setSyntheticConfig(scenario: 1)
            await refresh(); selectDevice("synthetic-reconnected-interface-0")
            await connect("synthetic-reconnected-interface-0")
            await readIdentity(); await readDeviceConfig(); showLocalDraft()
            guard let herdr = hostProfiles.first(where: { $0.name == "Herdr" }),
                  let antigravity = hostProfiles.first(where: { $0.name == "Antigravity" }) else {
                throw BridgeError.Failed(message: "profiles missing")
            }
            guard let index = snapshot?.profiles.firstIndex(where: { $0.id.text == herdr.id.text }),
                  let action = catalog.first(where: { $0.id == "search" }) else {
                throw BridgeError.Failed(message: "fixture action missing")
            }
            selectProfile(index); slot = 14
            guard await saveAction(action) else { throw BridgeError.Failed(message: "fixture binding failed") }
            await setHostActiveProfile(herdr.id.text)
            try await Task.sleep(for: .milliseconds(50))
            try await exchange("native")
            try await emit(wide: true)
            checks["native_blocks_wide_and_encoder"] = executor.receipts.isEmpty
            phase = "released"
            try await exchange("released")
            try await emit(wide: true)
            checks["released_wide_once_encoder_once"] = executor.receipts.count == 2
                && executor.receipts.filter { ["AG10", "AG11"].contains($0.bindingId) }.count == 1
                && executor.receipts.filter { $0.bindingId == "AG14" }.count == 1
            let old = nativeOwnership.beginRequest(microDrained: true)
            let delayed: NativeOwnershipReply
            if let peerPath { delayed = try await UnixNativeOwnershipTransport(path: peerPath).exchange(old) }
            else { delayed = reply(old, state: "released") }
            await setHostActiveProfile(antigravity.id.text)
            phase = "stale-profile"
            nativeOwnership.receive(delayed, for: old)
            try await emit(wide: false)
            checks["profile_switch_rejects_old_release"] = executor.receipts.count == 2 && !nativeOwnership.allowsExecution
            phase = "fresh"
            try await exchange("released")
            try await emit(wide: false)
            checks["fresh_release_routes_current_profile"] = executor.receipts.count == 3
                && executor.receipts.last?.profileId == antigravity.id.text
            let lost = nativeOwnership.beginRequest(microDrained: false)
            phase = "lost"
            if let peerPath {
                _ = try await control(["cmd": "disconnect"])
                var disconnected = false
                do { _ = try await UnixNativeOwnershipTransport(path: peerPath).exchange(lost) }
                catch { disconnected = true; nativeOwnership.failed(lost) }
                guard disconnected else { throw BridgeError.Failed(message: "peer did not disconnect") }
            } else { nativeOwnership.failed(lost) }
            try await emit(wide: true)
            checks["lost_peer_blocks_input"] = executor.receipts.count == 3 && !nativeOwnership.allowsExecution
            let direct = await executeHostAction(actionJSON: #"{"action_type":"open_app","bundle_id":"com.xd.cindy"}"#,
                bindingId: "ui-direct", profileId: antigravity.id.text)
            checks["unknown_blocks_direct_action"] = direct == nil && executor.receipts.count == 3
            startHeldDictation(adapter: "cindy", binding: "AG10", profile: antigravity.id.text)
            await beginLightingPreview()
            checks["unknown_blocks_ptt_and_lighting"] = !pushToTalk.isActive && lightingPreviewToken == nil

            guard let cindy = hostProfiles.first(where: { $0.name == "Cindy" }),
                  let codex = hostProfiles.first(where: { $0.name == "Codex" }),
                  let zcode = hostProfiles.first(where: { $0.name == "ZCode" }),
                  let dsh = hostProfiles.first(where: { $0.name == "DeepSeek Coder (DSH)" }),
                  let cindyIndex = snapshot?.profiles.firstIndex(where: { $0.id.text == cindy.id.text }),
                  let codexIndex = snapshot?.profiles.firstIndex(where: { $0.id.text == codex.id.text }) else {
                throw BridgeError.Failed(message: "protected or workbench profiles missing")
            }
            selectProfile(cindyIndex)
            let cindyProtected = isProtectedLayerSelected && !canEditCurrentLayer && selectedBinding == nil
            selectProfile(codexIndex)
            let protectedSelected = cindyProtected && isProtectedLayerSelected && !canEditCurrentLayer && selectedBinding == nil
            let saveRejected = !(await saveAction(action))
            let settingsRejected = !(await saveControlSettings(mode: "logical_sectors", sectors: "4"))
            await editLighting(zone: "backlight", field: "color", value: "#3377CC", allLayers: false)
            await beginLightingPreview()
            let lightingBlocked = lightingPreviewToken == nil && status.contains("受保护宿主层")
            let exportLayerBlocked = (try? await presetText(scope: "layer")) == nil
            let layerCountBeforeDup = profile?.layers.count ?? 0
            await duplicateLayer(name: "ChatGPT Copy")
            let duplicateBlocked = (profile?.layers.count ?? 0) == layerCountBeforeDup
            let compactJSON = try compileCompactHostKeymap(profileId: codex.id.text)
            let compactValid = compactJSON.utf8.count < 1024 && compactJSON.contains("KV_OAI_AG00") && compactJSON.contains("VENDOR")
            checks["protected_oai_layer_locks_ui_and_lighting"] =
                protectedSelected
                && saveRejected
                && settingsRejected
                && lightingBlocked
                && exportLayerBlocked
                && duplicateBlocked
                && compactValid

            await setHostActiveProfile(cindy.id.text)
            let cindyYieldsToNative = nativeOwnership.desiredOwner == "native" && !nativeOwnership.allowsExecution
            await setHostActiveProfile(codex.id.text)
            let codexYieldsToNative = nativeOwnership.desiredOwner == "native" && !nativeOwnership.allowsExecution
            await setHostActiveProfile(zcode.id.text)
            let zcodeRequestsTakeover = nativeOwnership.desiredOwner == "micro-input" && !nativeOwnership.allowsExecution
            await setHostActiveProfile(dsh.id.text)
            let dshRequestsTakeover = nativeOwnership.desiredOwner == "micro-input" && !nativeOwnership.allowsExecution
            await setHostActiveProfile(antigravity.id.text)
            let agRequestsTakeover = nativeOwnership.desiredOwner == "micro-input" && !nativeOwnership.allowsExecution
            var reclaimReq = nativeOwnership.beginRequest(microDrained: true)
            nativeOwnership.receive(reply(reclaimReq, state: "released"), for: reclaimReq)
            if !nativeOwnership.allowsExecution {
                reclaimReq = nativeOwnership.beginRequest(microDrained: true)
                nativeOwnership.receive(reply(reclaimReq, state: "released"), for: reclaimReq)
            }
            let reclaimed = nativeOwnership.allowsExecution && executor.receipts.count == 3
            nativeOwnership.invalidate("scenario complete")
            checks["cindy_codex_to_workbench_handoff"] =
                cindyYieldsToNative
                && codexYieldsToNative
                && zcodeRequestsTakeover
                && dshRequestsTakeover
                && agRequestsTakeover
                && reclaimed
        } catch { status = readableError(error); checks["scenario_completed"] = false }
        var payload: [String: Any] = ["status": checks.count == 9 && checks.values.allSatisfy({ $0 }) ? "PASS" : "FAIL",
            "checks": checks, "receipt_count": executor.receipts.count,
            "native_app": "NOT_RUN", "physical_key": "NOT_RUN", "lighting_visual": "NOT_RUN"]
        if peerPath != nil {
            let expected = ["native-wide": 1, "native-encoder": 1, "released-wide": 1, "released-encoder": 1,
                            "stale-profile-encoder": 0, "fresh-encoder": 1, "lost-wide": 0, "lost-encoder": 0]
            let paired = pairedEvents.count == expected.count && pairedEvents.allSatisfy { event in
                guard let id = event["event_id"] as? String, let native = event["native_actions"] as? Int,
                      let micro = event["micro_actions"] as? Int else { return false }
                return expected[id] == native + micro
            }
            payload["paired_events"] = pairedEvents
            payload["paired_native_source"] = paired ? "PASS" : "FAIL"
            if !paired { payload["status"] = "FAIL" }
        }
        if let audit = ProcessInfo.processInfo.environment["MI_HOST_AUDIT_LOG"],
           let data = try? JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys, .prettyPrinted]) {
            try? data.write(to: URL(fileURLWithPath: audit).deletingLastPathComponent().appendingPathComponent("ownership.json"))
        }
    }
}
#endif
