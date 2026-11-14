import AppKit
import Foundation
import MicroInputCoreFFI
import Observation
import SwiftUI

@MainActor
@Observable
public final class CoreStore {
    @ObservationIgnored private var handle: OpaquePointer?
    @ObservationIgnored private var pollTimer: Timer?
    @ObservationIgnored private var workspaceObserver: NSObjectProtocol?
    @ObservationIgnored private var workspaceLifecycle: WorkspaceLifecycle?
    @ObservationIgnored private let workspaceNotificationCenter: NotificationCenter
    @ObservationIgnored private var lastObservedBundleId: String?

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
    public private(set) var activeModal: ActiveModalModel? = nil

    public init(
        isolatedDirectory: String? = nil,
        pollInIsolation: Bool = false,
        workspaceNotificationCenter: NotificationCenter = NSWorkspace.shared.notificationCenter
    ) {
        self.workspaceNotificationCenter = workspaceNotificationCenter
        if let dir = isolatedDirectory {
            self.handle = dir.withCString { ptr in
                mi_core_new(ptr)
            }
        } else {
            self.handle = mi_core_new(nil)
        }
        refreshProjection()
        workspaceLifecycle = WorkspaceLifecycle(center: workspaceNotificationCenter) { [weak self] event in
            self?.dispatchRaw(["domain": event.rawValue])
        }
        if isolatedDirectory == nil || pollInIsolation {
            startEventLoops(observeWorkspace: isolatedDirectory == nil)
        }
    }

    deinit {
        pollTimer?.invalidate()
        if let observer = workspaceObserver {
            workspaceNotificationCenter.removeObserver(observer)
        }
        if let h = handle {
            mi_core_free(h)
        }
    }

    public func stopEventLoops() {
        workspaceLifecycle?.stop()
        workspaceLifecycle = nil
        pollTimer?.invalidate()
        pollTimer = nil
        if let observer = workspaceObserver {
            workspaceNotificationCenter.removeObserver(observer)
            workspaceObserver = nil
        }
    }

    private func startEventLoops(observeWorkspace: Bool) {
        let timer = Timer(timeInterval: 0.12, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, self.pollTimer != nil else { return }
                self.pollHardware()
            }
        }
        timer.tolerance = 0.03
        RunLoop.main.add(timer, forMode: .common)
        pollTimer = timer

        guard observeWorkspace else { return }

