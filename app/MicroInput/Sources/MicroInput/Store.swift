// R-A02/R-N01: navigation has no transport dependency or device side effect.
import Foundation
import MicroInputCore
protocol Backend {
    func hostRoutingState() -> String
    func hostInputWatermark() -> String
    func updateHostRouting(requestJson: String) async throws -> String
    func resolveHostInput(bindingId: String, expectedDocumentId: String, expectedDocumentRevision: UInt64, expectedHostStateRevision: UInt64) -> String?

    func loadDemo() async throws -> String
    func listDevices() async throws -> [DeviceInfo]
    func connect(id: String) async throws
    func readIdentity() async throws -> IdentityInfo
    func readConfig() async throws -> String
    func disconnect() async
    func waitForDisconnect() async -> String
    func shutdown() async
    func inputMonitoringStatus() async -> Bool
    func subscribeDeviceEvents(cb: DeviceEvents) async
    func subscribeInput() async throws -> InputSubscription
    func openDocument(path: String) async throws -> String
    func inspectAgentProposal(text: String) async throws -> String
    func rejectAgentProposal() async
    func acceptAgentProposal(token: String) async throws -> String
    func recoveryFiles() async throws -> [String]
    func inspectRecovery(path: String) async throws -> String
    func recoverDocument(token: String) async throws -> String
    func saveDocumentAs() async throws -> String
    func copyRecoveryOriginal(token: String, destination: String) async throws
    func actionCatalog() async throws -> String
    func actionLibrary(query: String, category: String, favoritesOnly: Bool) async throws -> String
    func setActionFavorite(actionId: String, favorite: Bool) async throws
    func assignAction(profileId: String, layerId: String, slot: UInt32, actionJson: String) async throws -> String
    func exportPreset(profileId: String?, layerId: String?) async throws -> String
    func inspectPreset(text: String) throws -> String
    func starterPreset() throws -> String
    func importPreset(text: String) async throws -> String
    func setControlSettings(profileId: String, layerId: String, settingsJson: String) async throws -> String
    func saveMacroAction(macroId: String?, profileId: String, layerId: String, slot: UInt32, actionJson: String) async throws -> String
    func bindMacroAction(macroId: String?, profileId: String, layerId: String, slot: UInt32) async throws -> String
    func deleteMacroAction(macroId: String) async throws -> String
    func saveMultiAction(multiId: String?, profileId: String, layerId: String, slot: UInt32, actionJson: String, additionsJson: String) async throws -> String
    func bindMultiAction(multiId: String?, profileId: String, layerId: String, slot: UInt32) async throws -> String
    func deleteMultiAction(multiId: String) async throws -> String
    func undoDraft(redo: Bool) async throws -> String
    func documentView() async throws -> String
    func createProfile(name: String) async throws -> String
    func renameProfile(profileId: String, name: String) async throws -> String
    func duplicateProfile(profileId: String, name: String) async throws -> String
    func deleteProfile(profileId: String) async throws -> String
    func moveProfile(profileId: String, toIndex: UInt32) async throws -> String
    func createLayer(profileId: String, name: String) async throws -> String
    func renameLayer(layerId: String, name: String) async throws -> String
    func duplicateLayer(profileId: String, layerId: String, name: String) async throws -> String
    func deleteLayer(profileId: String, layerId: String) async throws -> String
    func moveLayer(profileId: String, layerId: String, toIndex: UInt32) async throws -> String
    func createDraftFromSnapshot() async throws -> String
    func previewApply() async throws -> String
    func cancelApplyPreview() async
    func prepareApplyBackup(token: String) async throws -> String
    func compareSnapshot() async throws -> String
    func confirmComparisonIdentity(token: String) async throws -> String
    func resolveComparison(token: String, choice: String) async throws -> String
    func subscribeVerifiedInput() async throws -> InputSubscription
    func setHostActiveProfile(profileId: String?) async throws -> String
    func setHostExecutionEnabled(enabled: Bool) async throws -> String
    func checkPassthroughHealth() -> String
    func resolveInputAction(bindingId: String, isPressed: Bool) -> String?
    func installCodingAgentPresets() async throws -> String
}
extension Backend {
    func hostRoutingState() -> String { "{}" }
    func hostInputWatermark() -> String { #"{"session_id":null,"ingress_sequence":0}"# }
    func updateHostRouting(requestJson: String) async throws -> String { throw BridgeError.Unavailable }
    func resolveHostInput(bindingId: String, expectedDocumentId: String, expectedDocumentRevision: UInt64, expectedHostStateRevision: UInt64) -> String? { nil }

    func inspectAgentProposal(text: String) async throws -> String { throw BridgeError.Failed(message: "Agent 提案不可用") }
    func rejectAgentProposal() async {}
    func acceptAgentProposal(token: String) async throws -> String { throw BridgeError.Failed(message: "Agent 提案不可用") }
    func recoveryFiles() async throws -> [String] { throw BridgeError.Failed(message: "文档恢复不可用") }
    func inspectRecovery(path: String) async throws -> String { throw BridgeError.Failed(message: "文档恢复不可用") }
    func recoverDocument(token: String) async throws -> String { throw BridgeError.Failed(message: "文档恢复不可用") }
    func saveDocumentAs() async throws -> String { throw BridgeError.Failed(message: "文档另存不可用") }
    func copyRecoveryOriginal(token: String, destination: String) async throws { throw BridgeError.Failed(message: "原件副本保存不可用") }
    func exportPreset(profileId: String?, layerId: String?) async throws -> String { throw BridgeError.Failed(message: "预设导出不可用") }
    func inspectPreset(text: String) throws -> String { throw BridgeError.Failed(message: "预设检查不可用") }
    func starterPreset() throws -> String { throw BridgeError.Failed(message: "原创预设不可用") }
    func importPreset(text: String) async throws -> String { throw BridgeError.Failed(message: "预设导入不可用") }

    func waitForDisconnect() async -> String { "" }
    func shutdown() async { await disconnect() }
    func subscribeInput() async throws -> InputSubscription { throw BridgeError.Failed(message: "输入监听不可用") }
    func subscribeVerifiedInput() async throws -> InputSubscription { throw BridgeError.Failed(message: "已验证输入监听不可用") }
    func setHostActiveProfile(profileId: String?) async throws -> String { throw BridgeError.Failed(message: "宿主激活 Profile 不可用") }
    func setHostExecutionEnabled(enabled: Bool) async throws -> String { throw BridgeError.Failed(message: "宿主执行开关不可用") }
    func checkPassthroughHealth() -> String { #"{"status":"disconnected"}"# }
    func resolveInputAction(bindingId: String, isPressed: Bool) -> String? { nil }
    func installCodingAgentPresets() async throws -> String { throw BridgeError.Failed(message: "Coding Agent 预设不可用") }
    func readIdentity() async throws -> IdentityInfo { throw BridgeError.Failed(message: "身份读取不可用") }
    func openDocument(path: String) async throws -> String { throw BridgeError.Failed(message: "本地文档不可用") }
    func actionCatalog() async throws -> String { throw BridgeError.Failed(message: "动作目录不可用") }
    func actionLibrary(query: String, category: String, favoritesOnly: Bool) async throws -> String { throw BridgeError.Failed(message: "动作库不可用") }
    func setActionFavorite(actionId: String, favorite: Bool) async throws { throw BridgeError.Failed(message: "收藏不可用") }
    func assignAction(profileId: String, layerId: String, slot: UInt32, actionJson: String) async throws -> String { throw BridgeError.Failed(message: "草稿编辑不可用") }
    func setControlSettings(profileId: String, layerId: String, settingsJson: String) async throws -> String { throw BridgeError.Failed(message: "控件设置不可用") }
    func saveMacroAction(macroId: String?, profileId: String, layerId: String, slot: UInt32, actionJson: String) async throws -> String { throw BridgeError.Failed(message: "本地宏不可用") }
    func bindMacroAction(macroId: String?, profileId: String, layerId: String, slot: UInt32) async throws -> String { throw BridgeError.Failed(message: "本地宏不可用") }
    func deleteMacroAction(macroId: String) async throws -> String { throw BridgeError.Failed(message: "本地宏不可用") }
    func saveMultiAction(multiId: String?, profileId: String, layerId: String, slot: UInt32, actionJson: String, additionsJson: String) async throws -> String { throw BridgeError.Failed(message: "本地 Multi 不可用") }
    func bindMultiAction(multiId: String?, profileId: String, layerId: String, slot: UInt32) async throws -> String { throw BridgeError.Failed(message: "本地 Multi 不可用") }
    func deleteMultiAction(multiId: String) async throws -> String { throw BridgeError.Failed(message: "本地 Multi 不可用") }
    func undoDraft(redo: Bool) async throws -> String { throw BridgeError.Failed(message: "草稿撤销不可用") }

    func documentView() async throws -> String { throw BridgeError.Failed(message: "本地文档功能不可用") }
    func createProfile(name: String) async throws -> String { throw BridgeError.Failed(message: "本地文档功能不可用") }
    func renameProfile(profileId: String, name: String) async throws -> String { throw BridgeError.Failed(message: "本地文档功能不可用") }
    func duplicateProfile(profileId: String, name: String) async throws -> String { throw BridgeError.Failed(message: "本地文档功能不可用") }
    func deleteProfile(profileId: String) async throws -> String { throw BridgeError.Failed(message: "本地文档功能不可用") }
    func moveProfile(profileId: String, toIndex: UInt32) async throws -> String { throw BridgeError.Failed(message: "本地文档功能不可用") }
    func createLayer(profileId: String, name: String) async throws -> String { throw BridgeError.Failed(message: "本地文档功能不可用") }
    func renameLayer(layerId: String, name: String) async throws -> String { throw BridgeError.Failed(message: "本地文档功能不可用") }
    func duplicateLayer(profileId: String, layerId: String, name: String) async throws -> String { throw BridgeError.Failed(message: "本地文档功能不可用") }
    func deleteLayer(profileId: String, layerId: String) async throws -> String { throw BridgeError.Failed(message: "本地文档功能不可用") }
    func moveLayer(profileId: String, layerId: String, toIndex: UInt32) async throws -> String { throw BridgeError.Failed(message: "本地文档功能不可用") }
    func createDraftFromSnapshot() async throws -> String { throw BridgeError.Failed(message: "本地文档功能不可用") }
    func previewApply() async throws -> String { throw BridgeError.Failed(message:"设备应用准备不可用") }
    func cancelApplyPreview() async {}
    func prepareApplyBackup(token: String) async throws -> String { throw BridgeError.Failed(message:"设备备份不可用") }
    func confirmComparisonIdentity(token:String) async throws -> String {throw BridgeError.Failed(message:"设备核对不可用")}
    func compareSnapshot() async throws -> String { throw BridgeError.Failed(message:"重连比较不可用") }
    func resolveComparison(token: String, choice: String) async throws -> String { throw BridgeError.Failed(message:"重连比较不可用") }
    func subscribeDeviceEvents(cb: DeviceEvents) async {}
}
extension MicroInput: Backend {}
@MainActor final class Store: ObservableObject {
    func subscribeInput() async throws -> InputSubscription {
        guard !systemSleeping, !stopped, connectedID != nil else { throw BridgeError.Disconnected }
        return try await backend.subscribeInput()
    }
    func subscribeVerifiedInput() async throws -> InputSubscription {
        guard !systemSleeping, !stopped, connectedID != nil else { throw BridgeError.Disconnected }
        return try await backend.subscribeVerifiedInput()
    }
    let backend: Backend
    let actionExecutor: HostActionExecuting
    @Published var routing: HostRoutingState?
    @Published var inputNeedsResume = false
    var routingValid = false
    var hostMutations: [HostMutation] = []
    var mutationDraining = false
    var inputBoundary = HostInputBoundary()
    var pendingHostActions: [PendingHostAction] = []
    var hostExecutionTask: Task<Void, Never>?
    var overflowDraining = false
    var observationSequence: UInt64 = 0
    var externalBundleID: String?
    var foregroundTask: Task<Void, Never>?
    lazy var overlay = HostOverlay(store: self)
    @Published var hostActiveProfileId: String?
    @Published var hostExecutionEnabled = false
    @Published var passThroughHealth: PassThroughHealthView = .disconnected
    @Published var lastHostActionReceipt: HostActionReceipt?
    private var hostInputTask: Task<Void, Never>?
    @Published var draft = DraftState()
    @Published var macros: [MacroSummary] = []
    @Published var multis: [MacroSummary] = []
    @Published var graphActions: [String:CatalogAction] = [:]
    @Published var catalog: [CatalogAction] = []
    @Published var library = ActionLibraryState()
    @Published var actionCapabilities: [String:ActionCapability] = [:]
    private var libraryGeneration = 0
    private var libraryQuery = "", libraryCategory = "全部"
    private var libraryFavoritesOnly = false
    func searchActions(query: String, category: String, favoritesOnly: Bool) async {
        libraryGeneration += 1; let generation = libraryGeneration
        libraryQuery = query; libraryCategory = category; libraryFavoritesOnly = favoritesOnly
        do {
            let result = try await backend.actionLibrary(query: query, category: category, favoritesOnly: favoritesOnly)
            guard generation == libraryGeneration else { return }
            library = try JSONDecoder().decode(ActionLibraryState.self, from: Data(result.utf8))
        } catch { if generation == libraryGeneration { status = readableError(error) } }
    }
    func setFavorite(_ id: String, favorite: Bool) async {
        do {
            try await backend.setActionFavorite(actionId: id, favorite: favorite)
            await searchActions(query: libraryQuery, category: libraryCategory, favoritesOnly: libraryFavoritesOnly)
            status = "收藏已保存为本机偏好 · 草稿修订不变"
        } catch { status = readableError(error) }
    }
    @Published var controlSettings: [String:LocalControlSettings] = [:]
    @Published var controlSettingsError: String?
    var currentControlSettings: LocalControlSettings? { layer.flatMap { controlSettings[$0.id.text] } }
    @Published var slot = 0
    @Published var documentReady = false
    @Published private(set) var browsingDeviceSnapshot = false
    private var localSnapshot: Snapshot?
    private var localSelection: (profile: String?, layer: String?, slot: Int) = (nil, nil, 0)
    var canEditDraft: Bool { documentReady && !browsingDeviceSnapshot }
    func showLocalDraft() {
        guard !busy, let localSnapshot else { return }
        snapshot = localSnapshot; browsingDeviceSnapshot = false; stale = false
        restoreSelection(profile: localSelection.profile, layer: localSelection.layer, fallbackProfile: 0, fallbackLayer: 0); slot = localSelection.slot
        status = "本地草稿 · 修改自动保存 · 不写设备"
    }
    private var documentOverride: URL?
    var documentURL: URL {
        if let documentOverride { return documentOverride }
        if let directory = ProcessInfo.processInfo.environment["MI_DOCUMENT_DIR"] {
            return URL(fileURLWithPath: directory).appendingPathComponent("workbench.json")
        }
        return FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("MicroInput/workbench.json")
    }
    var autoHostReady = ProcessInfo.processInfo.environment["MI_AUTO_HOST_READY"] == "1"
    private var autoSetupInFlight = false
    func loadWorkbench() async {
        do {
            try await changeDocument(fresh: false) { [self] in try await backend.openDocument(path: documentURL.path) }
            if let index = snapshot?.profiles.firstIndex(where: {
                $0.id.text == hostActiveProfileId && DesktopAgent.named($0.name) != nil
            }) {
                selectProfile(index)
            }
            catalog = try JSONDecoder().decode([CatalogAction].self, from: Data(try await backend.actionCatalog().utf8))
            #if MI_SYNTHETIC_BACKEND
            let scenario = ProcessInfo.processInfo.environment["MI_SYNTHETIC_SCENARIO"] ?? ""
            if ["reconnect", "reconnect-transient", "apply-preparation"].contains(scenario), !preparedReconnectScenario {
                await prepareReconnectScenario()
            } else if scenario == "host-execution", !preparedHostExecutionScenario {
                await prepareHostExecutionScenario()
            }
            #endif
            if autoHostReady, (ProcessInfo.processInfo.environment["MI_SYNTHETIC_SCENARIO"] ?? "").isEmpty {
                await autoSetupHostExecutionIfNeeded()
            }
            await searchActions(query: "", category: "全部", favoritesOnly: false)
        } catch { status = readableError(error); documentReady = false }
    }
    private func restoreSelection(profile id: String?, layer lid: String?, fallbackProfile: Int, fallbackLayer: Int) {
        profileIndex = snapshot?.profiles.firstIndex(where: {$0.id.text == id}) ?? min(fallbackProfile, max(0, (snapshot?.profiles.count ?? 0)-1))
        layerIndex = profile?.layers.firstIndex(where: {$0.id.text == lid}) ?? min(fallbackLayer, max(0, (profile?.layers.count ?? 0)-1))
        slot = min(slot, max(0, (layer.map {$0.keys.count+$0.encoders.count+$0.joystick.sectors.count} ?? 0)-1))
    }
    var hostProfiles: [Profile] { (localSnapshot ?? snapshot)?.profiles ?? [] }
    private func applyDocument(_ json: String, fresh: Bool = false) throws {
        let result = try JSONDecoder().decode(DocumentView.self, from: Data(json.utf8))
        if draft.document_id != result.draft.document_id || draft.revision != result.draft.revision {applyPreview=nil;preparedApplyBackup=nil}
        let oldProfile = fresh ? nil : profile?.id.text, oldLayer = fresh ? nil : layer?.id.text
        let pi = fresh ? 0 : profileIndex, li = fresh ? 0 : layerIndex
        macros = result.macro_library ?? []
        multis = result.multi_library ?? []; graphActions = result.graph_actions ?? [:]
        draft = result.draft; snapshot = result.config; localSnapshot = result.config
        actionCapabilities = result.action_capabilities ?? [:]
        controlSettings = result.control_settings ?? [:]; controlSettingsError = nil
        hostActiveProfileId = result.host_active_profile_id
        if let enabled = result.host_execution_enabled { hostExecutionEnabled = enabled }
        try? refreshRouting()
        if connectedID != nil, hostInputTask == nil, !inputNeedsResume { startHostInputLoop(token: generation) }
        browsingDeviceSnapshot = false; stale = false; documentReady = !draft.read_only
        restoreSelection(profile: result.selection_hint?.profile_id ?? oldProfile, layer: result.selection_hint?.layer_id ?? oldLayer, fallbackProfile: pi, fallbackLayer: li)
        localSelection = (profile?.id.text, layer?.id.text, slot)
        status = draft.read_only ? "文档含未知数据 · 只读 · 原文件保留" : "本地草稿 · 修改自动保存 · 不写设备"
        if draft.loaded_format == 1 { status += " · 旧格式将在首次修改时转换并备份" }
    }
    func setHostActiveProfile(_ profileId: String?) async {
        guard let id = profileId ?? hostProfiles.first(where: { $0.name == "Antigravity" })?.id.text else { return }
        await routingChange(["kind":"select", "profile_id":id])
    }
    func setHostExecutionEnabled(_ enabled: Bool) async {
        await routingChange(["kind":"execution", "enabled":enabled])
        if connectedID != nil, hostInputTask == nil, !inputNeedsResume { startHostInputLoop(token: generation) }
    }
    func installCodingAgentPresets() async {
        guard canEditDraft, !busy else { return }
        busy = true; defer { busy = false }
        do {
            try await changeDocument(fresh: false) { [self] in try await backend.installCodingAgentPresets() }
            status = "已安装常用 Agent 配置 · 可在菜单栏选择当前控制目标"
        } catch { status = readableError(error) }
    }
    var orderedHostProfiles: [Profile] {
        let priorities = DesktopAgent.all.map(\.name)
        return hostProfiles.enumerated().sorted {
            let a = priorities.firstIndex(of: $0.element.name) ?? (priorities.count + $0.offset)
            let b = priorities.firstIndex(of: $1.element.name) ?? (priorities.count + $1.offset)
            return a < b
        }.map(\.element)
    }
    func prepareDesktopProfiles() async {
        guard canEditDraft, !busy else { return }
        let untouched = snapshot?.source == "Demo" && draft.revision == 0 && draft.edits.isEmpty && draft.structure_changes.isEmpty
        await installCodingAgentPresets()
        if untouched, let index = snapshot?.profiles.firstIndex(where: { $0.name == "Cindy" }) {
            selectProfile(index)
        }
    }
    var canTryDesktopAction: Bool {
        guard DesktopAgent.named(profile?.name ?? "") != nil, !browsingDeviceSnapshot,
              profile?.id.text == hostActiveProfileId, hostExecutionEnabled, (0...19).contains(slot) else { return false }
        let binding = Self.desktopBindings[slot]
        return !busy && backend.resolveInputAction(bindingId: binding, isPressed: true) != nil
    }
    private static let desktopBindings = ["AG00", "AG01", "AG02", "AG03", "AG04", "AG05", "AG06", "AG07", "AG08", "AG09", "AG10", "AG11", "AG12", "AG13", "AG14", "ENC_CLK", "AG15", "AG16", "AG17", "AG18"]
    func tryDesktopAction() async {
        guard dispatchOpen else { status = "正在切换或路由尚未就绪，请稍后重试"; return }
        guard canTryDesktopAction else { return }
        let binding = Self.desktopBindings[slot]
        guard let json = backend.resolveInputAction(bindingId: binding, isPressed: true),
              let payload = try? JSONDecoder().decode(ResolvedHostActionPayload.self, from: Data(json.utf8)),
              ["open_app", "shortcut", "agent_command"].contains(payload.action_type) else {
            status = "此按键未配置可试运行动作"; return
        }
        guard let (resolved, profile, state) = resolveHostAction(binding) else { return }
        await withCheckedContinuation { continuation in
            enqueueHostAction(json: resolved, binding: "ui-" + binding, profile: profile, state: state, done: { continuation.resume() })
        }
        status = lastHostActionReceipt?.detail ?? "没有执行回执"
    }
    func openDesktopAgent(_ agent: DesktopAgent) async {
        guard let state = routing, let data = try? JSONSerialization.data(withJSONObject: ["action_type": "open_app", "bundle_id": agent.bundleID]) else { return }
        await withCheckedContinuation { continuation in
            enqueueHostAction(json: String(decoding: data, as: UTF8.self), binding: "ui-open", profile: state.profile_id ?? "", state: state, requiresEnabled: false, done: { continuation.resume() })
        }
    }
    func autoSetupHostExecutionIfNeeded() async {
        guard !autoSetupInFlight, !systemSleeping, !stopped else { return }
        autoSetupInFlight = true
        defer { autoSetupInFlight = false }
        if browsingDeviceSnapshot { showLocalDraft() }
        let presetNames = Set(DesktopAgent.all.map(\.name) + ["Antigravity", "Herdr", "DeepSeek Coder (DSH)"])
        let existingNames = Set(hostProfiles.map(\.name))
        if !presetNames.isSubset(of: existingNames), canEditDraft, !busy {
            await prepareDesktopProfiles()
        }
        await startDiscovery()
        if devices.isEmpty { await refresh() }
        let usbCandidates = devices.filter { $0.transport == "Usb" }
        let verifiedUSB = usbCandidates.filter(\.verifiedModel)
        if connectedID == nil, usbCandidates.count == 1, verifiedUSB.count == 1, let target = verifiedUSB.first {
            selectDevice(target.id)
            await connect(target.id)
        }
        if connectedID != nil {
            if identity == nil {
                await readIdentity()
            }
            if passThroughHealth == .snapshotMissing,
               ["0.6.3", "synthetic-0.6.3"].contains(identity?.version ?? "") {
                await readDeviceConfig()
                if browsingDeviceSnapshot {
                    showLocalDraft()
                }
            }
        }
        if passThroughHealth == .ready, hostExecutionEnabled {
            let activeName = hostProfiles.first(where: { $0.id.text == hostActiveProfileId })?.name ?? "默认"
            status = "USB 透传层已就绪 · 宿主接管已开启（\(activeName)）· 不写设备"
            let receipt = HostActionReceipt(
                timestampMs: UInt64(Date().timeIntervalSince1970 * 1000),
                profileId: hostActiveProfileId ?? "",
                bindingId: "USB:303A:8298",
                actionType: "host_ready",
                target: identity?.version ?? "unknown",
                textByteLen: nil,
                outcome: "executed",
                detail: "passthrough=ready presets=\(hostProfiles.count) ax_trusted=\(LiveHostActionExecutor.isAccessibilityTrusted)"
            )
            AuditLogWriter.append(receipt)
        }
    }
    func refreshPassthroughHealth() {
        let raw = backend.checkPassthroughHealth()
        if let view = try? JSONDecoder().decode(PassThroughHealthView.self, from: Data(raw.utf8)) {
            passThroughHealth = view
        } else {
            passThroughHealth = .disconnected
        }
    }
    func resolveInputActionForTesting(bindingId: String, isPressed: Bool = true) -> String? {
        backend.resolveInputAction(bindingId: bindingId, isPressed: isPressed)
    }
    func startHostInputLoop(token: Int) {
        guard hostInputTask == nil else { return }
        hostInputTask = Task { @MainActor [weak self] in
            guard let self else { return }
            guard let stream = try? await self.subscribeVerifiedInput() else { self.hostInputTask = nil; return }
            self.readInputBoundary()
            await withTaskCancellationHandler(operation: {
                while !Task.isCancelled, let event = await stream.nextEvent() {
                    guard token == self.generation, !self.systemSleeping, !self.stopped else { break }
                    if event.kind == .inputStreamGap {
                        self.clearPendingActions(); self.inputNeedsResume = true
                        self.status = "输入流丢失，已暂停派发；请恢复输入并松开按键 / 摇杆回中"
                        break
                    }
                    if self.inputBoundary.consume(event, gateOpen: self.dispatchOpen && !self.inputNeedsResume),
                       let binding = event.bindingId, let (json, profile, state) = self.resolveHostAction(binding) {
                        self.enqueueHostAction(json: json, binding: binding, profile: profile, state: state, deviceSession: event.sessionId, deviceIngress: event.ingressSequence)
                    }
                }
            }, onCancel: { stream.close() })
            stream.close()
            if token == self.generation { self.hostInputTask = nil }
        }
    }
    private func changeDocument(fresh: Bool = false, _ operation: @escaping () async throws -> String) async throws {
        try await hostMutation { [self] in try applyDocument(try await operation(), fresh: fresh) }
        reevaluateForeground()
    }
    private func editDocument(_ action: @escaping () async throws -> String) async {
        guard canEditDraft, !busy else { return }; busy = true; defer { busy = false }
        do { try await changeDocument(fresh: false) { try await action() } } catch { status = readableError(error) }
    }
    func createProfile(name: String) async { await editDocument { [self] in try await backend.createProfile(name: name) } }
    func renameProfile(name: String) async { guard let id=profile?.id.text else {return}; await editDocument { [self] in try await backend.renameProfile(profileId:id,name:name) } }
    func duplicateProfile(name: String) async { guard let id=profile?.id.text else {return}; await editDocument { [self] in try await backend.duplicateProfile(profileId:id,name:name)} }
    func deleteProfile() async {guard let id=profile?.id.text else {return}; await editDocument { [self] in try await backend.deleteProfile(profileId:id)} }
    func moveProfile(by offset: Int) async {guard let id=profile?.id.text, let count=snapshot?.profiles.count, (0..<count).contains(profileIndex+offset) else {return}; await editDocument { [self] in try await backend.moveProfile(profileId:id,toIndex:UInt32(profileIndex+offset))} }
    func createLayer(name: String) async {guard let id=profile?.id.text else {return}; await editDocument { [self] in try await backend.createLayer(profileId:id,name:name)} }
    func renameLayer(name: String) async {guard let id=layer?.id.text else {return}; await editDocument { [self] in try await backend.renameLayer(layerId:id,name:name)} }
    func duplicateLayer(name: String) async {guard let pid=profile?.id.text, let id=layer?.id.text else {return}; await editDocument { [self] in try await backend.duplicateLayer(profileId:pid,layerId:id,name:name)} }
    func deleteLayer() async {guard let pid=profile?.id.text, let id=layer?.id.text else {return}; await editDocument { [self] in try await backend.deleteLayer(profileId:pid,layerId:id)} }
    func moveLayer(by offset: Int) async {guard let pid=profile?.id.text, let id=layer?.id.text, let count=profile?.layers.count, (0..<count).contains(layerIndex+offset) else {return}; await editDocument { [self] in try await backend.moveLayer(profileId:pid,layerId:id,toIndex:UInt32(layerIndex+offset))} }
    func presetText(scope: String) async throws -> String {
        guard canEditDraft, !busy else { throw BridgeError.Failed(message: "请先返回可编辑本地草稿") }
        return try await backend.exportPreset(profileId: scope == "document" ? nil : profile?.id.text, layerId: scope == "layer" ? layer?.id.text : nil)
    }
    func inspectPreset(_ text: String) throws -> PresetSummary {
        try JSONDecoder().decode(PresetSummary.self, from: Data(backend.inspectPreset(text: text).utf8))
    }
    func starterPreset() throws -> String { try backend.starterPreset() }
    @discardableResult func importPreset(_ text: String) async -> Bool {
        guard !busy, documentReady, !browsingDeviceSnapshot else { return false }
        busy = true; defer { busy = false }
        do {
            try await changeDocument(fresh: true) { [self] in try await backend.importPreset(text: text) }
            status = "已创建独立本地草稿 · 原文档保留 · 未激活设备或执行动作"
            return true
        } catch { status = readableError(error); return false }
    }
    func inspectAgentProposal(_ text: String) async throws -> AgentProposalPreview {
        guard canEditDraft, !busy else { throw BridgeError.Failed(message: "请先返回可编辑本地草稿") }
        return try JSONDecoder().decode(AgentProposalPreview.self, from: Data(try await backend.inspectAgentProposal(text: text).utf8))
    }
    func rejectAgentProposal() async { await backend.rejectAgentProposal() }
    @discardableResult func acceptAgentProposal(token: String) async -> Bool {
        guard canEditDraft, !busy else { return false }
        busy = true; defer { busy = false }
        do {
            try await changeDocument(fresh: true) { [self] in try await backend.acceptAgentProposal(token: token) }
            status = "已接受 Agent 提案为独立草稿 · 原文档保留 · 不写设备"
            return true
        } catch { status = readableError(error); return false }
    }
    func recoveryFiles() async throws -> [String] { try await backend.recoveryFiles() }
    func inspectRecovery(path: String) async throws -> RecoveryPreview {
        try JSONDecoder().decode(RecoveryPreview.self, from: Data(try await backend.inspectRecovery(path: path).utf8))
    }
    @discardableResult func recoverDocument(token: String) async -> Bool {
        guard !busy, !browsingDeviceSnapshot else { return false }
        busy = true; defer { busy = false }
        do {
            try await changeDocument(fresh: true) { [self] in try await backend.recoverDocument(token: token) }
            catalog = try JSONDecoder().decode([CatalogAction].self, from: Data(try await backend.actionCatalog().utf8))
            await searchActions(query: "", category: "全部", favoritesOnly: false)
            status = "已恢复为独立本地草稿 · 原件保留 · 不代表设备恢复"
            return true
        } catch { status = readableError(error); return false }
    }
    @discardableResult func saveDocumentAs() async -> Bool {
        guard canEditDraft, !busy else { return false }
        busy = true; defer { busy = false }
        do {
            try await changeDocument(fresh: true) { [self] in try await backend.saveDocumentAs() }
            status = "已另存为独立本地文档 · 原件保留 · 重开继续此草稿"
            return true
        } catch { status = readableError(error); return false }
    }
    func copyRecoveryOriginal(token: String, destination: String) async throws {
        try await backend.copyRecoveryOriginal(token: token, destination: destination)
        status = "已保存原始字节副本 · 未切换文档 · 不转换未知格式"
    }
    func createDraftFromSnapshot() async {
        guard browsingDeviceSnapshot, !stale, !busy else {return};busy=true;defer {busy=false}
        do {try await changeDocument(fresh: true) { [self] in try await backend.createDraftFromSnapshot() };status="已创建独立本地草稿 · 原工作台保留 · 重开继续此草稿"} catch {status=readableError(error)}
    }
    #if MI_SYNTHETIC_BACKEND
    private var preparedReconnectScenario=false
    private var preparedHostExecutionScenario=false
    private func prepareReconnectScenario() async {
        preparedReconnectScenario=true
        guard let core=backend as? MicroInputCore.MicroInput else {return}
        do {try await core.setSyntheticDiscovery(state:.twoDevices);try await core.setSyntheticConfig(scenario:1)} catch {status=readableError(error);return}
        await refresh();selectDevice("synthetic-interface-0")
        await connect("synthetic-interface-0");await readIdentity();await readDeviceConfig();await createDraftFromSnapshot()
        if ProcessInfo.processInfo.environment["MI_SYNTHETIC_SCENARIO"] == "apply-preparation" {
            await assign("device-key");await previewApply();return
        }
        await renameProfile(name:"Local comparison draft")
        if ProcessInfo.processInfo.environment["MI_SYNTHETIC_SCENARIO"] == "reconnect-transient" {
            await suspend()
            do {try await core.setSyntheticDiscovery(state:.reconnectedDevice)} catch {status=readableError(error);return}
            await refresh();selectDevice("synthetic-reconnected-interface-0")
            await connect("synthetic-reconnected-interface-0");await readIdentity()
        }
        do {try await core.setSyntheticConfig(scenario:4)} catch {status=readableError(error);return}
        await readDeviceConfig();await compareSnapshot()
    }
    private func prepareHostExecutionScenario() async {
        preparedHostExecutionScenario=true
        guard let core=backend as? MicroInputCore.MicroInput else {return}
        await installCodingAgentPresets()
        await setHostExecutionEnabled(true)
        do {
            try await core.setSyntheticDiscovery(state:.reconnectedDevice)
            try await core.setSyntheticConfig(scenario:1)
        } catch { status=readableError(error); return }
        await refresh()
        selectDevice("synthetic-reconnected-interface-0")
        await connect("synthetic-reconnected-interface-0")
        await readIdentity()
        await readDeviceConfig()
        showLocalDraft()
        // Wait briefly for hostInputLoop subscription to attach
        try? await Task.sleep(nanoseconds: 50_000_000)
        // Real app actions through the HID decoder and normal executor; no send/approval actions.
        for agent in DesktopAgent.all {
            guard let id = hostProfiles.first(where: { $0.name == agent.name })?.id.text else { continue }
            await setHostActiveProfile(id)
            let bindings = ["Antigravity", "DeepSeek Coder (DSH)"].contains(agent.name)
                ? ["AG00", "AG01", "AG02", "AG03"] : ["AG00"]
            for binding in bindings {
                try? await core.emitSyntheticBindingEvent(bindingId: binding, pressed: true)
                try? await core.emitSyntheticBindingEvent(bindingId: binding, pressed: false)
                try? await Task.sleep(nanoseconds: 80_000_000)
            }
            if agent.name == "Herdr" {
                for (binding, pressed) in [("AG10", true), ("AG11", true), ("AG10", false), ("AG11", false), ("AG15", true), ("AG15", false)] {
                    try? await core.emitSyntheticBindingEvent(bindingId: binding, pressed: pressed)
                }
                try? await Task.sleep(nanoseconds: 80_000_000)
            }
        }
    }
    #endif
    @Published var applyPreview: ApplyPreview?
    @Published var preparedApplyBackup: PreparedApplyBackup?
    func previewApply() async {
        guard !busy, canEditDraft else { return }; busy=true; defer {busy=false}
        do {
            let response=try await backend.previewApply()
            applyPreview=try JSONDecoder().decode(ApplyPreview.self,from:Data(response.utf8));preparedApplyBackup=nil
            status="单绑定预览 · 未发送 · 下一步仅保存本机原字节副本"
        } catch {applyPreview=nil;preparedApplyBackup=nil;status=readableError(error)}
    }
    func prepareApplyBackup() async {
        guard !busy, let preview=applyPreview else {return};busy=true;defer {busy=false}
        do {
            preparedApplyBackup=try JSONDecoder().decode(PreparedApplyBackup.self,from:Data(try await backend.prepareApplyBackup(token:preview.token).utf8))
            status="原配置副本已核验 · 尚未发送 · 设备恢复仍待验收"
        } catch {preparedApplyBackup=nil;status=readableError(error)}
    }
    func cancelApplyPreview() async {
        await backend.cancelApplyPreview();applyPreview=nil;preparedApplyBackup=nil
    }
    @Published var comparison: ReconnectComparison?
    func compareSnapshot() async {
        guard !busy else {return};busy=true;defer {busy=false}
        do {comparison=try JSONDecoder().decode(ReconnectComparison.self,from:Data(try await backend.compareSnapshot().utf8))}
        catch {comparison=nil;status=readableError(error)}
    }
    func confirmComparisonIdentity() async {
        guard !busy,let comparison else {return};busy=true;defer {busy=false}
        do {self.comparison=try JSONDecoder().decode(ReconnectComparison.self,from:Data(try await backend.confirmComparisonIdentity(token:comparison.token).utf8))}
        catch {status=readableError(error)}
    }
    func resolveComparison(_ choice:String) async {
        guard !busy,let comparison else {return};busy=true;defer {busy=false}
        do {
            try await changeDocument { [self] in try await backend.resolveComparison(token:comparison.token,choice:choice) }
            if choice != "cancel" {status="比较结果已保存 · 旧版本已备份 · 不写设备"}
            self.comparison=nil
        } catch {status=readableError(error)}
    }
    var slotKey: String { key(slot) }
    func key(_ slot: Int) -> String {
        guard let profile, let layer else { return "" }
        return bindingKey(profile: profile.id, layer: layer.id, slot: slot)
    }
    func assign(_ id: String) async {
        guard let action = catalog.first(where: {$0.id == id}) else { return }
        await saveAction(action)
    }
    @discardableResult func saveAction(_ action: CatalogAction) async -> Bool {
        if action.payload["type"]?.text == "local_multi" { return await saveMulti(action) }
        if action.payload["type"]?.text == "local_macro" { return await saveMacro(action) }
        guard canEditDraft, !busy, let profile, let layer else { return false }
        busy = true; defer { busy = false }
        do {
            let encoder = JSONEncoder()
            let p = String(decoding: try encoder.encode(profile.id), as: UTF8.self)
            let l = String(decoding: try encoder.encode(layer.id), as: UTF8.self)
            let a = String(decoding: try encoder.encode(action), as: UTF8.self)
            try await changeDocument(fresh: false) { [self] in try await backend.assignAction(profileId: p, layerId: l, slot: UInt32(slot), actionJson: a) }
            status = "草稿已保存到本机 · 不执行动作 · 不写设备"
            return true
        } catch { status = readableError(error); return false }
    }
    @discardableResult func saveControlSettings(mode: String, sectors: String) async -> Bool {
        guard canEditDraft, !busy, let profile, let layer else { return false }
        guard let count = UInt32(sectors), count <= 32 else {
            controlSettingsError = "活动扇区数：请输入 0–32 的整数（本机编辑器限制，非设备容量）"
            return false
        }
        busy = true; defer { busy = false }
        do {
            let data = try JSONEncoder().encode(LocalControlSettings(joystick_mode: mode, active_sectors: count))
            try await changeDocument(fresh: false) { [self] in try await backend.setControlSettings(profileId: profile.id.text, layerId: layer.id.text, settingsJson: String(decoding: data, as: UTF8.self)) }
            status = "控件意图已保存到本机 · 旧槽位及绑定保留 · 不写设备"
            return true
        } catch { controlSettingsError = readableError(error); return false }
    }
    var selectedMacro: MacroSummary? {
        macros.first { $0.references.contains { $0.profile_id == profile?.id.text && $0.layer_id == layer?.id.text && $0.logical_control == "logical-\(slot)" } }
    }
    @discardableResult func saveMacro(_ action: CatalogAction, id: String? = nil) async -> Bool {
        guard canEditDraft, !busy, let profile, let layer else { return false }
        busy = true; defer { busy = false }
        do {
            let encoder = JSONEncoder()
            let p = String(decoding: try encoder.encode(profile.id), as: UTF8.self)
            let l = String(decoding: try encoder.encode(layer.id), as: UTF8.self)
            let a = String(decoding: try encoder.encode(action), as: UTF8.self)
            try await changeDocument(fresh: false) { [self] in try await backend.saveMacroAction(macroId: id, profileId: p, layerId: l, slot: UInt32(slot), actionJson: a) }
            return true
        } catch { status = readableError(error); return false }
    }
    func bindMacro(_ id: String?) async {
        guard let profile, let layer else { return }
        let encoder = JSONEncoder()
        do {
            let p = String(decoding: try encoder.encode(profile.id), as: UTF8.self)
            let l = String(decoding: try encoder.encode(layer.id), as: UTF8.self)
            await editDocument { [self] in try await backend.bindMacroAction(macroId: id, profileId: p, layerId: l, slot: UInt32(slot)) }
        } catch { status = readableError(error) }
    }
    func deleteMacro(_ id: String) async { await editDocument { [self] in try await backend.deleteMacroAction(macroId: id) } }
    var selectedMulti: MacroSummary? {
        multis.first { $0.references.contains { $0.parent_action_name == nil && $0.profile_id == profile?.id.text && $0.layer_id == layer?.id.text && $0.logical_control == "logical-\(slot)" } }
    }
    @discardableResult func saveMulti(_ action: CatalogAction, id: String? = nil, additions: [String:CatalogAction] = [:]) async -> Bool {
        guard canEditDraft, !busy, let profile, let layer else { return false }
        busy = true; defer { busy = false }
        do {
            let encoder = JSONEncoder()
            func json<T: Encodable>(_ value: T) throws -> String { String(decoding: try encoder.encode(value), as: UTF8.self) }
            try await changeDocument(fresh: false) { [self] in try await backend.saveMultiAction(multiId: id, profileId: json(profile.id), layerId: json(layer.id), slot: UInt32(slot), actionJson: json(action), additionsJson: json(additions)) }
            return true
        } catch { status = readableError(error); return false }
    }
    func bindMulti(_ id: String?) async {
        guard let profile, let layer else { return }
        do {
            let encoder = JSONEncoder()
            let p = String(decoding: try encoder.encode(profile.id), as: UTF8.self)
            let l = String(decoding: try encoder.encode(layer.id), as: UTF8.self)
            await editDocument { [self] in try await backend.bindMultiAction(multiId: id, profileId: p, layerId: l, slot: UInt32(slot)) }
        } catch { status = readableError(error) }
    }
    func deleteMulti(_ id: String) async { await editDocument { [self] in try await backend.deleteMultiAction(multiId: id) } }
    func parameterSummary(_ action: CatalogAction) -> String {
        guard let multi = MultiEditorDraft(action: action) else { return action.parameterSummary }
        return MultiBranch.allCases.map { branch in
            let id = multi.branches[branch]?.actionID
            return "\(branch.label)：\(id.flatMap { graphActions[$0]?.name } ?? (id == nil ? "未分配" : "引用不可用"))"
        }.joined(separator: "\n") + "\n本地判定时长：\(multi.term) ms（固件范围未知）"
    }
    func undoDraft(redo: Bool = false) async {
        guard canEditDraft, !busy else { return }; busy = true; defer { busy = false }
        do { try await changeDocument(fresh: false) { [self] in try await backend.undoDraft(redo: redo) } }
        catch { status = readableError(error) }
    }
    var generation = 0
    var systemSleeping = false
    var stopped = false
    private var lifecycleGeneration = 0
    private var discoveryGeneration = 0
    private var sleepDisconnect: Task<Void, Never>?
    private var pendingID: String?
    private var connectionWatcher: Task<Void, Never>?
    var canDisconnect: Bool { pendingID != nil || connectedID != nil }
    @Published var identity: IdentityInfo?
    @Published var identityStatus = "身份尚未读取"
    @Published var snapshot: Snapshot?
    @Published var devices: [DeviceInfo] = []
    @Published var selectedDeviceID: String?
    @Published var discoveryStatus = "USB 发现尚未开始"
    private var events: Events?
    var selectedDevice: DeviceInfo? { devices.first { $0.id == selectedDeviceID } }
    var connectionSummary: String {
        if connectedID != nil, connectedID == selectedDeviceID {
            return identity.map { "USB 会话已连接 · 固件 \($0.version ?? "未知")" } ?? "USB 会话已连接 · 身份尚未读取"
        }
        return discoveryStatus
    }
    func startDiscovery() async {
        guard events == nil, !systemSleeping, !stopped else { return }
        let receiver = Events(self)
        events = receiver
        await backend.subscribeDeviceEvents(cb: receiver)
        await refresh()
    }
    func selectDevice(_ id: String) {
        guard !systemSleeping, !stopped, let device = devices.first(where: { $0.id == id }) else { return }
        selectedDeviceID = id
        discoveryStatus = device.verifiedModel ? "已连接 USB · 固件未知 · 尚未打开设备" : "未知型号 · 尚未验证 · 固件未知"
    }
    func discoveryFailed(_ message: String) {
        guard !systemSleeping, !stopped else { return }
        devices = []; selectedDeviceID = nil
        discoveryStatus = "发现失败 · \(message)"
    }
    @Published var profileIndex = 0
    @Published var layerIndex = 0
    @Published var selectedBinding: JSONValue?
    @Published var allowed = false
    @Published var status = "未连接设备"
    @Published var connectedID: String?
    @Published var stale = false
    @Published var busy = false
    init(backend: Backend, documentURL: URL? = nil, actionExecutor: HostActionExecuting? = nil) {
        self.backend = backend
        self.documentOverride = documentURL
        self.actionExecutor = actionExecutor ?? LiveHostActionExecutor()
    }
    var profile: Profile? { guard let ps = snapshot?.profiles, ps.indices.contains(profileIndex) else { return nil }; return ps[profileIndex] }
    var layer: Layer? { guard let ls = profile?.layers, ls.indices.contains(layerIndex) else { return nil }; return ls[layerIndex] }
    func selectProfile(_ index: Int) { controlSettingsError = nil; profileIndex = index; layerIndex = 0; slot = 0; selectedBinding = nil }
    func selectLayer(_ index: Int) { controlSettingsError = nil; layerIndex = index; slot = 0; selectedBinding = nil }
    func refresh() async {
        // R-D02: descriptors only, no permission preflight/request or device open.
        guard !systemSleeping, !stopped else { return }
        discoveryGeneration += 1; let token = discoveryGeneration
        do {
            let values = try await backend.listDevices()
            guard token == discoveryGeneration else { return }
            received(values)
        } catch { if token == discoveryGeneration { discoveryFailed(readableError(error)) } }
    }
    func received(_ values: [DeviceInfo]) {
        guard !systemSleeping, !stopped else { return }
        let removed = selectedDeviceID.map { id in !values.contains { $0.id == id } } ?? false
        devices = values
        if removed {
            selectedDeviceID = nil
            discoveryStatus = "设备已断开 · 可继续使用离线工作台"
        } else if selectedDeviceID == nil {
            discoveryStatus = values.isEmpty ? "无 USB 候选设备" : "发现 \(values.count) 个 USB 候选 · 请选择设备"
        }
        if let id = pendingID ?? connectedID, !values.contains(where: { $0.id == id }) { retireConnection("设备已断开") }
    }
    func loadDemo() async {
        guard !busy else { return }; busy = true; defer { busy = false }
        generation += 1; let token = generation
        hostInputTask?.cancel(); hostInputTask = nil
        await backend.disconnect(); guard token == generation else { return }; connectedID = nil; identity = nil; identityStatus = "身份尚未读取"; refreshPassthroughHealth()
        do { let json = try await hostMutation { [self] in try await backend.loadDemo() }; guard token == generation else { return }; try replace(json); localSnapshot = snapshot; browsingDeviceSnapshot = false; status = "本地草稿 · 演示基线 · 未与设备同步" } catch { if token == generation { status = readableError(error) } }
    }
    func connect(_ id: String) async {
        guard !busy, !systemSleeping, !stopped else { return }
        generation += 1; let token = generation
        busy = true; defer { if token == generation { busy = false } }
        connectionWatcher?.cancel()
        hostInputTask?.cancel(); hostInputTask = nil
        stale = snapshot != nil
        connectedID = nil; identity = nil; identityStatus = "身份尚未读取"; pendingID = id
        refreshPassthroughHealth()
        // Preserve provenance: a failed device read cannot turn demo data into a device snapshot.
        do {
            try await backend.connect(id: id)
            guard token == generation else { return }
            pendingID = nil; connectedID = id; status = "设备已打开 · 身份尚未读取 · 配置尚未读取"
            refreshPassthroughHealth()
            inputNeedsResume = false; inputBoundary = HostInputBoundary(); startHostInputLoop(token: token)
            connectionWatcher = Task { [weak self, backend] in
                let reason = await backend.waitForDisconnect()
                guard !Task.isCancelled, !reason.isEmpty, let self, token == self.generation else { return }
                self.retireConnection(reason)
            }
        } catch { if token == generation { pendingID = nil; stale = snapshot != nil; status = "连接失败 · \(readableError(error))" } }
    }
    func readIdentity() async {
        guard !busy, connectedID != nil else { return }
        let token = generation
        busy = true; defer { if token == generation { busy = false } }
        identity = nil; identityStatus = "正在读取身份"
        do {
            let result = try await backend.readIdentity()
            guard token == generation else { return }
            identity = result
            identityStatus = "身份已读回 · \(result.sourceMethod)"
            status = "设备身份已读回 · 固件 \(result.version ?? "未知") · 配置尚未读取"
            refreshPassthroughHealth()
        } catch { if token == generation { identityStatus = "身份读取失败 · \(readableError(error))" } }
    }
    func readDeviceConfig() async {
        guard !busy, connectedID != nil else { return }
        let token = generation
        busy = true; defer { if token == generation { busy = false } }
        stale = snapshot != nil
        status = "正在读取设备配置 · 旧数据尚未更新"
        do {
            let json = try await backend.readConfig()
            guard token == generation else { return }
            let result = try JSONDecoder().decode(Snapshot.self, from: Data(json.utf8))
            if !browsingDeviceSnapshot {
                localSnapshot = snapshot
                localSelection = (profile?.id.text, layer?.id.text, slot)
            }
            snapshot = result; browsingDeviceSnapshot = true; stale = false; selectProfile(0)
            status = result.source == "DeviceSnapshot" ? "设备快照 · 只读 · 已核对文件大小及摘要" : "合成设备夹具 · 只读 · 不是实际设备快照"
            refreshPassthroughHealth()
        } catch { if token == generation { stale = snapshot != nil; status = "配置读取失败 · \(readableError(error))" } }
    }

    private func retireConnection(_ reason: String) {
        clearPendingActions(); inputBoundary.session = nil
        generation += 1; pendingID = nil; busy = false
        connectionWatcher?.cancel(); connectionWatcher = nil
        hostInputTask?.cancel(); hostInputTask = nil
        connectedID = nil; identity = nil; identityStatus = "身份尚未读取"
        passThroughHealth = .disconnected
        stale = snapshot != nil; status = reason
    }
    func suspend() async { retireConnection("会话已断开"); await backend.disconnect() }
    // Retire synchronously in the notification callback, before asynchronous close/join.
    func systemWillSleep() {
        guard !stopped else { return }
        lifecycleGeneration += 1
        guard !systemSleeping else { return }
        invalidateHostWork()
        systemSleeping = true
        discoveryGeneration += 1
        events?.invalidatePending()
        retireConnection("系统睡眠 · 设备快照已过期，唤醒后请明确选择并重新连接")
        devices = []; selectedDeviceID = nil
        discoveryStatus = "系统睡眠 · 等待唤醒"
        sleepDisconnect = Task { [backend] in await backend.disconnect() }
    }
    func systemDidWake() async {
        guard systemSleeping, !stopped else { return }
        let token = lifecycleGeneration
        await sleepDisconnect?.value
        guard !Task.isCancelled, token == lifecycleGeneration, systemSleeping, !stopped else { return }
        sleepDisconnect = nil
        events?.invalidatePending()
        systemSleeping = false
        discoveryStatus = events == nil ? "USB 发现尚未开始" : "系统已唤醒 · 请明确选择设备并重新连接"
        if events != nil { await refresh() }
    }
    func shutdown() async {
        invalidateHostWork()
        stopped = true; lifecycleGeneration += 1; discoveryGeneration += 1
        events?.invalidatePending()
        retireConnection("应用正在退出")
        await sleepDisconnect?.value
        sleepDisconnect = nil
        await hostExecutionTask?.value
        await backend.shutdown()
    }
    private func replace(_ json: String) throws { let result = try JSONDecoder().decode(Snapshot.self, from: Data(json.utf8)); snapshot = result; stale = false; selectProfile(0) }
}
// Rust callbacks can already be queued on MainActor when sleep/wake retires a session.
// A locked epoch captures ownership at enqueue time, before crossing that executor.
private final class DiscoveryDeliveryEpoch: @unchecked Sendable {
    private let lock = NSLock()
    private var value = 0
    func current() -> Int { lock.withLock { value } }
    func invalidate() { lock.withLock { value += 1 } }
}
@MainActor final class Events: DeviceEvents {
    private weak var store: Store?
    private nonisolated let delivery = DiscoveryDeliveryEpoch()
    init(_ store: Store) { self.store = store }
    func invalidatePending() { delivery.invalidate() }
    nonisolated func changed(devices: [DeviceInfo]) {
        let token = delivery.current()
        Task { @MainActor [weak self] in
            guard let self, token == self.delivery.current() else { return }
            self.store?.received(devices)
        }
    }
    nonisolated func failed(message: String) {
        let token = delivery.current()
        Task { @MainActor [weak self] in
            guard let self, token == self.delivery.current() else { return }
            self.store?.discoveryFailed(message)
        }
    }
}

// R-A02: keep bridge type names and protocol internals out of normal status text.
func readableError(_ error: Error) -> String {
    if case BridgeError.Disconnected = error { return "设备已断开；请重新连接" }
    if case BridgeError.Timeout = error { return "请求超时；可以手动重试" }
    if case BridgeError.Unavailable = error { return "设备 I/O 失败，原因未确认；请检查连接后重试" }
    if case BridgeError.Failed(let message) = error {
        if message.contains("协议未验证") { return "协议未验证，设备读取待确认" }
        return message
    }
    return error.localizedDescription
}
