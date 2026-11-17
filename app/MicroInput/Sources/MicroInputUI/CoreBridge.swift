import AppKit
import Foundation
import MicroInputCoreFFI
import Observation
import SwiftUI

private final class CoreTeardownBox: @unchecked Sendable {
    var timer: Timer?
    var observer: NSObjectProtocol?
    let center: NotificationCenter
    var handle: OpaquePointer?

    init(center: NotificationCenter) {
        self.center = center
    }

    func stopLoops() {
        timer?.invalidate()
        timer = nil
        if let observer {
            center.removeObserver(observer)
            self.observer = nil
        }
    }

    func releaseAll() {
        stopLoops()
        if let handle {
            mi_core_free(handle)
            self.handle = nil
        }
    }
}

@MainActor
@Observable
public final class CoreStore {
    @ObservationIgnored private let lifecycle: CoreTeardownBox
    @ObservationIgnored private var workspaceLifecycle: WorkspaceLifecycle?
    @ObservationIgnored private var lastObservedBundleId: String?

    @ObservationIgnored private let projectionDecoder = CoreStore.makeProjectionDecoder()
    @ObservationIgnored private let commandEncoder = JSONEncoder()

    @ObservationIgnored public private(set) var model: ProjectionModel = ProjectionModel()

    public private(set) var header: HeaderSliceModel = HeaderSliceModel()
    public private(set) var sidebar: SidebarSliceModel = SidebarSliceModel()
    public private(set) var canvas: CanvasSliceModel = CanvasSliceModel()
    public private(set) var sessionStrip: SessionStripSliceModel = SessionStripSliceModel()
    public private(set) var inspector: InspectorSliceModel = InspectorSliceModel()
    public private(set) var statusMenu: StatusMenuSliceModel = StatusMenuSliceModel()
    public private(set) var overlay: OverlaySliceModel = OverlaySliceModel()
    public private(set) var footer: FooterSliceModel = FooterSliceModel()
    public private(set) var firmware: FirmwareSliceModel = FirmwareSliceModel()
    public private(set) var v070Hardware: V070HardwareSliceModel = V070HardwareSliceModel()
    public private(set) var activeModal: ActiveModalModel? = nil
    public private(set) var locale: ResolvedLocale = .zhHans

    public init(
        isolatedDirectory: String? = nil,
        pollInIsolation: Bool = false,
        workspaceNotificationCenter: NotificationCenter = NSWorkspace.shared.notificationCenter
    ) {
        let box = CoreTeardownBox(center: workspaceNotificationCenter)
        if let dir = isolatedDirectory {
            box.handle = dir.withCString { ptr in
                mi_core_new(ptr)
            }
        } else {
            box.handle = mi_core_new(nil)
        }
        self.lifecycle = box
        refreshProjection()
        if isolatedDirectory == nil && !LaunchOriginDetector.isIsolatedOrVerifyEnvironment() {
            syncSystemLocale(Locale.preferredLanguages)
        }
        workspaceLifecycle = WorkspaceLifecycle(center: workspaceNotificationCenter) { [weak self] event in
            self?.dispatchCommandDomain(event.rawValue)
        }
        if isolatedDirectory == nil || pollInIsolation {
            startEventLoops(observeWorkspace: isolatedDirectory == nil)
        }
    }

    deinit {
        let box = lifecycle
        if Thread.isMainThread {
            box.releaseAll()
        } else {
            DispatchQueue.main.async {
                box.releaseAll()
            }
        }
    }

    public func stopEventLoops() {
        workspaceLifecycle?.stop()
        workspaceLifecycle = nil
        lifecycle.stopLoops()
    }

    private func startEventLoops(observeWorkspace: Bool) {
        let timer = Timer(timeInterval: 0.12, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, self.lifecycle.timer != nil else { return }
                self.pollHardware()
            }
        }
        timer.tolerance = 0.03
        RunLoop.main.add(timer, forMode: .common)
        lifecycle.timer = timer

        guard observeWorkspace else { return }

