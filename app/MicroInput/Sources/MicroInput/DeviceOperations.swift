import SwiftUI
import MicroInputCore

struct DeviceOperationCapabilities: Codable {
    var apply = false
    var restore = false
    var preview = false
    var reason = "原生设备变更能力关闭"
}
struct DeviceOperationState: Decodable, Identifiable {
    let operation_id: String?
    let phase: String
    let running: Bool?
    let can_retry_local: Bool?
    let local_saved: Bool?
    var restores: String? = nil
    var id: String { operation_id ?? "not-sent" }
    var title: String {
        switch phase {
        case "Running": return "正在执行并核对"
        case "Verified": return "配置文件已核对"
        case "VerifiedLocalSaveFailed": return "配置已核对 · 本地保存待修复"
        case "OriginalVerified": return "读回仍为原配置"
        case "NotSent": return "未发送设备变更"
        case "DeviceStateUnknown": return "设备状态未知 · 请先核对"
        default: return "操作待核对"
        }
    }
}

extension Store {
    func refreshOperationCapabilities() {
        operationCapabilities = (try? JSONDecoder().decode(DeviceOperationCapabilities.self, from: Data(backend.deviceOperationCapabilities().utf8))) ?? DeviceOperationCapabilities()
    }

    func activeDesktopAgentAdapter(for profileId: String? = nil) -> String? {
        let targetProfileID = (profileId?.isEmpty == false) ? profileId : hostActiveProfileId
        if let targetProfileID,
           let matchedProfile = hostProfiles.first(where: { $0.id.text == targetProfileID }),
           let adapter = DesktopAgent.adapterForName(matchedProfile.name) {
            return adapter
        }
        if let idx = navigationHistory.index, navigationHistory.entries.indices.contains(idx) {
            for place in navigationHistory.entries[0...idx].reversed() {
                if case .app(let bundleID) = place,
                   let match = DesktopAgentControls.bundles.first(where: { $0.value == bundleID }) {
                    return match.key
                }
            }
        }
        return nil
    }

    @discardableResult
    func executeHostAction(actionJSON: String, bindingId: String, profileId: String, t3Selection: T3Selection? = nil, stillAdmitted: (() -> Bool)? = nil) async -> HostActionReceipt? {
        guard nativeOwnership.allowsExecution, !hostOperationPaused, let lease = try? backend.beginHostExecution() else { return nil }
        let epoch = hostRouteEpoch
        inFlightHostActions += 1
        defer { inFlightHostActions -= 1; backend.finishHostExecution(lease: lease) }
        let receipt: HostActionReceipt
        var effectiveJSON = actionJSON
        var payload = try? JSONDecoder().decode(ResolvedHostActionPayload.self, from: Data(actionJSON.utf8))
        if let decoded = payload,
           decoded.action_type == "agent_command",
           decoded.adapter == "host",
           let cmd = decoded.command,
           (cmd == "submit" || cmd == "focus-input" || cmd.hasPrefix("prompt:") || cmd.hasPrefix("submit-prompt:")),
           let resolvedAdapter = activeDesktopAgentAdapter(for: profileId) {
            let rewritten = ResolvedHostActionPayload(
                action_type: decoded.action_type,
                key: decoded.key,
                key_code: decoded.key_code,
                modifiers: decoded.modifiers,
                target_bundle_id: DesktopAgentControls.bundles[resolvedAdapter] ?? decoded.target_bundle_id,
                text: decoded.text,
                bundle_id: decoded.bundle_id,
                url: decoded.url,
                working_directory: decoded.working_directory,
                adapter: resolvedAdapter,
                command: decoded.command,
                session: decoded.session,
                events: decoded.events,
                no_newline: decoded.no_newline,
                submit: decoded.submit
            )
            if let encoded = try? JSONEncoder().encode(rewritten) {
                effectiveJSON = String(decoding: encoded, as: UTF8.self)
                payload = rewritten
            }
        }
        if payload?.action_type == "agent_command",
           let command = payload?.command,
           ["back", "navigate-back", "forward", "navigate-forward"].contains(command),
           let adapter = payload?.adapter,
           ["navigation", "host", "cindy", "zcode", "qwenwork", "codex", "antigravity", "dsh-desktop", "qoder-cn", "herdr", "t3"].contains(adapter) {
            let isBack = command == "back" || command == "navigate-back"
            let destination = isBack
                ? navigationHistory.back(available: { [weak self] place in self?.isNavigationPlaceAvailable(place) == true })
                : navigationHistory.forward(available: { [weak self] place in self?.isNavigationPlaceAvailable(place) == true })
            receipt = await replayNavigationPlace(destination, isBack: isBack, bindingId: bindingId, profileId: profileId, epoch: epoch, stillAdmitted: stillAdmitted)
        } else if payload?.action_type == "agent_command", payload?.adapter == "t3", payload?.command == "next-attention" {
            let result = await t3Attention.open(t3Selection, admitted: { [weak self] in
                guard let self else { return false }
                return epoch == self.hostRouteEpoch && self.dispatchOpen && (stillAdmitted?() ?? false)
            })
            if result == .delivered, let identity = t3Selection?.event.identity {
                navigationHistory.visit(.t3Thread(environmentID: identity.environmentID, threadID: identity.threadID))
            }
            receipt = HostActionReceipt(timestampMs: UInt64(Date().timeIntervalSince1970 * 1000), profileId: profileId,
                bindingId: bindingId, actionType: "t3_next_attention", target: "t3", textByteLen: nil,
                outcome: result.rawValue, detail: result.label)
            AuditLogWriter.append(receipt)
        } else {
            receipt = await actionExecutor.execute(actionJSON: effectiveJSON, bindingId: bindingId, profileId: profileId)
            if receipt.succeeded, let payload {
                recordNavigationVisitIfApplicable(payload)
            }
        }
        guard !Task.isCancelled, epoch == hostRouteEpoch, !hostOperationPaused else { return nil }
        lastHostActionReceipt = receipt
        return receipt
    }

