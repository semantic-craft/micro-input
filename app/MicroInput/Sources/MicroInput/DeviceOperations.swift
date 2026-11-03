import SwiftUI
import MicroInputCore

struct DeviceOperationCapabilities: Decodable {
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
    @discardableResult
    func executeHostAction(actionJSON: String, bindingId: String, profileId: String) async -> HostActionReceipt? {
        guard !hostOperationPaused, let lease = try? backend.beginHostExecution() else { return nil }
        let epoch = hostRouteEpoch
        inFlightHostActions += 1
        defer { inFlightHostActions -= 1; backend.finishHostExecution(lease: lease) }
        let receipt = await actionExecutor.execute(actionJSON: actionJSON, bindingId: bindingId, profileId: profileId)
        guard epoch == hostRouteEpoch, !hostOperationPaused else { return nil }
        lastHostActionReceipt = receipt
        return receipt
    }
    func refreshDeviceOperations() async {
        refreshOperationCapabilities()
        do { operationRows = try JSONDecoder().decode([DeviceOperationState].self, from: Data(try await backend.deviceOperations().utf8)) }
        catch { status = readableError(error) }
    }
    func performDeviceOperation(token: String, restore: Bool = false) async {
        guard !deviceOperationRunning else { return }
        guard lightingPreviewToken == nil else { status = "请先结束灯光预览并确认恢复"; return }
        refreshOperationCapabilities()
        guard restore ? operationCapabilities.restore : operationCapabilities.apply else { status = operationCapabilities.reason; return }
        deviceOperationRunning = true
        deviceCancellationRequested = false
        hostOperationPaused = true
        clearPendingActions()
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
        do { try await core.setSyntheticDiscovery(state: .twoDevices) } catch { status = readableError(error); return }
        await refresh(); selectDevice("synthetic-interface-0"); await connect("synthetic-interface-0")
        await readIdentity(); await readDeviceConfig(); await createDraftFromSnapshot()
        await assign("device-key"); await previewApply(); await prepareApplyBackup()
        guard let token = applyPreview?.token else { return }
        await performDeviceOperation(token: token)
        guard operationResult?.phase == "Verified", let id = operationResult?.operation_id else { return }
        await prepareDeviceRestore(id)
        guard let restore = restorePreview else { return }
        await performDeviceOperation(token: restore.token, restore: true)
        showingDeviceOperations = true
    }
    #endif
}

struct DeviceOperationsView: View {
    @ObservedObject var store: Store
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack { Text("设备操作与恢复").font(.title2); Spacer(); Button("关闭") { store.showingDeviceOperations = false } }
            Text("配置文件核对与实体按键、灯光效果验收分别记录。当前只开放合成设备。")
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