        lifecycle.observer = lifecycle.center.addObserver(
            forName: NSWorkspace.didActivateApplicationNotification,
            object: nil,
            queue: .main
        ) { [weak self] note in
            let app = note.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication
            MainActor.assumeIsolated {
                guard let self, self.lifecycle.observer != nil else { return }
                self.observeForeground(app?.bundleIdentifier)
            }
        }
        observeForeground(NSWorkspace.shared.frontmostApplication?.bundleIdentifier)
    }

    private func observeForeground(_ bundleId: String?) {
        guard bundleId != Bundle.main.bundleIdentifier else { return }
        guard bundleId != lastObservedBundleId else { return }
        lastObservedBundleId = bundleId
        dispatchCommand("foreground_app_changed", ForegroundBundleAction(bundle_id: bundleId))
    }

    public func refreshProjection() {
        guard let h = lifecycle.handle, let cStr = mi_core_project_json(h) else { return }
        applyProjectionCStr(cStr)
    }

    public func pollHardware() {
        guard let h = lifecycle.handle, let cStr = mi_core_poll_json(h) else { return }
        applyProjectionCStr(cStr)
    }

    public func dispatch(_ domain: String, _ action: Any) {
        dispatchRaw(["domain": domain, "action": action])
    }

    public func stagePresetImport(data: Data) {
        let maxBytes = 4 * 1024 * 1024 + 1
        dispatchCommand("status_menu", ["StagePresetImportBytes": [UInt8](data.prefix(maxBytes))])
    }

    public func selectHeaderCandidateDevice(_ selector: String) {
        dispatchCommand("header", SelectCandidateDeviceCommand(SelectCandidateDevice: SelectorPayload(selector: selector)))
    }

    public func selectStatusMenuCandidateDevice(_ selector: String) {
        dispatchCommand("status_menu", SelectCandidateDeviceCommand(SelectCandidateDevice: SelectorPayload(selector: selector)))
    }

    public func dispatchV070Hardware(_ action: Any) {
        dispatch("v070_hardware", action)
    }

    public func setV070AutoLayerSync(_ enabled: Bool) {
        dispatchCommand("v070_hardware", SetAutoLayerSyncCommand(SetAutoLayerSync: EnabledPayload(enabled: enabled)))
    }

    public func setV070SessionSaRgbSync(_ enabled: Bool) {
        dispatchCommand("v070_hardware", SetSessionSaRgbSyncCommand(SetSessionSaRgbSync: EnabledPayload(enabled: enabled)))
    }

    public func tr(_ zh: String) -> String {
        L10n.tr(zh, locale)
    }

    public func setUiLanguage(_ language: UiLanguagePreference) {
        dispatchCommand("header", SetUiLanguageCommand(SetUiLanguage: LanguagePayload(language: language.rawValue)))
    }

    public func syncSystemLocale(_ preferredLanguages: [String]) {
        dispatchCommand("header", SyncSystemLocaleCommand(SyncSystemLocale: PreferredLanguagesPayload(preferred_languages: preferredLanguages)))
    }

    public func setSilentAtLogin(_ silent: Bool) {
        dispatchCommand("header", SetSilentAtLoginCommand(SetSilentAtLogin: SilentPayload(silent: silent)))
    }

    public func prepareForTermination() {
        dispatchCommandDomain("system_will_terminate")
    }

    public func dispatchCommand<Action: Encodable>(_ domain: String, _ action: Action) {
        guard let data = try? commandEncoder.encode(DomainCommandEnvelope(domain: domain, action: action)) else { return }
        dispatchCommandData(data)
    }

    private func dispatchCommandDomain(_ domain: String) {
        guard let data = try? commandEncoder.encode(DomainOnlyEnvelope(domain: domain)) else { return }
        dispatchCommandData(data)
    }

    public func dispatchRaw(_ payload: [String: Any]) {
        guard JSONSerialization.isValidJSONObject(payload),
              let data = try? JSONSerialization.data(withJSONObject: payload)
        else { return }
        dispatchCommandData(data)
    }

    private func dispatchCommandData(_ data: Data) {
        guard let h = lifecycle.handle else { return }
        var terminated = data
        terminated.append(0)
        let updatedPtr = terminated.withUnsafeBytes { rawBuf -> UnsafeMutablePointer<CChar>? in
            guard let base = rawBuf.bindMemory(to: CChar.self).baseAddress else { return nil }
            return mi_core_dispatch_json(h, base)
        }
        guard let cStr = updatedPtr else { return }
        applyProjectionCStr(cStr)
    }

    private func applyProjectionCStr(_ rawPtr: UnsafeMutablePointer<CChar>) {
        defer { mi_core_string_free(rawPtr) }
        ViewRenderTelemetry.record("CoreStore.decodeProjection")
        let data = Data(bytesNoCopy: UnsafeMutableRawPointer(rawPtr), count: Int(strlen(rawPtr)), deallocator: .none)
        let dto: WorkbenchProjectionDTO
        do {
            dto = try projectionDecoder.decode(WorkbenchProjectionDTO.self, from: data)
        } catch {
            NSLog("WorkbenchProjectionDTO decode error: \(error)")
            return
        }
        let next = dto.toModel()
        self.model = next
        commitDomainSlices(from: next)
    }

    func commitDomainSlices(from m: ProjectionModel) {
        let nextHeader = HeaderSliceModel(from: m)
        if self.header != nextHeader { self.header = nextHeader }

        let nextSidebar = SidebarSliceModel(from: m)
        if self.sidebar != nextSidebar { self.sidebar = nextSidebar }

        let nextCanvas = CanvasSliceModel(from: m)
        if self.canvas != nextCanvas { self.canvas = nextCanvas }

        let nextSessionStrip = SessionStripSliceModel(from: m)
        if self.sessionStrip != nextSessionStrip { self.sessionStrip = nextSessionStrip }

        let nextInspector = InspectorSliceModel(from: m)
        if self.inspector != nextInspector { self.inspector = nextInspector }

        let nextStatusMenu = StatusMenuSliceModel(from: m)
        if self.statusMenu != nextStatusMenu { self.statusMenu = nextStatusMenu }

        let nextOverlay = OverlaySliceModel(from: m)
        if self.overlay != nextOverlay { self.overlay = nextOverlay }

        let nextFooter = FooterSliceModel(from: m)
        if self.footer != nextFooter { self.footer = nextFooter }

        if self.firmware != m.firmware { self.firmware = m.firmware }
        if self.v070Hardware != m.v070Hardware { self.v070Hardware = m.v070Hardware }

        if self.activeModal != m.activeModal { self.activeModal = m.activeModal }
        if self.locale != m.resolvedLocale { self.locale = m.resolvedLocale }
    }

    nonisolated static func makeProjectionDecoder() -> JSONDecoder {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return decoder
    }

    nonisolated static func decodeJSONSlice<T: Decodable>(_ type: T.Type, from json: [String: Any]) -> T? {
        guard JSONSerialization.isValidJSONObject(json),
              let data = try? JSONSerialization.data(withJSONObject: json)
        else { return nil }
        return try? makeProjectionDecoder().decode(T.self, from: data)
    }

    nonisolated static func parseProjection(_ root: [String: Any]) -> ProjectionModel {
        decodeJSONSlice(WorkbenchProjectionDTO.self, from: root)?.toModel() ?? ProjectionModel()
    }
}

private struct DomainOnlyEnvelope: Encodable { let domain: String }
private struct DomainCommandEnvelope<Action: Encodable>: Encodable { let domain: String; let action: Action }
private struct ForegroundBundleAction: Encodable {
    let bundle_id: String?
    private enum CodingKeys: String, CodingKey { case bundle_id }
    func encode(to encoder: Encoder) throws { var c = encoder.container(keyedBy: CodingKeys.self); try c.encode(bundle_id, forKey: .bundle_id) }
}
private struct SelectorPayload: Encodable { let selector: String }
private struct SelectCandidateDeviceCommand: Encodable { let SelectCandidateDevice: SelectorPayload }
private struct EnabledPayload: Encodable { let enabled: Bool }
private struct SetAutoLayerSyncCommand: Encodable { let SetAutoLayerSync: EnabledPayload }
private struct SetSessionSaRgbSyncCommand: Encodable { let SetSessionSaRgbSync: EnabledPayload }
private struct LanguagePayload: Encodable { let language: String }
private struct SetUiLanguageCommand: Encodable { let SetUiLanguage: LanguagePayload }
private struct PreferredLanguagesPayload: Encodable { let preferred_languages: [String] }
private struct SyncSystemLocaleCommand: Encodable { let SyncSystemLocale: PreferredLanguagesPayload }
private struct SilentPayload: Encodable { let silent: Bool }
private struct SetSilentAtLoginCommand: Encodable { let SetSilentAtLogin: SilentPayload }

private struct JSONPresence: Decodable {
    init(from decoder: Decoder) throws {}
}

private struct WorkbenchProjectionDTO: Decodable {
    let firmware: FirmwareSliceModel?
    let v070Hardware: V070HardwareSliceModel?
    let statusMessage, lastExportedDiagnosticsPath, lastExportedPreset, deviceSummary: String?
    let deviceConnected: Bool?
    let documentRevision, hostRevision: Int?
    let header: HeaderDTO?
    let sidebar: SidebarDTO?
    let canvas: CanvasDTO?
    let sessionStrip: SessionStripDTO?
    let inspector: InspectorDTO?
    let statusMenu: StatusMenuDTO?
    let radial: RadialDTO?
    let radialSlotConfigs: [RadialSlotConfigDTO]?
    let overlay: OverlayDTO?
    let activeModal: ActiveModalDTO?

