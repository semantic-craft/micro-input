import Foundation

@MainActor
public enum ViewRenderTelemetry {
    public static var isEnabled: Bool = false
    public private(set) static var counts: [String: Int] = [:]

    public static func reset() {
        counts.removeAll()
    }

    @inline(__always)
    public static func record(_ surface: String) {
        guard isEnabled else { return }
        counts[surface, default: 0] += 1
    }

    public static func count(_ surface: String) -> Int {
        counts[surface, default: 0]
    }
}

public struct SidebarRowModel: Identifiable, Equatable {
    public var id: String { profileId }
    public let profileId: String
    public let rawName: String
    public let displayName: String
    public let badgeMonogram: String
    public let canonicalAgent: String?
    public let isSelected: Bool
    public let isActiveHost: Bool
    public let isProtectedOai: Bool
    public let appInstalled: Bool
    public let isManualFallback: Bool
}

public struct HardwareProfileSlotModel: Identifiable, Equatable {
    public var id: Int { slotIndex }
    public let slotIndex: Int
    public let slotKind: String
    public let title: String
    public let triggerBadge: String
    public let profileId: String
    public let displayName: String
    public let monogram: String
    public let bundleId: String?
    public let isActiveSlot: Bool
    public let isActiveProfile: Bool
}

public struct LayerTabModel: Identifiable, Equatable {
    public var id: String { layerId }
    public let layerId: String
    public let name: String
    public let isSelected: Bool
    public let isProtectedOai: Bool
}

public struct ProtectedLockModel: Equatable {
    public let isLocked: Bool
    public let layerId: String
    public let layerName: String
    public let ownerClientName: String
    public let lockBadgeText: String
    public let explanationText: String
    public let unlockButtonTitle: String
}

public struct KeycapCellModel: Identifiable, Equatable {
    public var id: String { bindingId }
    public let surface: String
    public let primarySlot: Int
    public let effectiveSlot: Int
    public let bindingId: String
    public let row: Int
    public let col: Int
    public let spanCols: Int
    public let silkscreenIcon: String
    public let keycapId: String
    public let slotRole: String
    public let conciseCaption: String
    public let fullActionTitle: String
    public let isSessionSlot: Int?
    public let isSelected: Bool
    public let isAvailable: Bool
    public let perKeyLedHex: String
}

public struct JoystickSectorModel: Identifiable, Equatable {
    public var id: String { direction }
    public let direction: String
    public let logicalSlot: Int
    public let conciseLabel: String
    public let isSelected: Bool
}

public struct SessionSlotModel: Identifiable, Equatable {
    public var id: Int { slotIndex }
    public let slotIndex: Int
    public let keyBindingId: String
    public let mode: String
    public let status: String
    public let kind: String
    public let agentMonogram: String
    public let title: String
    public let subtitle: String
    public let isUnreadBlueDot: Bool
    public let isWorkingAnimated: Bool
    public let isBoundToTopKey: Bool
    public let ledColorHex: String
    public let placeRaw: Any?

    public static func == (lhs: SessionSlotModel, rhs: SessionSlotModel) -> Bool {
        lhs.slotIndex == rhs.slotIndex
            && lhs.keyBindingId == rhs.keyBindingId
            && lhs.mode == rhs.mode
            && lhs.status == rhs.status
            && lhs.kind == rhs.kind
            && lhs.title == rhs.title
            && lhs.subtitle == rhs.subtitle
            && lhs.isUnreadBlueDot == rhs.isUnreadBlueDot
            && lhs.isWorkingAnimated == rhs.isWorkingAnimated
            && lhs.isBoundToTopKey == rhs.isBoundToTopKey
            && lhs.ledColorHex == rhs.ledColorHex
    }
}

public struct SessionPillModel: Identifiable, Equatable {
    public let id: String
    public let agentMonogram: String
    public let title: String
    public let kind: String
    public let badgeText: String
    public let colorHex: String
    public let watermark: String
    public let placeRaw: Any?

    public static func == (lhs: SessionPillModel, rhs: SessionPillModel) -> Bool {
        lhs.id == rhs.id && lhs.title == rhs.title && lhs.watermark == rhs.watermark
    }
}

public struct SimpleLightingModel: Equatable {
    public var isOpen: Bool = false
    public var lightingEnabled: Bool = true
    public var autoSessionColor: Bool = true
    public var onlyGlowOnAttentionOrCompleted: Bool = true
    public var glowOnWorking: Bool = false
    public var ambientColor: String = "PureWhite"
    public var brightness: String = "Standard"
    public var autoOffTimeout: String = "ThreeMinutes"
    public var manualPreviewPhase: String? = nil
    public var suspendedByProtectedLayer: Bool = false
}