    private func recordNavigationVisitIfApplicable(_ payload: ResolvedHostActionPayload) {
        switch payload.action_type {
        case "open_app":
            if let bundleID = payload.bundle_id, !bundleID.isEmpty {
                navigationHistory.visit(.app(bundleID: bundleID))
            }
        case "paste_text":
            if let targetBundle = payload.target_bundle_id, !targetBundle.isEmpty, targetBundle != "frontmost-editor" {
                navigationHistory.visit(.app(bundleID: targetBundle))
            } else if let adapter = payload.adapter, let bundleID = DesktopAgentControls.bundles[adapter] {
                navigationHistory.visit(.app(bundleID: bundleID))
            }
        case "focus_session":
            if let adapter = payload.adapter, let session = payload.session,
               !adapter.isEmpty, !session.isEmpty, session != "unselected" {
                navigationHistory.visit(.session(adapter: adapter, session: session))
            }
        case "agent_command":
            guard let adapter = payload.adapter, let cmd = payload.command else { return }
            if let bundleID = DesktopAgentControls.bundles[adapter], DialScrollRequest.from(command: cmd) == nil {
                navigationHistory.visit(.app(bundleID: bundleID))
            } else if adapter == "herdr" {
                navigationHistory.visit(.app(bundleID: "dev.herdr.app"))
            }
        case "shortcut":
            if let targetBundle = payload.target_bundle_id, !targetBundle.isEmpty, targetBundle != "frontmost-editor" {
                navigationHistory.visit(.app(bundleID: targetBundle))
            }
        default:
            break
        }
    }