    func toModel() -> ProjectionModel {
        var m = ProjectionModel()
        if let firmware { m.firmware = firmware }
        if let v070Hardware { m.v070Hardware = v070Hardware }
        m.statusMessage = statusMessage ?? "已就绪"
        m.lastExportedDiagnosticsPath = lastExportedDiagnosticsPath
        m.lastExportedPreset = lastExportedPreset
        m.deviceConnected = deviceConnected ?? false
        m.deviceSummary = deviceSummary ?? "Creator Micro 2 (303A:8298) · 离线草稿模式"
        m.documentRevision = documentRevision ?? 0
        m.hostRevision = hostRevision ?? 0

        let hasHeader = header != nil
        if let header { header.apply(to: &m) }
        if let sidebar { sidebar.apply(to: &m, hasHeader: hasHeader) }
        if let canvas { canvas.apply(to: &m) }
        if let sessionStrip { sessionStrip.apply(to: &m) }
        if let inspector { inspector.apply(to: &m) }
        if let statusMenu { statusMenu.apply(to: &m, hasHeader: hasHeader) }
        if let radial { m.radialReadiness = radial.readiness ?? "Ready" }
        if let radialSlotConfigs {
            m.radialSlots = radialSlotConfigs.enumerated().map { idx, rs in rs.toModel(id: idx) }
        }
        if let cs = overlay?.content?.interactiveCheatSheet { m.cheatSheet = cs.toModel() }
        if let activeModal { m.activeModal = activeModal.toModel() }
        return m
    }
}

private struct PowerStatusDTO: Decodable {
    let batteryPercent: Int?
    let isCharging, linePowered, usbHostHold: Bool?
    let usbMode, pmMode, hostSleepStatus, summaryLabel: String?
    func toModel() -> PowerStatusBadgeModel {
        PowerStatusBadgeModel(
            batteryPercent: batteryPercent,
            isCharging: isCharging,
            linePowered: linePowered ?? false,
            usbHostHold: usbHostHold ?? false,
            usbMode: usbMode ?? "",
            pmMode: pmMode ?? "",
            hostSleepStatus: hostSleepStatus ?? "awake",
            summaryLabel: summaryLabel ?? ""
        )
    }
}

private struct CandidateDeviceDTO: Decodable {
    let path, product, serialNumber, selectorKey, displayLabel: String?
    let isConnected, isPreferred: Bool?
    func toModel() -> CandidateDeviceModel? {
        guard let path else { return nil }
        return CandidateDeviceModel(
            path: path, product: product ?? "Creator Micro 2", serialNumber: serialNumber,
            selectorKey: selectorKey, displayLabel: displayLabel ?? "",
            isConnected: isConnected ?? false, isPreferred: isPreferred ?? false
        )
    }
}

private struct HeaderDTO: Decodable {
    struct ReadyStateDTO: Decodable {
        struct TextBox: Decodable { let text: String? }
        let Ready, ModeNotice, Caution: TextBox?
    }
    struct BannerDTO: Decodable {
        struct SummaryBox: Decodable { let summary: String? }
        struct ReasonBox: Decodable { let reason: String? }
        let Ready, NativeHeld: SummaryBox?
        let Caution: ReasonBox?
    }
    let engineVersion, preferredDeviceSerial, uiLanguage, resolvedLocale: String?
    let readyState: ReadyStateDTO?
    let banner: BannerDTO?
    let showConnectButton, showReturnToHostButton, hostExecutionEnabled, accessibilityTrusted: Bool?
    let canEditDraft, canUndo, canRedo, busy, silentAtLogin: Bool?
    let powerStatus: PowerStatusDTO?
    let candidateDevices: [CandidateDeviceDTO]?

    func apply(to m: inout ProjectionModel) {
        m.engineVersion = engineVersion ?? "0.2.0"
        if let r = readyState?.Ready { m.readyKind = "Ready"; m.readyText = r.text ?? "已就绪" }
        else if let mn = readyState?.ModeNotice { m.readyKind = "ModeNotice"; m.readyText = mn.text ?? "" }
        else if let c = readyState?.Caution { m.readyKind = "Caution"; m.readyText = c.text ?? "" }
        if let r = banner?.Ready { m.bannerKind = "Ready"; m.bannerText = r.summary ?? "就绪" }
        else if let nh = banner?.NativeHeld { m.bannerKind = "NativeHeld"; m.bannerText = nh.summary ?? "原生接管" }
        else if let c = banner?.Caution { m.bannerKind = "Caution"; m.bannerText = c.reason ?? "需注意" }
        m.showConnectButton = showConnectButton ?? true
        m.showReturnToHostButton = showReturnToHostButton ?? false
        m.hostExecutionEnabled = hostExecutionEnabled ?? false
        m.accessibilityTrusted = accessibilityTrusted ?? true
        m.canEditDraft = canEditDraft ?? true
        m.canUndo = canUndo ?? false
        m.canRedo = canRedo ?? false
        m.busy = busy ?? false
        m.powerStatus = powerStatus?.toModel()
        if let devs = candidateDevices { m.candidateDevices = devs.compactMap { $0.toModel() } }
        m.preferredDeviceSerial = preferredDeviceSerial
        m.uiLanguage = uiLanguage.flatMap(UiLanguagePreference.init(rawValue:)) ?? .system
        m.resolvedLocale = resolvedLocale.flatMap(ResolvedLocale.init(rawValue:)) ?? .zhHans
        m.silentAtLogin = silentAtLogin ?? true
    }
}