public struct KeycapCatalogItemModel: Identifiable, Equatable {
    public var id: String { keycapId }
    public let keycapId: String
    public let silkCode: String
    public let shortLabel: String
    public let chineseHint: String
    public let glyphSymbol: String
    public let defaultActionId: String?
    public let isSelected: Bool
}

public struct ActionRowModel: Identifiable, Equatable {
    public var id: String { actionId }
    public let actionId: String
    public let name: String
    public let category: String
    public let detail: String
    public let targetSummary: String
    public let executionAvailable: Bool
    public let unavailableReason: String
    public let isFavorite: Bool
    public let isCurrentlyBound: Bool
}

public struct MacroRowModel: Identifiable, Equatable {
    public let id: String
    public let name: String
    public let isMulti: Bool
    public let stepOrBranchSummary: String
    public let referenceCount: Int
    public let referenceSummary: String
    public let isCurrentlyBound: Bool
}

public struct ShortcutRecorderModel: Equatable {
    public var isRecording: Bool = false
    public var key: String = ""
    public var modifiers: [String] = []
    public var target: String = "frontmost"
    public var previewBadge: String = "未录制"
    public var validationError: String? = nil
}

public struct EditableInspectorModel: Equatable {
    public var selectedSurface: String = "Key0"
    public var selectedLogicalSlot: Int = 0
    public var selectedBindingId: String = "AG00"
    public var surfaceDisplayName: String = "按键 AG00"
    public var joystickDirection: String? = nil
    public var bottomRowLayout: String = "split"
    public var supportsTaskRole: Bool = true
    public var slotRole: String = "TaskKey"
    public var boundActionId: String? = nil
    public var boundActionName: String = "未绑定"
    public var boundActionCategory: String = ""
    public var boundActionDetail: String = ""
    public var capabilityTarget: String = ""
    public var capabilityPrerequisites: String = ""
    public var capabilitySource: String = ""
    public var capabilityAvailable: Bool = true
    public var capabilityUnavailableReason: String = ""
    public var isAppleFnBound: Bool = false
    public var selectedKeycap: String = "Blank"
    public var keycapSearchQuery: String = ""
    public var filteredKeycaps: [KeycapCatalogItemModel] = []
    public var bindKeycapDefaultActionOnSelect: Bool = true
    public var shortcutRecorder: ShortcutRecorderModel = ShortcutRecorderModel()
    public var actionSearchQuery: String = ""
    public var actionCategoryFilter: String = "全部"
    public var availableCategories: [String] = []
    public var favoritesOnly: Bool = false
    public var filteredActions: [ActionRowModel] = []
    public var macros: [MacroRowModel] = []
    public var multis: [MacroRowModel] = []
    public var canAddLighting: Bool = false
    public var hasLayerLighting: Bool = false
    public var commandExecutionEnabled: Bool = true
}

public struct FavoriteSlotModel: Identifiable, Equatable {
    public var id: Int { slotIndex }
    public let slotIndex: Int
    public let profileId: String?
    public let title: String
    public let isActive: Bool
}

public struct RadialSlotConfigModel: Identifiable, Equatable {
    public let id: Int
    public let kind: String
    public let label: String
    public let text: String?
    public let bundleId: String?
    public let adapter: String?
    public let submit: Bool
    public let symbol: String
    public let colorHex: String
}

public struct MacroStepModel: Identifiable, Equatable {
    public var id: Int { index }
    public let index: Int
    public let summaryLabel: String
    public let macosKeySupported: Bool
}

public struct MacroEditorModel: Equatable {
    public let macroId: String?
    public let name: String
    public let steps: [MacroStepModel]
    public let draftKeyInput: String
    public let draftDelayMs: Int
    public let bindToSelectedSlot: Bool
    public let targetSlot: Int?
    public let referenceSummaries: [String]
    public let validationError: String?
    public let hostExecutionSupported: Bool
}

public struct MultiBranchModel: Identifiable, Equatable {
    public var id: String { branch }
    public let branch: String
    public let title: String
    public let targetKind: String?
    public let targetId: String?
    public let targetDisplayName: String
}

public struct MultiEditorModel: Equatable {
    public let multiId: String?
    public let name: String
    public let termMs: Int
    public let branches: [MultiBranchModel]
    public let bindToSelectedSlot: Bool
    public let targetSlot: Int?
    public let availableActions: [(String, String)]
    public let availableMacros: [(String, String)]
    public let referenceSummaries: [String]
    public let validationError: String?