        workspaceObserver = workspaceNotificationCenter.addObserver(
            forName: NSWorkspace.didActivateApplicationNotification,
            object: nil,
            queue: .main
        ) { [weak self] note in
            let app = note.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication
            MainActor.assumeIsolated {
                guard let self, self.workspaceObserver != nil else { return }
                self.observeForeground(app?.bundleIdentifier)
            }
        }
        observeForeground(NSWorkspace.shared.frontmostApplication?.bundleIdentifier)
    }

    private func observeForeground(_ bundleId: String?) {
        guard bundleId != Bundle.main.bundleIdentifier else { return }
        guard bundleId != lastObservedBundleId else { return }
        lastObservedBundleId = bundleId
        dispatchRaw([
            "domain": "foreground_app_changed",
            "action": ["bundle_id": (bundleId as Any?) ?? NSNull()]
        ])
    }

    public func refreshProjection() {
        guard let h = handle else { return }
        guard let cStr = mi_core_project_json(h) else { return }
        defer { mi_core_string_free(cStr) }
        let jsonString = String(cString: cStr)
        applyProjectionJSON(jsonString)
    }

    public func pollHardware() {
        guard let h = handle else { return }
        guard let cStr = mi_core_poll_json(h) else { return }
        defer { mi_core_string_free(cStr) }
        let jsonString = String(cString: cStr)
        applyProjectionJSON(jsonString)
    }

    public func dispatch(_ domain: String, _ action: Any) {
        dispatchRaw(["domain": domain, "action": action])
    }

    public func selectHeaderCandidateDevice(_ selector: String) {
        dispatch("header", ["SelectCandidateDevice": ["selector": selector]])
    }

    public func selectStatusMenuCandidateDevice(_ selector: String) {
        dispatch("status_menu", ["SelectCandidateDevice": ["selector": selector]])
    }

    public func setV070AutoLayerSync(_ enabled: Bool) {
        dispatch("firmware", V070HardwareCommand.setAutoLayerSync(enabled))
    }

    public func setV070SessionSaRgbSync(_ enabled: Bool) {
        dispatch("firmware", V070HardwareCommand.setSessionSaRgbSync(enabled))
    }

    public func prepareForTermination() {
        dispatchRaw(["domain": "system_will_terminate"])
    }

    public func dispatchRaw(_ payload: [String: Any]) {
        guard let h = handle else { return }
        guard JSONSerialization.isValidJSONObject(payload),
              let data = try? JSONSerialization.data(withJSONObject: payload),
              let cmdString = String(data: data, encoding: .utf8)
        else {
            return
        }
        let updatedPtr = cmdString.withCString { ptr in
            mi_core_dispatch_json(h, ptr)
        }
        guard let cStr = updatedPtr else { return }
        defer { mi_core_string_free(cStr) }
        let jsonString = String(cString: cStr)
        applyProjectionJSON(jsonString)
    }

    private func applyProjectionJSON(_ jsonString: String) {
        ViewRenderTelemetry.record("CoreStore.decodeProjection")
        guard let data = jsonString.data(using: .utf8),
              let root = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
        else {
            return
        }
        let next = Self.parseProjection(root)
        self.model = next
        commitDomainSlices(from: next)
    }

    private func commitDomainSlices(from m: ProjectionModel) {
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

        if self.activeModal != m.activeModal { self.activeModal = m.activeModal }
    }

    static func parseProjection(_ root: [String: Any]) -> ProjectionModel {
        var m = ProjectionModel()
        m.firmware = FirmwareSliceModel(json: root["firmware"] as? [String: Any] ?? [:])
        m.statusMessage = root["status_message"] as? String ?? "已就绪"
        m.lastExportedPreset = root["last_exported_preset"] as? String
        m.deviceConnected = root["device_connected"] as? Bool ?? false
        m.deviceSummary = root["device_summary"] as? String ?? "Creator Micro 2 (303A:8298) · 离线草稿模式"
        m.documentRevision = root["document_revision"] as? Int ?? 0
        m.hostRevision = root["host_revision"] as? Int ?? 0

        if let header = root["header"] as? [String: Any] {
            m.engineVersion = header["engine_version"] as? String ?? "0.2.0"
            if let rs = header["ready_state"] as? [String: [String: Any]] {
                if let r = rs["Ready"] {
                    m.readyKind = "Ready"
                    m.readyText = r["text"] as? String ?? "已就绪"
                } else if let mn = rs["ModeNotice"] {
                    m.readyKind = "ModeNotice"
                    m.readyText = mn["text"] as? String ?? ""
                } else if let c = rs["Caution"] {
                    m.readyKind = "Caution"
                    m.readyText = c["text"] as? String ?? ""
                }
            }
            if let bn = header["banner"] as? [String: [String: Any]] {
                if let r = bn["Ready"] {
                    m.bannerKind = "Ready"
                    m.bannerText = r["summary"] as? String ?? "就绪"
                } else if let nh = bn["NativeHeld"] {
                    m.bannerKind = "NativeHeld"
                    m.bannerText = nh["summary"] as? String ?? "原生接管"
                } else if let c = bn["Caution"] {
                    m.bannerKind = "Caution"
                    m.bannerText = c["reason"] as? String ?? "需注意"
                }
            }
            m.showConnectButton = header["show_connect_button"] as? Bool ?? true
            m.showReturnToHostButton = header["show_return_to_host_button"] as? Bool ?? false
            m.hostExecutionEnabled = header["host_execution_enabled"] as? Bool ?? false
            m.accessibilityTrusted = header["accessibility_trusted"] as? Bool ?? true
            m.canEditDraft = header["can_edit_draft"] as? Bool ?? true
            m.canUndo = header["can_undo"] as? Bool ?? false
            m.canRedo = header["can_redo"] as? Bool ?? false
            m.busy = header["busy"] as? Bool ?? false
            m.powerStatus = (header["power_status"] as? [String: Any]).map(parsePowerStatus)
            if let devs = header["candidate_devices"] as? [[String: Any]] {
                m.candidateDevices = parseCandidateDevices(devs)
            }
            m.preferredDeviceSerial = header["preferred_device_serial"] as? String
        }

        if let sidebar = root["sidebar"] as? [String: Any] {
            m.selectedProfileId = sidebar["selected_profile_id"] as? String
            m.selectedDisplayName = sidebar["selected_display_name"] as? String ?? "未选择方案"
            m.activeHostProfileId = sidebar["active_host_profile_id"] as? String
            m.autoSwitchState = sidebar["auto_switch"] as? String ?? "Active"
            m.isProfileLocked = sidebar["is_profile_locked"] as? Bool ?? false
            m.lockedProfileId = sidebar["locked_profile_id"] as? String
            if let trusted = sidebar["accessibility_trusted"] as? Bool {
                m.accessibilityTrusted = trusted
            }
            m.showOtherProfiles = sidebar["show_other_profiles"] as? Bool ?? false
            m.editingDevice = sidebar["editing_device"] as? Bool ?? false
            m.canMoveUp = sidebar["can_move_up"] as? Bool ?? false
            m.canMoveDown = sidebar["can_move_down"] as? Bool ?? false
            m.canDelete = sidebar["can_delete"] as? Bool ?? false
            if let rows = sidebar["rows"] as? [[String: Any]] {
                m.sidebarRows = rows.map { r in
                    SidebarRowModel(
                        profileId: r["profile_id"] as? String ?? "",
                        rawName: r["raw_name"] as? String ?? "",
                        displayName: r["display_name"] as? String ?? "",
                        badgeMonogram: r["badge_monogram"] as? String ?? "",
                        canonicalAgent: r["canonical_agent"] as? String,
                        isSelected: r["is_selected"] as? Bool ?? false,
                        isActiveHost: r["is_active_host"] as? Bool ?? false,
                        isProtectedOai: r["is_protected_oai"] as? Bool ?? false,
                        appInstalled: r["app_installed"] as? Bool ?? true,
                        isManualFallback: r["is_manual_fallback"] as? Bool ?? false
                    )
                }
            }
            if let board = sidebar["board_layers"] as? [String: Any] {
                if let layers = board["layers"] as? [[String: Any]] {
                    m.boardLayers.layers = layers.compactMap { row in
                        guard let layer = row["layer"] as? Int,
                              let leds = row["leds"] as? [Bool], leds.count == 3 else { return nil }
                        return BoardLayerModel(
                            layer: layer, leds: leds,
                            profileId: row["profile_id"] as? String,
                            displayName: row["display_name"] as? String ?? "未分配",
                            owner: row["owner"] as? String ?? "Unassigned",
                            isPreviewSelected: row["is_preview_selected"] as? Bool ?? false,
                            assignmentIssue: row["assignment_issue"] as? String
                        )
                    }
                }
                if let choices = board["profile_choices"] as? [[String: Any]] {
                    m.boardLayers.profileChoices = choices.compactMap { row in
                        guard let id = row["profile_id"] as? String, let name = row["display_name"] as? String else { return nil }
                        return BoardProfileChoiceModel(profileId: id, displayName: name)
                    }
                }
                m.boardLayers.usbStartupProfileId = board["usb_startup_profile_id"] as? String
                m.boardLayers.usbStartupDisplayName = board["usb_startup_display_name"] as? String ?? "未分配"
                m.boardLayers.usbPreviewSelected = board["usb_preview_selected"] as? Bool ?? false
                m.boardLayers.observedLayer = board["observed_layer"] as? Int
                m.boardLayers.observedProfileKind = board["observed_profile_kind"] as? String
                m.boardLayers.observedStatus = board["observed_status"] as? String ?? "设备层未确认"
                m.boardLayers.applyBlockReason = board["apply_block_reason"] as? String ?? "六层应用尚未可用"
                m.boardLayers.canApply = board["can_apply"] as? Bool ?? false
                m.boardLayers.configurationObserved = board["configuration_observed"] as? Bool ?? false
                m.boardLayers.keyboardGrabbed = board["keyboard_grabbed"] as? Bool ?? false
                m.boardLayers.firstEventVerified = board["first_event_verified"] as? Bool ?? false
                m.boardLayers.runtimeReady = board["runtime_ready"] as? Bool ?? false
                m.boardLayers.restoreAvailable = board["restore_available"] as? Bool ?? false
            }
        }

        if let canvas = root["canvas"] as? [String: Any] {
            m.selectedLayerId = canvas["selected_layer_id"] as? String
            m.selectedSurface = canvas["selected_surface"] as? String ?? "Key0"
            m.selectedLogicalSlot = canvas["selected_logical_slot"] as? Int ?? 0
            m.selectedJoystickDirection = canvas["selected_joystick_direction"] as? String ?? "North"
            m.bottomRowLayout = canvas["bottom_row_layout"] as? String ?? "split"
            m.showKeycapIcons = canvas["show_keycap_icons"] as? Bool ?? true
            m.showKeycapLabels = canvas["show_keycap_labels"] as? Bool ?? true
            m.statusLedHex = canvas["status_led_hex"] as? String ?? "#14C76B"
            m.underglowHex = canvas["underglow_hex"] as? String ?? "#3B82F6"
            if let leds = canvas["touch_led_states"] as? [Bool], leds.count == 3 {
                m.touchLedStates = leds
            }
            m.boardPreviewLayer = canvas["board_preview_layer"] as? Int
            m.boardPreviewLabel = canvas["board_preview_label"] as? String ?? "USB 启动方案预览"
            m.canEditBindings = canvas["can_edit_bindings"] as? Bool ?? true
            m.canAddLighting = canvas["can_add_lighting"] as? Bool ?? true
            m.canExportOrCopyLayer = canvas["can_export_or_copy_layer"] as? Bool ?? true

            if let tabs = canvas["layer_tabs"] as? [[String: Any]] {
                m.layerTabs = tabs.map { t in
                    LayerTabModel(
                        layerId: t["layer_id"] as? String ?? "",
                        name: t["name"] as? String ?? "",
                        isSelected: t["is_selected"] as? Bool ?? false,
                        isProtectedOai: t["is_protected_oai"] as? Bool ?? false
                    )
                }
            }
            if let enc = canvas["encoder"] as? [String: Any] {
                m.encoderCcwLabel = enc["ccw_label"] as? String ?? "上滚"
                m.encoderCwLabel = enc["cw_label"] as? String ?? "下滚"
                m.encoderPressLabel = enc["press_label"] as? String ?? "按压"
                m.encoderSelectedSlot = enc["selected_slot"] as? Int
                m.encoderRingGlowHex = enc["ring_glow_hex"] as? String ?? "#14C76B"
            }
            if let joy = canvas["joystick"] as? [String: Any] {
                m.joystickModeLabel = joy["mode_label"] as? String ?? "八向摇杆"
                m.joystickActiveDirection = joy["active_direction"] as? String ?? "North"
                m.joystickIsSelected = joy["is_selected"] as? Bool ?? false
                if let secs = joy["sectors"] as? [[String: Any]] {
                    m.joystickSectors = secs.map { s in
                        JoystickSectorModel(
                            direction: s["direction"] as? String ?? "North",
                            logicalSlot: s["logical_slot"] as? Int ?? 16,
                            conciseLabel: s["concise_label"] as? String ?? "",
                            isSelected: s["is_selected"] as? Bool ?? false
                        )
                    }
                }
            }
            if let cells = canvas["keycap_cells"] as? [[String: Any]] {
                m.keycapCells = cells.map { c in
                    KeycapCellModel(
                        surface: c["surface"] as? String ?? "Key0",
                        primarySlot: c["primary_slot"] as? Int ?? 0,
                        effectiveSlot: c["effective_slot"] as? Int ?? 0,
                        bindingId: c["binding_id"] as? String ?? "AG00",
                        row: c["row"] as? Int ?? 0,
                        col: c["col"] as? Int ?? 0,
                        spanCols: c["span_cols"] as? Int ?? 1,
                        silkscreenIcon: c["silkscreen_icon"] as? String ?? "⌘",
                        keycapId: c["keycap_id"] as? String ?? "Blank",
                        slotRole: c["slot_role"] as? String ?? "ActionKey",
                        conciseCaption: c["concise_caption"] as? String ?? "未绑定",
                        fullActionTitle: c["full_action_title"] as? String ?? "未绑定",
                        isSessionSlot: c["is_session_slot"] as? Int,
                        isSelected: c["is_selected"] as? Bool ?? false,
                        isAvailable: c["is_available"] as? Bool ?? true,
                        perKeyLedHex: c["per_key_led_hex"] as? String ?? "#222222"
                    )
                }
            }
            if let lock = canvas["protected_lock"] as? [String: Any] {
                m.protectedLock = parseProtectedLock(lock)
            }
        }

        if let strip = root["session_strip"] as? [String: Any] {
            m.unreadCompletedCount = strip["unread_completed_count"] as? Int ?? 0
            m.workingCount = strip["working_count"] as? Int ?? 0
            m.topSixBoundAsSessionKeys = strip["top_six_bound_as_session_keys"] as? Bool ?? false
            m.canMutateSessionLayout = strip["can_mutate_layout"] as? Bool ?? true
            if let slots = strip["slots"] as? [[String: Any]] {
                m.sessionSlots = slots.map { s in
                    SessionSlotModel(
                        slotIndex: s["slot_index"] as? Int ?? 0,
                        keyBindingId: s["key_binding_id"] as? String ?? "AG00",
                        mode: s["mode"] as? String ?? "Recent",
                        status: s["status"] as? String ?? "Idle",
                        kind: s["kind"] as? String ?? "Idle",
                        agentMonogram: s["agent_monogram"] as? String ?? "",
                        title: s["title"] as? String ?? "",
                        subtitle: s["subtitle"] as? String ?? "",
                        isUnreadBlueDot: s["is_unread_blue_dot"] as? Bool ?? false,
                        isWorkingAnimated: s["is_working_animated"] as? Bool ?? false,
                        isBoundToTopKey: s["is_bound_to_top_key"] as? Bool ?? false,
                        ledColorHex: s["led_color_hex"] as? String ?? "#222222",
                        placeRaw: s["place"]
                    )
                }
            }
            if let lp = strip["lighting_popover"] as? [String: Any] {
                m.simpleLighting = SimpleLightingModel(
                    isOpen: lp["is_open"] as? Bool ?? false,
                    lightingEnabled: lp["lighting_enabled"] as? Bool ?? true,
                    autoSessionColor: lp["auto_session_color"] as? Bool ?? true,
                    onlyGlowOnAttentionOrCompleted: lp["only_glow_on_attention_or_completed"] as? Bool ?? true,
                    glowOnWorking: lp["glow_on_working"] as? Bool ?? true,
                    ambientColor: lp["ambient_color"] as? String ?? "OceanBlue",
                    workingColor: lp["working_color"] as? String ?? "EmeraldGreen",
                    brightness: lp["brightness"] as? String ?? "Standard",
                    autoOffTimeout: lp["auto_off_timeout"] as? String ?? "ThreeMinutes",
                    manualPreviewPhase: lp["manual_preview_phase"] as? String,
                    suspendedByProtectedLayer: lp["suspended_by_protected_layer"] as? Bool ?? false
                )
            }
        }

        if let insp = root["inspector"] as? [String: Any] {
            m.inspectorLayerName = insp["layer_name"] as? String ?? "默认层"
            m.isProtectedLayer = insp["is_protected_layer"] as? Bool ?? false
            if let body = insp["body"] as? [String: Any] {
                if let lock = body["ProtectedLocked"] as? [String: Any] {
                    m.protectedLock = parseProtectedLock(lock)
                    m.editableInspector = nil
                } else if let edit = body["Editable"] as? [String: Any] {
                    m.editableInspector = parseEditableInspector(edit)
                }
            }
        }

        if let sm = root["status_menu"] as? [String: Any] {
            m.activeProfileName = sm["active_profile_name"] as? String ?? "未选择方案"
            m.routingModeSummary = sm["routing_mode_summary"] as? String ?? "手动模式"
            m.autoFollowEnabled = sm["auto_follow_enabled"] as? Bool ?? false
            m.manualOverride = sm["manual_override"] as? Bool ?? false
            m.isProfileLocked = sm["is_profile_locked"] as? Bool ?? m.isProfileLocked
            m.lockedProfileId = (sm["locked_profile_id"] as? String) ?? m.lockedProfileId
            if let trusted = sm["accessibility_trusted"] as? Bool {
                m.accessibilityTrusted = trusted
            }
            m.competingHostNotice = sm["competing_host_notice"] as? String
            m.radialMenuEnabled = sm["radial_menu_enabled"] as? Bool ?? false
            m.radialConfirmMode = sm["radial_confirm_mode"] as? String ?? "encoder_press"
            m.lightingEnabled = sm["lighting_enabled"] as? Bool ?? true
            m.cheatSheetPreview = (sm["cheat_sheet_preview"] as? [String: Any]).map(parseCheatSheet)
            m.cheatSheetVisible = sm["cheat_sheet_visible"] as? Bool ?? false
            if let ps = sm["power_status"] as? [String: Any] {
                m.powerStatus = parsePowerStatus(ps)
            }
            if let devs = sm["candidate_devices"] as? [[String: Any]] {
                m.candidateDevices = parseCandidateDevices(devs)
            }
            if let pref = sm["preferred_device_serial"] as? String {
                m.preferredDeviceSerial = pref
            }
            if let favs = sm["favorites_2x2"] as? [[String: Any]] {
                m.favorites2x2 = favs.enumerated().map { idx, f in
                    FavoriteSlotModel(
                        slotIndex: f["slot"] as? Int ?? idx,
                        profileId: f["profile_id"] as? String,
                        title: f["title"] as? String ?? "未绑定",
                        isActive: f["is_active"] as? Bool ?? false
                    )
                }
            }
        }

        if let rad = root["radial"] as? [String: Any] {
            m.radialReadiness = rad["readiness"] as? String ?? "Ready"
        }
        if let rslots = root["radial_slot_configs"] as? [[String: Any]] {
            m.radialSlots = rslots.enumerated().map { idx, rs in
                RadialSlotConfigModel(
                    id: idx,
                    kind: rs["kind"] as? String ?? "open_app",
                    label: rs["label"] as? String ?? "",
                    text: rs["text"] as? String,
                    bundleId: rs["bundle_id"] as? String,
                    adapter: rs["adapter"] as? String,
                    submit: rs["submit"] as? Bool ?? false,
                    symbol: rs["symbol"] as? String ?? "◎",
                    colorHex: rs["color_hex"] as? String ?? "#3B82F6"
                )
            }
        }

        if let ov = root["overlay"] as? [String: Any],
           let content = ov["content"] as? [String: Any]
        {
            if let cs = content["InteractiveCheatSheet"] as? [String: Any] {
                m.cheatSheet = parseCheatSheet(cs)
            }
        }

        if let modalDict = root["active_modal"] as? [String: Any] {
            m.activeModal = parseActiveModal(modalDict)
        }

        return m
    }

    private static func parseProtectedLock(_ lock: [String: Any]) -> ProtectedLockModel {
        ProtectedLockModel(
            isLocked: lock["is_locked"] as? Bool ?? true,
            layerId: lock["layer_id"] as? String ?? "",
            layerName: lock["layer_name"] as? String ?? "",
            ownerClientName: lock["owner_client_name"] as? String ?? "原生客户端",
            lockBadgeText: lock["lock_badge_text"] as? String ?? "原生保护层 · 只读",
            explanationText: lock["explanation_text"] as? String ?? "",
            unlockButtonTitle: lock["unlock_button_title"] as? String ?? "切到自定义层解锁编辑"
        )
    }

    private static func parseEditableInspector(_ edit: [String: Any]) -> EditableInspectorModel {
        var e = EditableInspectorModel()
        e.selectedSurface = edit["selected_surface"] as? String ?? "Key0"
        e.selectedLogicalSlot = edit["selected_logical_slot"] as? Int ?? 0
        e.selectedBindingId = edit["selected_binding_id"] as? String ?? "AG00"
        e.surfaceDisplayName = edit["surface_display_name"] as? String ?? "按键 AG00"
        e.joystickDirection = edit["joystick_direction"] as? String
        e.joystickMode = edit["joystick_mode"] as? String ?? "logical_sectors"
        e.joystickModeSummary = edit["joystick_mode_summary"] as? String ?? ""
        e.v070HardwareMouseSupported = edit["v070_hardware_mouse_supported"] as? Bool ?? true
        e.bottomRowLayout = edit["bottom_row_layout"] as? String ?? "Split"
        e.supportsTaskRole = edit["supports_task_role"] as? Bool ?? true
        e.slotRole = edit["slot_role"] as? String ?? "TaskKey"
        e.boundActionId = edit["bound_action_id"] as? String
        e.boundActionName = edit["bound_action_name"] as? String ?? "未绑定"
        e.boundActionCategory = edit["bound_action_category"] as? String ?? ""
        e.boundActionDetail = edit["bound_action_detail"] as? String ?? ""
        if let cap = edit["bound_capability"] as? [String: Any] {
            e.capabilityTarget = cap["target"] as? String ?? ""
            e.capabilityPrerequisites = cap["prerequisites"] as? String ?? ""
            e.capabilitySource = cap["source"] as? String ?? ""
            e.capabilityAvailable = cap["execution_available"] as? Bool ?? true
            e.capabilityUnavailableReason = cap["unavailable_reason"] as? String ?? ""
        }
        e.isAppleFnBound = edit["is_apple_fn_bound"] as? Bool ?? false
        e.selectedKeycap = edit["selected_keycap"] as? String ?? "Blank"
        e.keycapSearchQuery = edit["keycap_search_query"] as? String ?? ""
        e.bindKeycapDefaultActionOnSelect = edit["bind_keycap_default_action_on_select"] as? Bool ?? true
        if let kcs = edit["filtered_keycaps"] as? [[String: Any]] {
            e.filteredKeycaps = kcs.map { k in
                KeycapCatalogItemModel(
                    keycapId: k["id"] as? String ?? "Blank",
                    silkCode: k["silk_code"] as? String ?? "BLANK",
                    shortLabel: k["short_label"] as? String ?? "",
                    chineseHint: k["chinese_hint"] as? String ?? "",
                    glyphSymbol: k["glyph_symbol"] as? String ?? "□",
                    defaultActionId: k["default_action_id"] as? String,
                    isSelected: k["is_selected"] as? Bool ?? false
                )
            }
        }
        if let sr = edit["shortcut_recorder"] as? [String: Any] {
            e.shortcutRecorder = ShortcutRecorderModel(
                isRecording: sr["is_recording"] as? Bool ?? false,
                key: sr["key"] as? String ?? "",
                modifiers: sr["modifiers"] as? [String] ?? [],
                target: sr["target"] as? String ?? "frontmost",
                previewBadge: sr["preview_badge"] as? String ?? "未录制",
                validationError: sr["validation_error"] as? String
            )
        }
        e.actionSearchQuery = edit["action_search_query"] as? String ?? ""
        e.actionCategoryFilter = edit["action_category_filter"] as? String ?? "全部"
        e.availableCategories = edit["available_categories"] as? [String] ?? []
        e.favoritesOnly = edit["favorites_only"] as? Bool ?? false
        if let acts = edit["filtered_actions"] as? [[String: Any]] {
            e.filteredActions = acts.map { a in
                ActionRowModel(
                    actionId: a["action_id"] as? String ?? "",
                    name: a["name"] as? String ?? "",
                    category: a["category"] as? String ?? "",
                    detail: a["detail"] as? String ?? "",
                    targetSummary: a["target_summary"] as? String ?? "",
                    executionAvailable: a["execution_available"] as? Bool ?? true,
                    unavailableReason: a["unavailable_reason"] as? String ?? "",
                    isFavorite: a["is_favorite"] as? Bool ?? false,
                    isCurrentlyBound: a["is_currently_bound"] as? Bool ?? false
                )
            }
        }
        if let macs = edit["macros"] as? [[String: Any]] {
            e.macros = macs.map(parseMacroRow)
        }
        if let muls = edit["multis"] as? [[String: Any]] {
            e.multis = muls.map(parseMacroRow)
        }
        e.canAddLighting = edit["can_add_lighting"] as? Bool ?? false
        e.hasLayerLighting = edit["has_layer_lighting"] as? Bool ?? false
        e.commandExecutionEnabled = edit["command_execution_enabled"] as? Bool ?? true
        return e
    }

    private static func parseMacroRow(_ r: [String: Any]) -> MacroRowModel {
        MacroRowModel(
            id: r["id"] as? String ?? "",
            name: r["name"] as? String ?? "",
            isMulti: r["is_multi"] as? Bool ?? false,
            stepOrBranchSummary: r["step_or_branch_summary"] as? String ?? "",
            referenceCount: r["reference_count"] as? Int ?? 0,
            referenceSummary: r["reference_summary"] as? String ?? "",
            isCurrentlyBound: r["is_currently_bound"] as? Bool ?? false
        )
    }

    private static func parseCheatSheet(_ cs: [String: Any]) -> CheatSheetModel {
        var m = CheatSheetModel()
        m.profileName = cs["profile_name"] as? String ?? ""
        m.routingSummary = cs["routing_summary"] as? String ?? ""
        if let card = cs["target_agent_card"] as? [String: Any] {
            m.agentName = card["agent_name"] as? String
            m.agentPhaseLabel = card["phase_label"] as? String
            m.agentVoiceSummary = card["voice_summary"] as? String
        }
        if let cells = cs["key_cells"] as? [[String: Any]] {
            m.keyCells = cells.map { c in
                CheatSheetCellModel(
                    bindingId: c["binding_id"] as? String ?? "",
                    controlName: c["control_name"] as? String ?? "",
                    label: c["label"] as? String ?? "",
                    shortLabel: c["short_label"] as? String,
                    available: c["available"] as? Bool ?? true
                )
            }
        }
        return m
    }

    private static func parseActiveModal(_ dict: [String: Any]) -> ActiveModalModel? {
        if let mac = dict["MacroEditor"] as? [String: Any] {
            let steps = (mac["steps"] as? [[String: Any]] ?? []).map { s in
                MacroStepModel(
                    index: s["index"] as? Int ?? 0,
                    summaryLabel: s["summary_label"] as? String ?? "",
                    macosKeySupported: s["macos_key_supported"] as? Bool ?? true
                )
            }
            return .macroEditor(
                MacroEditorModel(
                    macroId: mac["macro_id"] as? String,
                    name: mac["name"] as? String ?? "",
                    steps: steps,
                    draftKeyInput: mac["draft_key_input"] as? String ?? "a",
                    draftDelayMs: mac["draft_delay_ms"] as? Int ?? 30,
                    bindToSelectedSlot: mac["bind_to_selected_slot"] as? Bool ?? true,
                    targetSlot: mac["target_slot"] as? Int,
                    referenceSummaries: mac["reference_summaries"] as? [String] ?? [],
                    validationError: mac["validation_error"] as? String,
                    hostExecutionSupported: mac["host_execution_supported"] as? Bool ?? true
                )
            )
        }
        if let mul = dict["MultiEditor"] as? [String: Any] {
            let branches = (mul["branches"] as? [[String: Any]] ?? []).map { b -> MultiBranchModel in
                var tKind: String? = nil
                var tId: String? = nil
                if let t = b["target"] as? [String: [String: Any]] {
                    if let act = t["Action"] {
                        tKind = "Action"
                        tId = act["action_id"] as? String
                    } else if let mac = t["Macro"] {
                        tKind = "Macro"
                        tId = mac["action_id"] as? String
                    }
                }
                return MultiBranchModel(
                    branch: b["branch"] as? String ?? "Tap",
                    title: b["title"] as? String ?? "",
                    targetKind: tKind,
                    targetId: tId,
                    targetDisplayName: b["target_display_name"] as? String ?? "未设置"
                )
            }
            let availActs = (mul["available_actions"] as? [[Any]] ?? []).compactMap { pair -> (String, String)? in
                guard pair.count >= 2, let id = pair[0] as? String, let name = pair[1] as? String else { return nil }
                return (id, name)
            }
            let availMacs = (mul["available_macros"] as? [[Any]] ?? []).compactMap { pair -> (String, String)? in
                guard pair.count >= 2, let id = pair[0] as? String, let name = pair[1] as? String else { return nil }
                return (id, name)
            }
            return .multiEditor(
                MultiEditorModel(
                    multiId: mul["multi_id"] as? String,
                    name: mul["name"] as? String ?? "",
                    termMs: mul["term_ms"] as? Int ?? 200,
                    branches: branches,
                    bindToSelectedSlot: mul["bind_to_selected_slot"] as? Bool ?? true,
                    targetSlot: mul["target_slot"] as? Int,
                    availableActions: availActs,
                    availableMacros: availMacs,
                    referenceSummaries: mul["reference_summaries"] as? [String] ?? [],
                    validationError: mul["validation_error"] as? String
                )
            )
        }
        if let lit = dict["LightingEditor"] as? [String: Any] {
            return .lightingEditor(
                LightingEditorModel(
                    profileId: lit["profile_id"] as? String ?? "",
                    layerId: lit["layer_id"] as? String ?? "",
                    layerName: lit["layer_name"] as? String ?? "",
                    applyToAllLayers: lit["apply_to_all_layers"] as? Bool ?? false,
                    canAddLighting: lit["can_add_lighting"] as? Bool ?? false,
                    hasLighting: lit["has_lighting"] as? Bool ?? false,
                    backlight: (lit["backlight"] as? [String: Any]).map(parseLightingZone),
                    underglow: (lit["underglow"] as? [String: Any]).map(parseLightingZone),
                    previewActive: lit["preview_active"] as? Bool ?? false,
                    previewRemainingSecs: lit["preview_remaining_secs"] as? Int ?? 0,
                    validationError: lit["validation_error"] as? String
                )
            )
        }
        if let cmp = dict["ReconnectComparison"] as? [String: Any] {
            let rows = (cmp["rows"] as? [[String: Any]] ?? []).map { r in
                ComparisonRowModel(
                    path: r["path"] as? String ?? "",
                    label: r["label"] as? String ?? "",
                    baseSummary: r["base_summary"] as? String ?? "",
                    localSummary: r["local_summary"] as? String ?? "",
                    deviceSummary: r["device_summary"] as? String ?? "",
                    resultBadge: r["result_badge"] as? String ?? "",
                    isConflict: r["is_conflict"] as? Bool ?? false
                )
            }
            return .reconnectComparison(
                ReconnectComparisonModel(
                    token: cmp["token"] as? String ?? "",
                    revision: cmp["revision"] as? Int ?? 0,
                    identityMatches: cmp["identity_matches"] as? Bool ?? true,
                    identityConflict: cmp["identity_conflict"] as? Bool ?? false,
                    identityConfirmationRequired: cmp["identity_confirmation_required"] as? Bool ?? false,
                    identityUserConfirmed: cmp["identity_user_confirmed"] as? Bool ?? false,
                    canMerge: cmp["can_merge"] as? Bool ?? false,
                    conflicts: cmp["conflicts"] as? [String] ?? [],
                    rows: rows,
                    errorMessage: cmp["error_message"] as? String
                )
            )
        }
        if let app = dict["ApplyPreparation"] as? [String: Any] {
            let changes = (app["changes"] as? [[String: Any]] ?? []).enumerated().map { idx, c in
                ApplyChangeRowModel(
                    id: "\(idx)-\(c["location_label"] as? String ?? "")",
                    kind: c["kind"] as? String ?? "",
                    locationLabel: c["location_label"] as? String ?? "",
                    before: c["before"] as? String ?? "",
                    after: c["after"] as? String ?? ""
                )
            }
            return .applyPreparation(
                ApplyPreparationModel(
                    token: app["token"] as? String ?? "",
                    kind: app["kind"] as? String ?? "",
                    documentId: app["document_id"] as? String ?? "",
                    revision: app["revision"] as? Int ?? 0,
                    documentSha256: app["document_sha256"] as? String ?? "",
                    plannedKeymapSha256: app["planned_keymap_sha256"] as? String ?? "",
                    beforeSummary: app["before_summary"] as? String ?? "",
                    afterSummary: app["after_summary"] as? String ?? "",
                    changes: changes,
                    backupPrepared: app["backup_prepared"] as? Bool ?? false,
                    backupPath: app["backup_path"] as? String,
                    manifestSha256: app["manifest_sha256"] as? String,
                    candidateTaken: app["candidate_taken"] as? Bool ?? false,
                    candidateOperationId: app["candidate_operation_id"] as? String,
                    unavailableReason: app["unavailable_reason"] as? String ?? "",
                    errorMessage: app["error_message"] as? String
                )
            )
        }
        return nil
    }

    private static func parsePowerStatus(_ ps: [String: Any]) -> PowerStatusBadgeModel {
        PowerStatusBadgeModel(
            batteryPercent: (ps["battery_percent"] as? NSNumber)?.intValue,
            isCharging: ps["is_charging"] as? Bool,
            linePowered: ps["line_powered"] as? Bool ?? false,
            usbHostHold: ps["usb_host_hold"] as? Bool ?? false,
            usbMode: ps["usb_mode"] as? String ?? "",
            pmMode: ps["pm_mode"] as? String ?? "",
            hostSleepStatus: ps["host_sleep_status"] as? String ?? "awake",
            summaryLabel: ps["summary_label"] as? String ?? ""
        )
    }

    private static func parseCandidateDevices(_ devs: [[String: Any]]) -> [CandidateDeviceModel] {
        devs.compactMap { d in
            guard let path = d["path"] as? String else { return nil }
            return CandidateDeviceModel(
                path: path,
                product: d["product"] as? String ?? "Creator Micro 2",
                serialNumber: d["serial_number"] as? String,
                displayLabel: d["display_label"] as? String ?? "",
                isConnected: d["is_connected"] as? Bool ?? false,
                isPreferred: d["is_preferred"] as? Bool ?? false
            )
        }
    }

    private static func parseLightingZone(_ z: [String: Any]) -> LightingZoneModel {
        LightingZoneModel(
            zone: z["zone"] as? String ?? "Backlight",
            zoneTitle: z["zone_title"] as? String ?? "",
            effect: z["effect"] as? String ?? "solid",
            brightnessPercent: z["brightness_percent"] as? Int ?? 100,
            speedPercent: z["speed_percent"] as? Int ?? 50,
            magicPercent: z["magic_percent"] as? Int ?? 50,
            colorHex: z["color_hex"] as? String ?? "#FFFFFF",
            canEditColor: z["can_edit_color"] as? Bool ?? true,
            canEditBrightness: z["can_edit_brightness"] as? Bool ?? true,
            canEditSpeed: z["can_edit_speed"] as? Bool ?? false,
            canEditMagic: z["can_edit_magic"] as? Bool ?? false
        )
    }
}

@MainActor
final class WorkspaceLifecycle {
    enum Event: String {
        case willSleep = "system_will_sleep"
        case didWake = "system_did_wake"
    }

    private let center: NotificationCenter
    private let receive: (Event) -> Void
    private var observers: [NSObjectProtocol] = []
    private var active = true

    init(center: NotificationCenter, receive: @escaping (Event) -> Void) {
        self.center = center
        self.receive = receive
        for (name, event) in [
            (NSWorkspace.willSleepNotification, Event.willSleep),
            (NSWorkspace.didWakeNotification, Event.didWake),
        ] {
            observers.append(center.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated {
                    guard let self, self.active else { return }
                    self.receive(event)
                }
            })
        }
    }

    func stop() {
        active = false
        observers.forEach(center.removeObserver)
        observers.removeAll()
    }

    deinit {
        observers.forEach(center.removeObserver)
    }
}