private struct SidebarDTO: Decodable {
    struct RowDTO: Decodable {
        let profileId, rawName, displayName, badgeMonogram, canonicalAgent: String?
        let isSelected, isActiveHost, isProtectedOai, appInstalled, isManualFallback: Bool?
        func toModel() -> SidebarRowModel {
            SidebarRowModel(
                profileId: profileId ?? "", rawName: rawName ?? "", displayName: displayName ?? "",
                badgeMonogram: badgeMonogram ?? "", canonicalAgent: canonicalAgent,
                isSelected: isSelected ?? false, isActiveHost: isActiveHost ?? false,
                isProtectedOai: isProtectedOai ?? false, appInstalled: appInstalled ?? true,
                isManualFallback: isManualFallback ?? false
            )
        }
    }
    struct BoardLayerDTO: Decodable {
        let layer: Int?; let leds: [Bool]?
        let profileId, displayName, owner, assignmentIssue: String?; let isPreviewSelected: Bool?
        func toModel() -> BoardLayerModel? {
            guard let layer, let leds, leds.count == 3 else { return nil }
            return BoardLayerModel(
                layer: layer, leds: leds, profileId: profileId, displayName: displayName ?? "未分配",
                owner: owner ?? "Unassigned", isPreviewSelected: isPreviewSelected ?? false,
                assignmentIssue: assignmentIssue
            )
        }
    }
    struct BoardProfileChoiceDTO: Decodable {
        let profileId, displayName: String?
        func toModel() -> BoardProfileChoiceModel? {
            guard let id = profileId, let name = displayName else { return nil }
            return BoardProfileChoiceModel(profileId: id, displayName: name)
        }
    }
    struct BoardLayersDTO: Decodable {
        let layers: [BoardLayerDTO]?; let profileChoices: [BoardProfileChoiceDTO]?
        let usbStartupProfileId, usbStartupDisplayName, observedProfileKind, observedStatus, applyBlockReason: String?
        let observedLayer: Int?
        let usbPreviewSelected, canApply, configurationObserved, keyboardGrabbed, firstEventVerified, runtimeReady, restoreAvailable: Bool?
    }
    let selectedProfileId, selectedDisplayName, activeHostProfileId, lockedProfileId: String?
    let isProfileLocked, accessibilityTrusted, showOtherProfiles, editingDevice, canMoveUp, canMoveDown, canDelete: Bool?
    let rows: [RowDTO]?
    let boardLayers: BoardLayersDTO?

    func apply(to m: inout ProjectionModel, hasHeader: Bool) {
        m.selectedProfileId = selectedProfileId
        m.selectedDisplayName = selectedDisplayName ?? "未选择方案"
        m.activeHostProfileId = activeHostProfileId
        m.isProfileLocked = isProfileLocked ?? false
        m.lockedProfileId = lockedProfileId
        if !hasHeader, let trusted = accessibilityTrusted { m.accessibilityTrusted = trusted }
        m.showOtherProfiles = showOtherProfiles ?? false
        m.editingDevice = editingDevice ?? false
        m.canMoveUp = canMoveUp ?? false
        m.canMoveDown = canMoveDown ?? false
        m.canDelete = canDelete ?? false
        if let rows { m.sidebarRows = rows.map { $0.toModel() } }
        if let board = boardLayers {
            if let layers = board.layers { m.boardLayers.layers = layers.compactMap { $0.toModel() } }
            if let choices = board.profileChoices { m.boardLayers.profileChoices = choices.compactMap { $0.toModel() } }
            m.boardLayers.usbStartupProfileId = board.usbStartupProfileId
            m.boardLayers.usbStartupDisplayName = board.usbStartupDisplayName ?? "未分配"
            m.boardLayers.usbPreviewSelected = board.usbPreviewSelected ?? false
            m.boardLayers.observedLayer = board.observedLayer
            m.boardLayers.observedProfileKind = board.observedProfileKind
            m.boardLayers.observedStatus = board.observedStatus ?? "设备层未确认"
            m.boardLayers.applyBlockReason = board.applyBlockReason ?? "六层应用尚未可用"
            m.boardLayers.canApply = board.canApply ?? false
            m.boardLayers.configurationObserved = board.configurationObserved ?? false
            m.boardLayers.keyboardGrabbed = board.keyboardGrabbed ?? false
            m.boardLayers.firstEventVerified = board.firstEventVerified ?? false
            m.boardLayers.runtimeReady = board.runtimeReady ?? false
            m.boardLayers.restoreAvailable = board.restoreAvailable ?? false
        }
    }
}

private struct ProtectedLockDTO: Decodable {
    let isLocked: Bool?; let layerId, layerName, ownerClientName, lockBadgeText, explanationText, unlockButtonTitle: String?
    func toModel() -> ProtectedLockModel {
        ProtectedLockModel(
            isLocked: isLocked ?? true, layerId: layerId ?? "", layerName: layerName ?? "",
            ownerClientName: ownerClientName ?? "原生客户端", lockBadgeText: lockBadgeText ?? "原生保护层 · 只读",
            explanationText: explanationText ?? "", unlockButtonTitle: unlockButtonTitle ?? "切到自定义层解锁编辑"
        )
    }
}

private struct CanvasDTO: Decodable {
    struct LayerTabDTO: Decodable { let layerId, name: String?; let isSelected, isProtectedOai: Bool? }
    struct EncoderDTO: Decodable { let ccwLabel, cwLabel, pressLabel, holdLabel, ringGlowHex: String?; let selectedSlot: Int? }
    struct JoystickSectorDTO: Decodable { let direction, conciseLabel: String?; let logicalSlot: Int?; let isSelected: Bool? }
    struct JoystickDTO: Decodable { let mode, modeLabel, activeDirection: String?; let isSelected: Bool?; let sectors: [JoystickSectorDTO]? }
    struct KeycapCellDTO: Decodable {
        let surface, bindingId, silkscreenIcon, keycapId, slotRole, conciseCaption, fullActionTitle, perKeyLedHex: String?
        let primarySlot, effectiveSlot, row, col, spanCols, isSessionSlot: Int?
        let isSelected, isAvailable: Bool?
    }
    let selectedLayerId, selectedSurface, selectedJoystickDirection, bottomRowLayout, statusLedHex, underglowHex, boardPreviewLabel: String?
    let selectedLogicalSlot, boardPreviewLayer: Int?
    let showKeycapIcons, showKeycapLabels, canEditBindings, canAddLighting, canExportOrCopyLayer: Bool?
    let touchLedStates: [Bool]?
    let layerTabs: [LayerTabDTO]?; let encoder: EncoderDTO?; let joystick: JoystickDTO?
    let keycapCells: [KeycapCellDTO]?; let protectedLock: ProtectedLockDTO?