    public static func == (lhs: MultiEditorModel, rhs: MultiEditorModel) -> Bool {
        lhs.multiId == rhs.multiId
            && lhs.name == rhs.name
            && lhs.termMs == rhs.termMs
            && lhs.branches == rhs.branches
            && lhs.validationError == rhs.validationError
    }
}

public struct LightingZoneModel: Equatable {
    public let zone: String
    public let zoneTitle: String
    public let effect: String
    public let brightnessPercent: Int
    public let speedPercent: Int
    public let magicPercent: Int
    public let colorHex: String
    public let canEditColor: Bool
    public let canEditBrightness: Bool
    public let canEditSpeed: Bool
    public let canEditMagic: Bool
}

public struct LightingEditorModel: Equatable {
    public let profileId: String
    public let layerId: String
    public let layerName: String
    public let applyToAllLayers: Bool
    public let canAddLighting: Bool
    public let hasLighting: Bool
    public let backlight: LightingZoneModel?
    public let underglow: LightingZoneModel?
    public let previewActive: Bool
    public let previewRemainingSecs: Int
    public let validationError: String?
}

public struct ComparisonRowModel: Identifiable, Equatable {
    public var id: String { path }
    public let path: String
    public let label: String
    public let baseSummary: String
    public let localSummary: String
    public let deviceSummary: String
    public let resultBadge: String
    public let isConflict: Bool
}

public struct ReconnectComparisonModel: Equatable {
    public let token: String
    public let revision: Int
    public let identityMatches: Bool
    public let identityConflict: Bool
    public let identityConfirmationRequired: Bool
    public let identityUserConfirmed: Bool
    public let canMerge: Bool
    public let conflicts: [String]
    public let rows: [ComparisonRowModel]
    public let errorMessage: String?
}

public struct ApplyChangeRowModel: Identifiable, Equatable {
    public let id: String
    public let kind: String
    public let locationLabel: String
    public let before: String
    public let after: String
}

public struct ApplyPreparationModel: Equatable {
    public let token: String
    public let kind: String
    public let documentId: String
    public let revision: Int
    public let documentSha256: String
    public let plannedKeymapSha256: String
    public let beforeSummary: String
    public let afterSummary: String
    public let changes: [ApplyChangeRowModel]
    public let backupPrepared: Bool
    public let backupPath: String?
    public let manifestSha256: String?
    public let candidateTaken: Bool
    public let candidateOperationId: String?
    public let unavailableReason: String
    public let errorMessage: String?
}

public enum ActiveModalModel: Identifiable, Equatable {
    case macroEditor(MacroEditorModel)
    case multiEditor(MultiEditorModel)
    case lightingEditor(LightingEditorModel)
    case reconnectComparison(ReconnectComparisonModel)
    case applyPreparation(ApplyPreparationModel)

    public var id: String {
        switch self {
        case .macroEditor(let m): return "macro-\(m.macroId ?? "new")"
        case .multiEditor(let m): return "multi-\(m.multiId ?? "new")"
        case .lightingEditor(let m): return "lighting-\(m.layerId)"
        case .reconnectComparison(let m): return "reconnect-\(m.token)"
        case .applyPreparation(let m): return "apply-\(m.token)"
        }
    }
}

public struct CheatSheetCellModel: Identifiable, Equatable {
    public var id: String { bindingId }
    public let bindingId: String
    public let controlName: String
    public let label: String
    public let available: Bool
}

public struct CheatSheetModel: Equatable {
    public var profileName: String = ""
    public var routingSummary: String = ""
    public var agentName: String? = nil
    public var agentPhaseLabel: String? = nil
    public var agentVoiceSummary: String? = nil
    public var keyCells: [CheatSheetCellModel] = []
}

public struct HeaderSliceModel: Equatable {
    public var readyKind: String = "Ready"
    public var readyText: String = "已就绪"
    public var bannerKind: String = "Ready"
    public var bannerText: String = "就绪"
    public var showConnectButton: Bool = true
    public var showReturnToHostButton: Bool = false
    public var hostExecutionEnabled: Bool = false
    public var canEditDraft: Bool = true
    public var canUndo: Bool = false
    public var canRedo: Bool = false
    public var busy: Bool = false
    public var cheatSheetVisible: Bool = false
    public var deviceConnected: Bool = false
    public var deviceSummary: String = "Creator Micro 2 (303A:8298) · 离线草稿模式"
    public var documentRevision: Int = 0
    public var hostRevision: Int = 0
    public init() {}
}