    private func replayNavigationPlace(
        _ destination: KeypadNavigationPlace?,
        isBack: Bool,
        bindingId: String,
        profileId: String,
        epoch: Int,
        stillAdmitted: (() -> Bool)?
    ) async -> HostActionReceipt {
        let actionType = isBack ? "navigate_back" : "navigate_forward"
        guard let destination else {
            let receipt = HostActionReceipt(
                timestampMs: UInt64(Date().timeIntervalSince1970 * 1000),
                profileId: profileId,
                bindingId: bindingId,
                actionType: actionType,
                target: "navigation",
                textByteLen: nil,
                outcome: "blocked",
                detail: isBack ? "没有可后退的打开历史" : "没有可前进的打开历史"
            )
            AuditLogWriter.append(receipt)
            return receipt
        }
        switch destination {
        case .app(let bundleID):
            if let data = try? JSONSerialization.data(withJSONObject: ["action_type": "open_app", "bundle_id": bundleID]),
               let replayJSON = String(data: data, encoding: .utf8) {
                return await actionExecutor.execute(actionJSON: replayJSON, bindingId: bindingId, profileId: profileId)
            }
        case .session(let adapter, let session):
            if let data = try? JSONSerialization.data(withJSONObject: ["action_type": "focus_session", "adapter": adapter, "session": session]),
               let replayJSON = String(data: data, encoding: .utf8) {
                return await actionExecutor.execute(actionJSON: replayJSON, bindingId: bindingId, profileId: profileId)
            }
        case .t3Thread(let environmentID, let threadID):
            let identity = T3ThreadIdentity(environmentID: environmentID, threadID: threadID)
            let delivered = await t3Attention.navigate(to: identity, admitted: { [weak self] in
                guard let self else { return false }
                return epoch == self.hostRouteEpoch && self.dispatchOpen && (stillAdmitted?() ?? true)
            })
            let receipt = HostActionReceipt(
                timestampMs: UInt64(Date().timeIntervalSince1970 * 1000),
                profileId: profileId,
                bindingId: bindingId,
                actionType: actionType,
                target: "t3",
                textByteLen: nil,
                outcome: delivered ? "delivered" : "failed",
                detail: delivered ? (isBack ? "已后退到 T3 会话" : "已前进到 T3 会话") : "T3 历史目标不可达"
            )
            AuditLogWriter.append(receipt)
            return receipt
        }
        let receipt = HostActionReceipt(
            timestampMs: UInt64(Date().timeIntervalSince1970 * 1000),
            profileId: profileId,
            bindingId: bindingId,
            actionType: actionType,
            target: destination.receiptTarget,
            textByteLen: nil,
            outcome: "blocked",
            detail: "历史目标编码失败"
        )
        AuditLogWriter.append(receipt)
        return receipt
    }
    func refreshDeviceOperations() async {
        refreshOperationCapabilities()
        do { operationRows = try JSONDecoder().decode([DeviceOperationState].self, from: Data(try await backend.deviceOperations().utf8)) }
        catch { status = readableError(error) }
    }
    func performDeviceOperation(token: String, restore: Bool = false) async {
        guard nativeOwnership.allowsExecution else { status = nativeOwnership.summary; return }
        guard !deviceOperationRunning else { return }
        guard lightingPreviewToken == nil else { status = "请先结束灯光预览"; return }
        refreshOperationCapabilities()
        guard restore ? operationCapabilities.restore : operationCapabilities.apply else { status = operationCapabilities.reason; return }
        deviceOperationRunning = true
        deviceCancellationRequested = false
        hostOperationPaused = true
        cancelHostExecution()
        hostRouteEpoch += 1
        hostInputTask?.cancel(); hostInputTask = nil
        let connection = generation, document = draft.document_id
        var drain: String?
        defer { deviceOperationRunning = false }
        do {
            let ticket = try backend.beginDeviceDrain(); drain = ticket
            let deadline = Date().addingTimeInterval(drainTimeout)
            while inFlightHostActions > 0 || backend.hostExecutionsPending() > 0 {
                guard Date() < deadline, connection == generation, !Task.isCancelled, !deviceCancellationRequested else {
                    backend.abandonDeviceDrain(token: ticket); drain = nil
                    operationResult = DeviceOperationState(operation_id: nil, phase: "NotSent", running: false, can_retry_local: false, local_saved: false)
                    status = "宿主动作尚未排空 · 未发送设备变更"
                    return
                }
                try await Task.sleep(nanoseconds: 10_000_000)
            }
            guard pushToTalk.cleanupConfirmed else {
                backend.abandonDeviceDrain(token: ticket); drain = nil
                operationResult = DeviceOperationState(operation_id: nil, phase: "NotSent", running: false, can_retry_local: false, local_saved: false)
                hostOperationPaused = false
                readInputBoundary()
                if hostExecutionEnabled { startHostInputLoop(token: generation) }
                status = "原听写停止未确认；未发送设备变更"
                return
            }
            guard connection == generation, !Task.isCancelled, !deviceCancellationRequested else { throw BridgeError.Disconnected }
            let id = try await backend.startDeviceApply(token: token, drainToken: ticket, restore: restore)
            drain = nil
            if restore { restorePreview = nil }
            operationResult = DeviceOperationState(operation_id: id, phase: "Running", running: true, can_retry_local: false, local_saved: false)
            let until = Date().addingTimeInterval(125)
            while true {
                let result = try JSONDecoder().decode(DeviceOperationState.self, from: Data(try backend.deviceOperationStatus(operationId: id).utf8))
                operationResult = result
                if result.running != true { break }
                if Task.isCancelled || deviceCancellationRequested || Date() > until { backend.cancelDeviceOperation(operationId: id) }
                if Date() > until.addingTimeInterval(5) { status = "操作仍待核对 · 已请求停止"; return }
                // Cancellation stops the Rust operation; keep observing its bounded terminal result.
                await Task.detached { try? await Task.sleep(nanoseconds: 50_000_000) }.value
            }
            if draft.document_id == document { try applyDocument(try await backend.documentView()) }
            await refreshDeviceOperations()
            status = operationResult?.title ?? "操作结束"
            if connection == generation, ["Verified", "NotSent"].contains(operationResult?.phase ?? "") {
                hostOperationPaused = false
                refreshPassthroughHealth()
                if hostExecutionEnabled { startHostInputLoop(token: generation) }
            }
            recordDeviceOperationEvidence()
        } catch {
            if let drain { backend.abandonDeviceDrain(token: drain) }
            status = readableError(error)
        }
    }
    func cancelActiveDeviceOperation() {
        deviceCancellationRequested = true
        if let id = operationResult?.operation_id, deviceOperationRunning { backend.cancelDeviceOperation(operationId: id) }
    }
    func discardDeviceRestore() { backend.discardDeviceRestore(); restorePreview = nil }
    func prepareDeviceRestore(_ id: String) async {
        guard !deviceOperationRunning else { return }
        deviceOperationRunning = true
        defer { deviceOperationRunning = false }
        do { restorePreview = try JSONDecoder().decode(ApplyPreview.self, from: Data(try await backend.prepareDeviceRestore(operationId: id).utf8)) }
        catch { status = readableError(error) }
    }
    func checkDeviceOperation(_ id: String) async {
        guard !deviceOperationRunning else { return }
        let connection = generation
        deviceOperationRunning = true
        defer { deviceOperationRunning = false }
        do {
            operationResult = try JSONDecoder().decode(DeviceOperationState.self, from: Data(try await backend.checkDeviceOperation(operationId: id).utf8))
            try applyDocument(try await backend.documentView())
            await refreshDeviceOperations(); status = operationResult?.title ?? "核对完成"
            if connection == generation, ["Verified", "OriginalVerified", "VerifiedLocalSaveFailed"].contains(operationResult?.phase ?? "") {
                hostOperationPaused = false
                refreshPassthroughHealth()
                if hostExecutionEnabled { startHostInputLoop(token: generation) }
            }
        } catch { status = readableError(error) }
    }
    func repairLocalOperation(_ id: String) async {
        guard !deviceOperationRunning else { return }
        deviceOperationRunning = true
        defer { deviceOperationRunning = false }
        do { try applyDocument(try await backend.retryLocalSave(operationId: id)); await refreshDeviceOperations(); status = "本地保存已修复 · 没有发送设备请求" }
        catch { status = readableError(error) }
    }
    func recordDeviceOperationEvidence() {
        guard let result = operationResult,
              let audit = ProcessInfo.processInfo.environment["MI_HOST_AUDIT_LOG"],
              let data = try? JSONSerialization.data(withJSONObject: ["operation_id": result.id, "phase": result.phase, "config_readback": result.phase == "Verified" ? "PASS" : "NOT_VERIFIED", "physical_key": "NOT_RUN", "lighting_visual": "NOT_RUN"], options: [.sortedKeys]) else { return }
        let url = URL(fileURLWithPath: audit).deletingLastPathComponent().appendingPathComponent("device-operations.jsonl")
        if !FileManager.default.fileExists(atPath: url.path) { FileManager.default.createFile(atPath: url.path, contents: nil) }
        if let handle = try? FileHandle(forWritingTo: url) { defer { try? handle.close() }; _ = try? handle.seekToEnd(); try? handle.write(contentsOf: data + Data([10])) }
    }
    #if MI_SYNTHETIC_BACKEND
    func runSyntheticDeviceOperationScenario() async {
        guard let core = backend as? MicroInputCore.MicroInput else { return }
        do {
            await installCodingAgentPresets()
            await setHostExecutionEnabled(true)
            let hostID = draft.document_id
            let hostBytes = try Data(contentsOf: documentURL)
            let preferenceURL = documentURL.deletingPathExtension().appendingPathExtension("preferences.json")
            let preferences = try Data(contentsOf: preferenceURL)
            let originalRoute = try JSONDecoder().decode(HostRoutingState.self, from: Data(core.hostRoutingState().utf8))
            func resolves(_ binding: String) -> Bool {
                core.resolveHostInput(bindingId: binding, expectedDocumentId: originalRoute.document_id, expectedDocumentRevision: originalRoute.document_revision, expectedHostStateRevision: originalRoute.host_state_revision) != nil
            }
            try await core.setSyntheticDiscovery(state: .twoDevices)
            await refresh(); selectDevice("synthetic-interface-0")
            await editDeviceConfiguration()
            guard editingDevice, hostProfiles.contains(where: { $0.id.text == originalRoute.profile_id }), resolves("AG00") else { throw BridgeError.Failed(message: "设备编辑改变宿主路由") }
            slot = 0; await assign("device-key")
            slot = 1; await assign("device-key")
            await editLighting(zone: "backlight", field: "color", value: "#3377CC", allLayers: false)
            await previewApply()
            guard applyPreview?.changes?.count == 3 else { throw BridgeError.Failed(message: "混合变更摘要不完整") }
            await prepareApplyBackup()
            guard let token = applyPreview?.token else { throw BridgeError.Failed(message: status) }
            await performDeviceOperation(token: token)
            guard operationResult?.phase == "Verified", let id = operationResult?.operation_id,
                  !resolves("AG00"), !resolves("AG01"), resolves("AG02") else { throw BridgeError.Failed(message: "应用或逐绑定路由核对失败") }
            await prepareDeviceRestore(id)
            guard let restore = restorePreview else { throw BridgeError.Failed(message: status) }
            await performDeviceOperation(token: restore.token, restore: true)
            guard operationResult?.phase == "Verified", resolves("AG00") else { throw BridgeError.Failed(message: "恢复未核验或原绑定未恢复") }
            await returnToHostDocument()
            guard !editingDevice, draft.document_id == hostID,
                  try Data(contentsOf: documentURL) == hostBytes,
                  try Data(contentsOf: preferenceURL) == preferences else { throw BridgeError.Failed(message: "宿主草稿或偏好发生变化") }
            if let audit = ProcessInfo.processInfo.environment["MI_HOST_AUDIT_LOG"] {
                let payload = ["status":"PASS", "host_document":"UNCHANGED", "host_preferences":"UNCHANGED", "mixed_changes":"3", "routing":"PASS"]
                try JSONEncoder().encode(payload).write(to: URL(fileURLWithPath: audit).deletingLastPathComponent().appendingPathComponent("device-editor.json"))
            }
            showingDeviceOperations = true
        } catch { status = readableError(error) }
    }