    func apply(to m: inout ProjectionModel) {
        m.selectedLayerId = selectedLayerId
        m.selectedSurface = selectedSurface ?? "Key0"
        m.selectedLogicalSlot = selectedLogicalSlot ?? 0
        m.selectedJoystickDirection = selectedJoystickDirection ?? "North"
        m.bottomRowLayout = bottomRowLayout ?? "split"
        m.showKeycapIcons = showKeycapIcons ?? true
        m.showKeycapLabels = showKeycapLabels ?? true
        m.statusLedHex = statusLedHex ?? "#14C76B"
        m.underglowHex = underglowHex ?? "#3B82F6"
        if let leds = touchLedStates, leds.count == 3 { m.touchLedStates = leds }
        m.boardPreviewLayer = boardPreviewLayer
        m.boardPreviewLabel = boardPreviewLabel ?? "USB 启动方案预览"
        m.canEditBindings = canEditBindings ?? true
        m.canAddLighting = canAddLighting ?? true
        m.canExportOrCopyLayer = canExportOrCopyLayer ?? true
        if let layerTabs {
            m.layerTabs = layerTabs.map {
                LayerTabModel(layerId: $0.layerId ?? "", name: $0.name ?? "", isSelected: $0.isSelected ?? false, isProtectedOai: $0.isProtectedOai ?? false)
            }
        }
        if let enc = encoder {
            m.encoderCcwLabel = enc.ccwLabel ?? "上滚"; m.encoderCwLabel = enc.cwLabel ?? "下滚"
            m.encoderPressLabel = enc.pressLabel ?? "按压"; m.encoderHoldLabel = enc.holdLabel ?? ""
            m.encoderSelectedSlot = enc.selectedSlot; m.encoderRingGlowHex = enc.ringGlowHex ?? "#14C76B"
        }
        if let joy = joystick {
            m.joystickMode = joy.mode ?? "logical_sectors"; m.joystickModeLabel = joy.modeLabel ?? "八向摇杆"
            m.joystickActiveDirection = joy.activeDirection ?? "North"; m.joystickIsSelected = joy.isSelected ?? false
            if let secs = joy.sectors {
                m.joystickSectors = secs.map {
                    JoystickSectorModel(direction: $0.direction ?? "North", logicalSlot: $0.logicalSlot ?? 16, conciseLabel: $0.conciseLabel ?? "", isSelected: $0.isSelected ?? false)
                }
            }
        }
        if let keycapCells {
            m.keycapCells = keycapCells.map {
                KeycapCellModel(
                    surface: $0.surface ?? "Key0", primarySlot: $0.primarySlot ?? 0, effectiveSlot: $0.effectiveSlot ?? 0,
                    bindingId: $0.bindingId ?? "AG00", row: $0.row ?? 0, col: $0.col ?? 0, spanCols: $0.spanCols ?? 1,
                    silkscreenIcon: $0.silkscreenIcon ?? "⌘", keycapId: $0.keycapId ?? "Blank",
                    slotRole: $0.slotRole ?? "ActionKey", conciseCaption: $0.conciseCaption ?? "未绑定",
                    fullActionTitle: $0.fullActionTitle ?? "未绑定", isSessionSlot: $0.isSessionSlot,
                    isSelected: $0.isSelected ?? false, isAvailable: $0.isAvailable ?? true,
                    perKeyLedHex: $0.perKeyLedHex ?? "#222222"
                )
            }
        }
        if let protectedLock { m.protectedLock = protectedLock.toModel() }
    }
}

private struct SessionStripDTO: Decodable {
    struct SlotDTO: Decodable {
        let slotIndex: Int?; let keyBindingId, mode, status, kind, agentMonogram, title, subtitle, ledColorHex: String?
        let isUnreadBlueDot, isWorkingAnimated, isBoundToTopKey: Bool?; let place: JSONPresence?
    }
    struct LightingPopoverDTO: Decodable {
        let isOpen, lightingEnabled, autoSessionColor, onlyGlowOnAttentionOrCompleted, glowOnWorking, suspendedByProtectedLayer: Bool?
        let ambientColor, workingColor, brightness, autoOffTimeout, manualPreviewPhase: String?
    }
    let unreadCompletedCount, workingCount: Int?; let topSixBoundAsSessionKeys, canMutateLayout: Bool?
    let slots: [SlotDTO]?; let lightingPopover: LightingPopoverDTO?

    func apply(to m: inout ProjectionModel) {
        m.unreadCompletedCount = unreadCompletedCount ?? 0; m.workingCount = workingCount ?? 0
        m.topSixBoundAsSessionKeys = topSixBoundAsSessionKeys ?? false; m.canMutateSessionLayout = canMutateLayout ?? true
        if let slots {
            m.sessionSlots = slots.map {
                SessionSlotModel(
                    slotIndex: $0.slotIndex ?? 0, keyBindingId: $0.keyBindingId ?? "AG00", mode: $0.mode ?? "Recent",
                    status: $0.status ?? "Idle", kind: $0.kind ?? "Idle", agentMonogram: $0.agentMonogram ?? "",
                    title: $0.title ?? "", subtitle: $0.subtitle ?? "", isUnreadBlueDot: $0.isUnreadBlueDot ?? false,
                    isWorkingAnimated: $0.isWorkingAnimated ?? false, isBoundToTopKey: $0.isBoundToTopKey ?? false,
                    ledColorHex: $0.ledColorHex ?? "#222222", hasAssignedPlace: $0.place != nil
                )
            }
        }
        if let lp = lightingPopover {
            m.simpleLighting = SimpleLightingModel(
                isOpen: lp.isOpen ?? false, lightingEnabled: lp.lightingEnabled ?? true,
                autoSessionColor: lp.autoSessionColor ?? true, onlyGlowOnAttentionOrCompleted: lp.onlyGlowOnAttentionOrCompleted ?? true,
                glowOnWorking: lp.glowOnWorking ?? true, ambientColor: lp.ambientColor ?? "OceanBlue",
                workingColor: lp.workingColor ?? "EmeraldGreen", brightness: lp.brightness ?? "Standard",
                autoOffTimeout: lp.autoOffTimeout ?? "ThreeMinutes", manualPreviewPhase: lp.manualPreviewPhase,
                suspendedByProtectedLayer: lp.suspendedByProtectedLayer ?? false
            )
        }
    }
}