public struct SidebarSliceModel: Equatable {
    public var sidebarRows: [SidebarRowModel] = []
    public var hardwareSlots: [HardwareProfileSlotModel] = []
    public var activeHardwareSlot: Int = 0
    public var selectedProfileId: String? = nil
    public var selectedDisplayName: String = "未选择方案"
    public var activeHostProfileId: String? = nil
    public var autoSwitchState: String = "Active"
    public var autoFollowEnabled: Bool = true
    public var showOtherProfiles: Bool = false
    public var editingDevice: Bool = false
    public var canMoveUp: Bool = false
    public var canMoveDown: Bool = false
    public var canDelete: Bool = false
    public var selectedLayerId: String? = nil
    public var layerTabs: [LayerTabModel] = []
    public var inspectorLayerName: String = "默认层"
    public var isProtectedLayer: Bool = false
    public var lastExportedPreset: String? = nil
    public init() {}

    public var selectedRawName: String {
        sidebarRows.first(where: { $0.isSelected })?.rawName ?? selectedDisplayName
    }
}

public struct CanvasSliceModel: Equatable {
    public var selectedLayerId: String? = nil
    public var selectedSurface: String = "Key0"
    public var selectedLogicalSlot: Int = 0
    public var selectedJoystickDirection: String = "North"
    public var bottomRowLayout: String = "split"
    public var showKeycapIcons: Bool = true
    public var showKeycapLabels: Bool = true
    public var encoderCcwLabel: String = "上滚"
    public var encoderCwLabel: String = "下滚"
    public var encoderPressLabel: String = "按压"
    public var encoderSelectedSlot: Int? = nil
    public var encoderRingGlowHex: String = "#14C76B"
    public var joystickModeLabel: String = "八向摇杆"
    public var joystickActiveDirection: String = "North"
    public var joystickIsSelected: Bool = false
    public var joystickSectors: [JoystickSectorModel] = []
    public var keycapCells: [KeycapCellModel] = []
    public var statusLedHex: String = "#14C76B"
    public var underglowHex: String = "#3B82F6"
    public var activeHardwareSlot: Int = 0
    public var touchLedStates: [Bool] = [true, true, true]
    public var activeHardwareSlotLabel: String = "插入 USB · Antigravity"
    public var protectedLock: ProtectedLockModel? = nil
    public var canEditBindings: Bool = true
    public var canAddLighting: Bool = true
    public var canExportOrCopyLayer: Bool = true
    public var deviceConnected: Bool = false
    public var hostExecutionEnabled: Bool = false
    public init() {}
}

public struct SessionStripSliceModel: Equatable {
    public var sessionSlots: [SessionSlotModel] = []
    public var completedPills: [SessionPillModel] = []
    public var workingPills: [SessionPillModel] = []
    public var attentionPills: [SessionPillModel] = []
    public var unreadCompletedCount: Int = 0
    public var workingCount: Int = 0
    public var topSixBoundAsSessionKeys: Bool = false
    public var canMutateSessionLayout: Bool = true
    public var simpleLighting: SimpleLightingModel = SimpleLightingModel()
    public init() {}
}

public struct InspectorSliceModel: Equatable {
    public var selectedLogicalSlot: Int = 0
    public var selectedSurface: String = "Key0"
    public var selectedJoystickDirection: String = "North"
    public var bottomRowLayout: String = "split"
    public var selectedSlotRole: String = "TaskKey"
    public var inspectorLayerName: String = "默认层"
    public var isProtectedLayer: Bool = false
    public var protectedLock: ProtectedLockModel? = nil
    public var editableInspector: EditableInspectorModel? = EditableInspectorModel()
    public init() {}
}

public struct StatusMenuSliceModel: Equatable {
    public var statusMenuIconTitle: String = "MI"
    public var activeProfileName: String = "Antigravity"
    public var routingModeSummary: String = "自动跟随前台软件"
    public var favorites2x2: [FavoriteSlotModel] = []
    public var hostExecutionEnabled: Bool = false
    public var autoFollowEnabled: Bool = true
    public var manualOverride: Bool = false
    public var radialMenuEnabled: Bool = false
    public var radialConfirmMode: String = "encoder_press"
    public var radialReadiness: String = "Ready"
    public var radialSlots: [RadialSlotConfigModel] = []
    public var cheatSheetVisible: Bool = false
    public var pendingImportName: String? = nil
    public var pendingImportSummary: String? = nil
    public var pendingExportScope: String? = nil
    public var presetError: String? = nil
    public init() {}
}

public struct OverlaySliceModel: Equatable {
    public var cheatSheetVisible: Bool = false
    public var cheatSheet: CheatSheetModel? = nil
    public var passiveToastTitle: String? = nil
    public var passiveToastSubtitle: String? = nil
    public init() {}
}