    #endif
}

struct DeviceOperationsView: View {
    @ObservedObject var store: Store
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack { Text("设备操作与恢复").font(.title2); Spacer(); Button("关闭") { store.showingDeviceOperations = false } }
            Text("支持已核验的 Creator Micro 2 / 0.6.3 当前连接。配置读回与实体按键、灯光效果分别核验。")
            if let result = store.operationResult { Text(result.title).font(.headline) }
            if store.deviceOperationRunning { Button("停止后续发送") { store.cancelActiveDeviceOperation() } }
            ScrollView {
                ForEach(store.operationRows) { row in
                    VStack(alignment: .leading, spacing: 8) {
                        Text("\(row.restores == nil ? "应用配置" : "恢复配置") · \(row.title)").font(.headline)
                        Text(row.id).font(.caption.monospaced()).textSelection(.enabled)
                        HStack {
                            Button("只读核对当前设备") { Task { await store.checkDeviceOperation(row.id) } }.disabled(!store.operationCapabilities.restore)
                            Button("审阅恢复原配置…") { Task { await store.prepareDeviceRestore(row.id) } }.disabled(!store.operationCapabilities.restore)
                            if row.can_retry_local == true { Button("仅修复本地保存") { Task { await store.repairLocalOperation(row.id) } } }
                        }.disabled(store.deviceOperationRunning)
                    }.padding(.vertical, 8)
                }
            }
            if let preview = store.restorePreview {
                Divider()
                Text("恢复审阅").font(.headline)
                Text("将当前 keymap 替换为关联原副本；保留当前 Smart Actions。执行前再次核对现状，变化后需重新审阅。")
                Text("目标摘要：\(preview.planned_keymap_sha256)").font(.caption.monospaced())
                HStack {
                    Button("确认恢复") { Task { await store.performDeviceOperation(token: preview.token, restore: true); store.restorePreview = nil } }.disabled(store.deviceOperationRunning || !store.operationCapabilities.restore)
                    Button("取消恢复准备") { store.discardDeviceRestore() }.disabled(store.deviceOperationRunning)
                }
            }
            Text(store.status).font(.caption)
        }.padding(24).frame(width: 720, height: 540).task { await store.refreshDeviceOperations() }
    }
}