private struct InspectorDTO: Decodable {
    struct MacroRowDTO: Decodable {
        let id, name, stepOrBranchSummary, referenceSummary: String?; let isMulti, isCurrentlyBound: Bool?; let referenceCount: Int?
        func toModel() -> MacroRowModel {
            MacroRowModel(id: id ?? "", name: name ?? "", isMulti: isMulti ?? false, stepOrBranchSummary: stepOrBranchSummary ?? "",
                          referenceCount: referenceCount ?? 0, referenceSummary: referenceSummary ?? "", isCurrentlyBound: isCurrentlyBound ?? false)
        }
    }
    struct EditableDTO: Decodable {
        struct CapabilityDTO: Decodable {
            let target, source, unavailableReason: String?
            let prerequisites: [String]?
            let executionAvailable: Bool?
        }
        struct KeycapDTO: Decodable { let id, silkCode, shortLabel, chineseHint, glyphSymbol, defaultActionId: String?; let isSelected: Bool? }
        struct ShortcutRecorderDTO: Decodable { let isRecording: Bool?; let key, target, previewBadge, validationError: String?; let modifiers: [String]? }
        struct ActionRowDTO: Decodable { let actionId, name, category, detail, targetSummary, unavailableReason: String?; let executionAvailable, isFavorite, isCurrentlyBound: Bool? }
        let selectedSurface, selectedBindingId, surfaceDisplayName, joystickDirection, joystickMode, joystickModeSummary: String?
        let bottomRowLayout, slotRole, boundActionId, boundActionName, boundActionCategory, boundActionDetail: String?
        let selectedKeycap, keycapSearchQuery, actionSearchQuery, actionCategoryFilter: String?
        let selectedLogicalSlot: Int?; let availableCategories: [String]?
        let v070HardwareMouseSupported, supportsTaskRole, isAppleFnBound, bindKeycapDefaultActionOnSelect: Bool?
        let favoritesOnly, canAddLighting, hasLayerLighting, commandExecutionEnabled: Bool?
        let boundCapability: CapabilityDTO?; let filteredKeycaps: [KeycapDTO]?; let shortcutRecorder: ShortcutRecorderDTO?
        let filteredActions: [ActionRowDTO]?; let macros, multis: [MacroRowDTO]?

        func toModel() -> EditableInspectorModel {
            var e = EditableInspectorModel()
            e.selectedSurface = selectedSurface ?? "Key0"; e.selectedLogicalSlot = selectedLogicalSlot ?? 0
            e.selectedBindingId = selectedBindingId ?? "AG00"; e.surfaceDisplayName = surfaceDisplayName ?? "按键 AG00"
            e.joystickDirection = joystickDirection; e.joystickMode = joystickMode ?? "logical_sectors"
            e.joystickModeSummary = joystickModeSummary ?? ""; e.v070HardwareMouseSupported = v070HardwareMouseSupported ?? true
            e.bottomRowLayout = bottomRowLayout ?? "split"; e.supportsTaskRole = supportsTaskRole ?? true
            e.slotRole = slotRole ?? "TaskKey"; e.boundActionId = boundActionId
            e.boundActionName = boundActionName ?? "未绑定"; e.boundActionCategory = boundActionCategory ?? ""
            e.boundActionDetail = boundActionDetail ?? ""
            if let cap = boundCapability {
                e.capabilityTarget = cap.target ?? ""
                e.capabilityPrerequisites = (cap.prerequisites ?? []).joined(separator: " · ")
                e.capabilitySource = cap.source ?? ""
                e.capabilityAvailable = cap.executionAvailable ?? true
                e.capabilityUnavailableReason = cap.unavailableReason ?? ""
            }
            e.isAppleFnBound = isAppleFnBound ?? false; e.selectedKeycap = selectedKeycap ?? "Blank"
            e.keycapSearchQuery = keycapSearchQuery ?? ""; e.bindKeycapDefaultActionOnSelect = bindKeycapDefaultActionOnSelect ?? true
            if let kcs = filteredKeycaps {
                e.filteredKeycaps = kcs.map {
                    KeycapCatalogItemModel(keycapId: $0.id ?? "Blank", silkCode: $0.silkCode ?? "BLANK", shortLabel: $0.shortLabel ?? "",
                                           chineseHint: $0.chineseHint ?? "", glyphSymbol: $0.glyphSymbol ?? "□",
                                           defaultActionId: $0.defaultActionId, isSelected: $0.isSelected ?? false)
                }
            }
            if let sr = shortcutRecorder {
                e.shortcutRecorder = ShortcutRecorderModel(
                    isRecording: sr.isRecording ?? false, key: sr.key ?? "", modifiers: sr.modifiers ?? [],
                    target: sr.target ?? "frontmost", previewBadge: sr.previewBadge ?? "未录制", validationError: sr.validationError
                )
            }
            e.actionSearchQuery = actionSearchQuery ?? ""; e.actionCategoryFilter = actionCategoryFilter ?? "全部"
            e.availableCategories = availableCategories ?? []; e.favoritesOnly = favoritesOnly ?? false
            if let acts = filteredActions {
                e.filteredActions = acts.map {
                    ActionRowModel(actionId: $0.actionId ?? "", name: $0.name ?? "", category: $0.category ?? "",
                                   detail: $0.detail ?? "", targetSummary: $0.targetSummary ?? "",
                                   executionAvailable: $0.executionAvailable ?? true, unavailableReason: $0.unavailableReason ?? "",
                                   isFavorite: $0.isFavorite ?? false, isCurrentlyBound: $0.isCurrentlyBound ?? false)
                }
            }
            if let macros { e.macros = macros.map { $0.toModel() } }
            if let multis { e.multis = multis.map { $0.toModel() } }
            e.canAddLighting = canAddLighting ?? false; e.hasLayerLighting = hasLayerLighting ?? false
            e.commandExecutionEnabled = commandExecutionEnabled ?? false
            return e
        }
    }
    struct BodyDTO: Decodable { let ProtectedLocked: ProtectedLockDTO?; let Editable: EditableDTO? }
    let layerName: String?; let isProtectedLayer: Bool?; let body: BodyDTO?

    func apply(to m: inout ProjectionModel) {
        m.inspectorLayerName = layerName ?? "默认层"; m.isProtectedLayer = isProtectedLayer ?? false
        if let lock = body?.ProtectedLocked { m.protectedLock = lock.toModel(); m.editableInspector = nil }
        else if let edit = body?.Editable { m.editableInspector = edit.toModel() }
    }
}

private struct CheatSheetDTO: Decodable {
    struct TargetAgentCardDTO: Decodable { let agentName, phaseLabel, voiceSummary: String? }
    struct KeyCellDTO: Decodable { let bindingId, controlName, label, shortLabel: String?; let available: Bool? }
    let profileName, routingSummary, joystickMode: String?; let targetAgentCard: TargetAgentCardDTO?; let keyCells: [KeyCellDTO]?
    func toModel() -> CheatSheetModel {
        var m = CheatSheetModel()
        m.profileName = profileName ?? ""; m.routingSummary = routingSummary ?? ""; m.joystickMode = joystickMode ?? "logical_sectors"
        if let card = targetAgentCard { m.agentName = card.agentName; m.agentPhaseLabel = card.phaseLabel; m.agentVoiceSummary = card.voiceSummary }
        if let keyCells {
            m.keyCells = keyCells.map {
                CheatSheetCellModel(bindingId: $0.bindingId ?? "", controlName: $0.controlName ?? "", label: $0.label ?? "",
                                    shortLabel: $0.shortLabel, available: $0.available ?? true)
            }
        }
        return m
    }
}