public struct FooterSliceModel: Equatable {
    public var statusMessage: String = "已就绪"
    public var footerStatusText: String = "就绪"
    public var footerLimitText: String = "长按说话上限 120 秒，响应超时 8 秒"
    public init() {}
}

public struct ProjectionModel: Equatable {
    public var readyKind: String = "Ready"
    public var readyText: String = "已就绪"
    public var bannerKind: String = "Ready"
    public var bannerText: String = "就绪"
    public var showConnectButton: Bool = true
    public var showReturnToHostButton: Bool = false
    public var hostExecutionEnabled: Bool = false
    public var canEditDraft: Bool = true
    public var canUndo: Bool = false
    public var canRedo: Bool = false
    public var busy: Bool = false

    public var sidebarRows: [SidebarRowModel] = []
    public var hardwareSlots: [HardwareProfileSlotModel] = []
    public var activeHardwareSlot: Int = 0
    public var selectedProfileId: String? = nil
    public var selectedDisplayName: String = "未选择方案"
    public var activeHostProfileId: String? = nil
    public var autoSwitchState: String = "Active"
    public var showOtherProfiles: Bool = false
    public var editingDevice: Bool = false
    public var canMoveUp: Bool = false
    public var canMoveDown: Bool = false
    public var canDelete: Bool = false
    public var footerStatusText: String = "就绪"
    public var footerLimitText: String = "长按说话上限 120 秒，响应超时 8 秒"

    public var selectedLayerId: String? = nil
    public var layerTabs: [LayerTabModel] = []
    public var selectedSurface: String = "Key0"
    public var selectedLogicalSlot: Int = 0
    public var selectedJoystickDirection: String = "North"
    public var bottomRowLayout: String = "split"
    public var showKeycapIcons: Bool = true
    public var showKeycapLabels: Bool = true
    public var encoderCcwLabel: String = "上滚"
    public var encoderCwLabel: String = "下滚"
    public var encoderPressLabel: String = "按压"
    public var encoderSelectedSlot: Int? = nil
    public var encoderRingGlowHex: String = "#14C76B"
    public var joystickModeLabel: String = "八向摇杆"
    public var joystickActiveDirection: String = "North"
    public var joystickIsSelected: Bool = false
    public var joystickSectors: [JoystickSectorModel] = []
    public var keycapCells: [KeycapCellModel] = []
    public var statusLedHex: String = "#14C76B"
    public var underglowHex: String = "#3B82F6"
    public var touchLedStates: [Bool] = [true, true, true]
    public var activeHardwareSlotLabel: String = "插入 USB · Antigravity"
    public var protectedLock: ProtectedLockModel? = nil
    public var canEditBindings: Bool = true
    public var canAddLighting: Bool = true
    public var canExportOrCopyLayer: Bool = true

    public var sessionSlots: [SessionSlotModel] = []
    public var completedPills: [SessionPillModel] = []
    public var workingPills: [SessionPillModel] = []
    public var attentionPills: [SessionPillModel] = []
    public var unreadCompletedCount: Int = 0
    public var workingCount: Int = 0
    public var topSixBoundAsSessionKeys: Bool = false
    public var canMutateSessionLayout: Bool = true
    public var simpleLighting: SimpleLightingModel = SimpleLightingModel()

    public var inspectorLayerName: String = "默认层"
    public var isProtectedLayer: Bool = false
    public var editableInspector: EditableInspectorModel? = EditableInspectorModel()

    public var statusMenuIconTitle: String = "MI"
    public var activeProfileName: String = "Antigravity"
    public var routingModeSummary: String = "自动跟随前台软件"
    public var favorites2x2: [FavoriteSlotModel] = []
    public var autoFollowEnabled: Bool = true
    public var manualOverride: Bool = false
    public var radialMenuEnabled: Bool = false
    public var radialConfirmMode: String = "encoder_press"
    public var radialReadiness: String = "Ready"
    public var radialSlots: [RadialSlotConfigModel] = []
    public var cheatSheetVisible: Bool = false
    public var cheatSheet: CheatSheetModel? = nil
    public var passiveToastTitle: String? = nil
    public var passiveToastSubtitle: String? = nil
    public var pendingImportName: String? = nil
    public var pendingImportSummary: String? = nil
    public var pendingExportScope: String? = nil
    public var presetError: String? = nil

    public var activeModal: ActiveModalModel? = nil
    public var statusMessage: String = "已就绪"
    public var lastExportedPreset: String? = nil
    public var deviceConnected: Bool = false
    public var deviceSummary: String = "Creator Micro 2 (303A:8298) · 离线草稿模式"
    public var documentRevision: Int = 0
    public var hostRevision: Int = 0

    public init() {}
}