private struct StatusMenuDTO: Decodable {
    struct PendingImportDTO: Decodable { let name, scope: String?; let containsCompleteArchive: Bool?; let profileCount, layerCount, actionCount: Int? }
    struct FavoriteDTO: Decodable { let slot: Int?; let profileId, title: String?; let isActive: Bool? }
    let activeProfileName, routingModeSummary, lockedProfileId, competingHostNotice, radialConfirmMode, presetError, preferredDeviceSerial: String?
    let isProfileLocked, accessibilityTrusted, radialMenuEnabled, lightingEnabled, cheatSheetVisible: Bool?
    let cheatSheetPreview: CheatSheetDTO?; let pendingImport: PendingImportDTO?
    let powerStatus: PowerStatusDTO?; let candidateDevices: [CandidateDeviceDTO]?; let favorites2X2: [FavoriteDTO]?

    func apply(to m: inout ProjectionModel, hasHeader: Bool) {
        m.activeProfileName = activeProfileName ?? "未选择方案"
        m.routingModeSummary = routingModeSummary ?? "手动选择方案 · 按键映射已启用"
        m.isProfileLocked = isProfileLocked ?? m.isProfileLocked
        m.lockedProfileId = lockedProfileId ?? m.lockedProfileId
        if !hasHeader, let trusted = accessibilityTrusted { m.accessibilityTrusted = trusted }
        m.competingHostNotice = competingHostNotice
        m.radialMenuEnabled = radialMenuEnabled ?? false
        m.radialConfirmMode = radialConfirmMode ?? "encoder_press"
        m.lightingEnabled = lightingEnabled ?? true
        m.cheatSheetPreview = cheatSheetPreview?.toModel()
        m.cheatSheetVisible = cheatSheetVisible ?? false
        if let pending = pendingImport {
            m.pendingImport = PendingPresetImportModel(
                name: pending.name ?? "预设方案", scope: pending.scope ?? "document",
                containsCompleteArchive: pending.containsCompleteArchive ?? false,
                profileCount: pending.profileCount ?? 0, layerCount: pending.layerCount ?? 0, actionCount: pending.actionCount ?? 0
            )
        }
        m.presetError = presetError
        if !hasHeader {
            if let ps = powerStatus { m.powerStatus = ps.toModel() }
            if let devs = candidateDevices { m.candidateDevices = devs.compactMap { $0.toModel() } }
            if let pref = preferredDeviceSerial { m.preferredDeviceSerial = pref }
        }
        if let favs = favorites2X2 {
            m.favorites2x2 = favs.enumerated().map { idx, f in
                FavoriteSlotModel(slotIndex: f.slot ?? idx, profileId: f.profileId, title: f.title ?? "未绑定", isActive: f.isActive ?? false)
            }
        }
    }
}

private struct RadialDTO: Decodable { let readiness: String? }

private struct RadialSlotConfigDTO: Decodable {
    let kind, label, text, bundleId, adapter, symbol, colorHex: String?; let submit: Bool?
    func toModel(id: Int) -> RadialSlotConfigModel {
        RadialSlotConfigModel(id: id, kind: kind ?? "open_app", label: label ?? "", text: text, bundleId: bundleId,
                              adapter: adapter, submit: submit ?? false, symbol: symbol ?? "◎", colorHex: colorHex ?? "#3B82F6")
    }
}

private struct OverlayDTO: Decodable {
    struct ContentDTO: Decodable {
        let interactiveCheatSheet: CheatSheetDTO?
        private enum CodingKeys: String, CodingKey { case InteractiveCheatSheet }
        init(from decoder: Decoder) throws {
            if let single = try? decoder.singleValueContainer(), (try? single.decode(String.self)) != nil {
                interactiveCheatSheet = nil; return
            }
            let c = try decoder.container(keyedBy: CodingKeys.self)
            interactiveCheatSheet = try c.decodeIfPresent(CheatSheetDTO.self, forKey: .InteractiveCheatSheet)
        }
    }
    let content: ContentDTO?
}

private struct ActiveModalDTO: Decodable {
    struct MacroStepDTO: Decodable { let index: Int?; let summaryLabel: String?; let macosKeySupported: Bool? }
    struct MacroEditorDTO: Decodable {
        let macroId, name, draftKeyInput, validationError: String?; let draftDelayMs, targetSlot: Int?
        let bindToSelectedSlot, hostExecutionSupported: Bool?; let steps: [MacroStepDTO]?; let referenceSummaries: [String]?
    }
    struct MultiBranchDTO: Decodable {
        struct TargetDTO: Decodable {
            struct RefBox: Decodable { let actionId: String? }
            let Action, Macro: RefBox?
        }
        let branch, title, targetDisplayName: String?; let target: TargetDTO?
    }
    struct MultiOptionDTO: Decodable { let id, label: String? }
    struct MultiEditorDTO: Decodable {
        let multiId, name, validationError: String?; let termMs, targetSlot: Int?; let bindToSelectedSlot: Bool?
        let branches: [MultiBranchDTO]?; let availableActions, availableMacros: [MultiOptionDTO]?; let referenceSummaries: [String]?
    }
    struct LightingZoneDTO: Decodable {
        let zone, zoneTitle, effect, colorHex: String?; let brightnessPercent, speedPercent, magicPercent: Int?
        let canEditColor, canEditBrightness, canEditSpeed, canEditMagic: Bool?
        func toModel() -> LightingZoneModel {
            LightingZoneModel(zone: zone ?? "Backlight", zoneTitle: zoneTitle ?? "", effect: effect ?? "solid",
                              brightnessPercent: brightnessPercent ?? 100, speedPercent: speedPercent ?? 50,
                              magicPercent: magicPercent ?? 50, colorHex: colorHex ?? "#FFFFFF",
                              canEditColor: canEditColor ?? true, canEditBrightness: canEditBrightness ?? true,
                              canEditSpeed: canEditSpeed ?? false, canEditMagic: canEditMagic ?? false)
        }
    }
    struct LightingEditorDTO: Decodable {
        let profileId, layerId, layerName, validationError: String?; let applyToAllLayers, canAddLighting, hasLighting, previewActive: Bool?
        let previewRemainingSecs: Int?; let backlight, underglow: LightingZoneDTO?
    }
    struct ComparisonRowDTO: Decodable { let path, label, baseSummary, localSummary, deviceSummary, resultBadge: String?; let isConflict: Bool? }
    struct ReconnectComparisonDTO: Decodable {
        let token, errorMessage: String?; let revision: Int?; let identityMatches, identityConflict, identityConfirmationRequired, identityUserConfirmed, canMerge: Bool?
        let conflicts: [String]?; let rows: [ComparisonRowDTO]?
    }
    struct ApplyChangeRowDTO: Decodable { let kind, locationLabel, before, after: String? }
    struct ApplyPreparationDTO: Decodable {
        let token, kind, documentId, documentSha256, plannedKeymapSha256, beforeSummary, afterSummary: String?
        let backupPath, manifestSha256, candidateOperationId, unavailableReason, errorMessage: String?
        let revision: Int?; let backupPrepared, candidateTaken: Bool?; let changes: [ApplyChangeRowDTO]?
    }
    let MacroEditor: MacroEditorDTO?; let MultiEditor: MultiEditorDTO?; let LightingEditor: LightingEditorDTO?
    let ReconnectComparison: ReconnectComparisonDTO?; let ApplyPreparation: ApplyPreparationDTO?

    func toModel() -> ActiveModalModel? {
        if let mac = MacroEditor {
            return .macroEditor(MacroEditorModel(
                macroId: mac.macroId, name: mac.name ?? "",
                steps: (mac.steps ?? []).map { MacroStepModel(index: $0.index ?? 0, summaryLabel: $0.summaryLabel ?? "", macosKeySupported: $0.macosKeySupported ?? true) },
                draftKeyInput: mac.draftKeyInput ?? "a", draftDelayMs: mac.draftDelayMs ?? 30,
                bindToSelectedSlot: mac.bindToSelectedSlot ?? true, targetSlot: mac.targetSlot,
                referenceSummaries: mac.referenceSummaries ?? [], validationError: mac.validationError,
                hostExecutionSupported: mac.hostExecutionSupported ?? true
            ))
        }
        if let mul = MultiEditor {
            let branches = (mul.branches ?? []).map { b -> MultiBranchModel in
                let kind: String? = b.target?.Action != nil ? "Action" : (b.target?.Macro != nil ? "Macro" : nil)
                let tid = b.target?.Action?.actionId ?? b.target?.Macro?.actionId
                return MultiBranchModel(branch: b.branch ?? "Tap", title: b.title ?? "", targetKind: kind, targetId: tid, targetDisplayName: b.targetDisplayName ?? "未设置")
            }
            let toOpts = { (opts: [MultiOptionDTO]?) in (opts ?? []).compactMap { o -> MultiTargetOptionModel? in
                guard let id = o.id, let label = o.label else { return nil }; return MultiTargetOptionModel(id: id, label: label)
            }}
            return .multiEditor(MultiEditorModel(
                multiId: mul.multiId, name: mul.name ?? "", termMs: mul.termMs ?? 200, branches: branches,
                bindToSelectedSlot: mul.bindToSelectedSlot ?? true, targetSlot: mul.targetSlot,
                availableActions: toOpts(mul.availableActions), availableMacros: toOpts(mul.availableMacros),
                referenceSummaries: mul.referenceSummaries ?? [], validationError: mul.validationError
            ))
        }
        if let lit = LightingEditor {
            return .lightingEditor(LightingEditorModel(
                profileId: lit.profileId ?? "", layerId: lit.layerId ?? "", layerName: lit.layerName ?? "",
                applyToAllLayers: lit.applyToAllLayers ?? false, canAddLighting: lit.canAddLighting ?? false,
                hasLighting: lit.hasLighting ?? false, backlight: lit.backlight?.toModel(), underglow: lit.underglow?.toModel(),
                previewActive: lit.previewActive ?? false, previewRemainingSecs: lit.previewRemainingSecs ?? 0,
                validationError: lit.validationError
            ))
        }
        if let cmp = ReconnectComparison {
            let rows = (cmp.rows ?? []).map { ComparisonRowModel(path: $0.path ?? "", label: $0.label ?? "", baseSummary: $0.baseSummary ?? "", localSummary: $0.localSummary ?? "", deviceSummary: $0.deviceSummary ?? "", resultBadge: $0.resultBadge ?? "", isConflict: $0.isConflict ?? false) }
            return .reconnectComparison(ReconnectComparisonModel(
                token: cmp.token ?? "", revision: cmp.revision ?? 0, identityMatches: cmp.identityMatches ?? true,
                identityConflict: cmp.identityConflict ?? false, identityConfirmationRequired: cmp.identityConfirmationRequired ?? false,
                identityUserConfirmed: cmp.identityUserConfirmed ?? false, canMerge: cmp.canMerge ?? false,
                conflicts: cmp.conflicts ?? [], rows: rows, errorMessage: cmp.errorMessage
            ))
        }
        if let app = ApplyPreparation {
            let changes = (app.changes ?? []).enumerated().map { idx, c in
                ApplyChangeRowModel(id: "\(idx)-\(c.locationLabel ?? "")", kind: c.kind ?? "", locationLabel: c.locationLabel ?? "", before: c.before ?? "", after: c.after ?? "")
            }
            return .applyPreparation(ApplyPreparationModel(
                token: app.token ?? "", kind: app.kind ?? "", documentId: app.documentId ?? "", revision: app.revision ?? 0,
                documentSha256: app.documentSha256 ?? "", plannedKeymapSha256: app.plannedKeymapSha256 ?? "",
                beforeSummary: app.beforeSummary ?? "", afterSummary: app.afterSummary ?? "", changes: changes,
                backupPrepared: app.backupPrepared ?? false, backupPath: app.backupPath, manifestSha256: app.manifestSha256,
                candidateTaken: app.candidateTaken ?? false, candidateOperationId: app.candidateOperationId,
                unavailableReason: app.unavailableReason ?? "", errorMessage: app.errorMessage
            ))
        }
        return nil
    }
}

private final class WorkspaceObserverBag: @unchecked Sendable {
    let center: NotificationCenter
    var observers: [NSObjectProtocol] = []

    init(center: NotificationCenter) {
        self.center = center
    }

    func removeAll() {
        if !Thread.isMainThread {
            DispatchQueue.main.async { self.removeAll() }
            return
        }
        observers.forEach(center.removeObserver)
        observers.removeAll()
    }
}

@MainActor
final class WorkspaceLifecycle {
    enum Event: String {
        case willSleep = "system_will_sleep"
        case didWake = "system_did_wake"
    }

    private let bag: WorkspaceObserverBag
    private let receive: (Event) -> Void
    private var active = true

    init(center: NotificationCenter, receive: @escaping (Event) -> Void) {
        self.bag = WorkspaceObserverBag(center: center)
        self.receive = receive
        for (name, event) in [
            (NSWorkspace.willSleepNotification, Event.willSleep),
            (NSWorkspace.didWakeNotification, Event.didWake),
        ] {
            bag.observers.append(center.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated {
                    guard let self, self.active else { return }
                    self.receive(event)
                }
            })
        }
    }

    func stop() {
        active = false
        bag.removeAll()
    }

    deinit {
        bag.removeAll()
    }
}
