import Foundation

public enum UiLanguagePreference: String, CaseIterable, Equatable, Sendable {
    case system = "system"
    case zhHans = "zh-Hans"
    case en = "en"

    public func displayTitle(_ locale: ResolvedLocale) -> String {
        switch self {
        case .system:
            return locale == .en ? "Follow System" : "跟随系统"
        case .zhHans:
            return "简体中文"
        case .en:
            return "English"
        }
    }

    public func displaySubtitle(_ locale: ResolvedLocale) -> String {
        switch self {
        case .system:
            return locale == .en ? "Match macOS Preferred Languages automatically" : "自动匹配 macOS 系统首选语言"
        case .zhHans:
            return locale == .en ? "Always display Simplified Chinese" : "固定显示简体中文界面"
        case .en:
            return locale == .en ? "Always display English" : "固定显示英文界面"
        }
    }
}

public enum ResolvedLocale: String, Equatable, Sendable {
    case zhHans = "zh-Hans"
    case en = "en"
}

public enum HostGateReason: Equatable, Sendable {
    case qaVerifyMode
    case unbundledExecutable
    case ephemeralBuildBundle(bundlePath: String)
    case wrongBundleIdentifier(actual: String)
    case disabledByEnvironment(variable: String)
    case missingInfoPlistKey(key: String)
    case translocatedOrMountedVolume(bundlePath: String)
    case missingDriver
    case unsupportedOS

    public func localizedDescription(_ locale: ResolvedLocale) -> String {
        switch (self, locale) {
        case (.qaVerifyMode, .zhHans):
            return "QA 隔离验证模式已禁用此宿主服务"
        case (.qaVerifyMode, .en):
            return "Disabled in isolated QA verification mode"
        case (.unbundledExecutable, .zhHans):
            return "当前进程未从签名 .app 应用包启动"
        case (.unbundledExecutable, .en):
            return "Running outside a signed .app bundle"
        case (.ephemeralBuildBundle, .zhHans):
            return "当前运行在临时构建目录，请从 /Applications 启动正式版"
        case (.ephemeralBuildBundle, .en):
            return "Running from a temporary build directory; launch from /Applications"
        case (.wrongBundleIdentifier(let actual), .zhHans):
            return "应用包标识符不匹配（\(actual.isEmpty ? "未知" : actual)）"
        case (.wrongBundleIdentifier(let actual), .en):
            return "Bundle identifier mismatch (\(actual.isEmpty ? "unknown" : actual))"
        case (.disabledByEnvironment(let variable), .zhHans):
            return "已由环境变量 \(variable)=1 显式禁用"
        case (.disabledByEnvironment(let variable), .en):
            return "Disabled by environment variable \(variable)=1"
        case (.missingInfoPlistKey(let key), .zhHans):
            return "缺少或无效的 Info.plist 配置项：\(key)"
        case (.missingInfoPlistKey(let key), .en):
            return "Missing or invalid Info.plist key: \(key)"
        case (.translocatedOrMountedVolume, .zhHans):
            return "应用位于挂载卷或 App Translocation，请移至 /Applications 后再试"
        case (.translocatedOrMountedVolume, .en):
            return "App is translocated or on a mounted volume; move to /Applications"
        case (.missingDriver, .zhHans):
            return "当前宿主未挂载服务驱动"
        case (.missingDriver, .en):
            return "Host service driver is not attached"
        case (.unsupportedOS, .zhHans):
            return "当前 macOS 版本不支持此功能"
        case (.unsupportedOS, .en):
            return "Unsupported on this macOS version"
        }
    }
}

public enum L10n {
    public static func tr(_ zh: String, _ locale: ResolvedLocale) -> String {
        switch locale {
        case .zhHans:
            return zh
        case .en:
            return enTable[zh] ?? zh
        }
    }

    public static func destinationTitle(_ rawValue: String, _ locale: ResolvedLocale) -> String {
        guard locale == .en else { return rawValue }
        switch rawValue {
        case "轮盘菜单": return "Wheel Menu"
        case "灯光": return "Lighting"
        case "设备与连接": return "Device & Connection"
        case "应用方案": return "Profiles"
        case "通用": return "General"
        default: return rawValue
        }
    }

    public static func radialDirections(_ locale: ResolvedLocale) -> [String] {
        switch locale {
        case .zhHans:
            return ["右", "右下", "下", "左下", "左", "左上", "上", "右上"]
        case .en:
            return ["Right", "Bottom-Right", "Down", "Bottom-Left", "Left", "Top-Left", "Up", "Top-Right"]
        }
    }

    public static func layerNumber(_ layer: Int, _ locale: ResolvedLocale) -> String {
        locale == .en ? "Layer \(layer)" : "第 \(layer) 层"
    }

    public static func duplicateLayerName(_ base: String, _ locale: ResolvedLocale) -> String {
        locale == .en ? "\(base) Copy" : "\(base) 副本"
    }

    public static func customProfileTitle(_ index: Int, _ locale: ResolvedLocale) -> String {
        locale == .en ? "Custom Profile \(index)" : "自定义方案 \(index)"
    }

    public static func lightingLayerSubtitle(_ layerName: String, _ locale: ResolvedLocale) -> String {
        locale == .en
            ? "Adjust effect, color, and speed for “\(layerName)”"
            : "针对「\(layerName)」单独调节灯效模式、色彩与速度"
    }

    public static func inspectorSlotTitle(
        slot: Int,
        isSplit: Bool,
        selectedSurface: String,
        joystickMode: String?,
        locale: ResolvedLocale
    ) -> String {
        if selectedSurface == "EncoderHold" {
            return locale == .en ? "Knob Hold" : "旋钮长按"
        }
        if (16...19).contains(slot) || selectedSurface == "Joystick" {
            switch joystickMode {
            case "mouse":
                return locale == .en ? "Top-Right Stick (HW Mouse)" : "右上角摇杆（硬件鼠标）"
            case "scroll":
                return locale == .en ? "Top-Right Stick (HW Scroll)" : "右上角摇杆（硬件滚轮）"
            default:
                break
            }
        }
        if locale == .en {
            switch slot {
            case 0: return "Top-Left Key (Key 01)"
            case 1: return "Top-Right Key (Key 02)"
            case 2..<10: return String(format: "Key %02d", slot + 1)
            case 10: return isSplit ? "Bottom-Left Key (Switch 11)" : "Bottom Wide Key (2U, 11+12)"
            case 11: return isSplit ? "Bottom-Mid Key (Switch 12)" : "Bottom Wide Key (Right Switch)"
            case 12: return "Bottom-Right Key (Switch 13)"
            case 13: return "Knob Rotate Left (CCW)"
            case 14: return "Knob Rotate Right (CW)"
            case 15: return "Knob Press"
            case 16: return "4-Way Stick (Up)"
            case 17: return "4-Way Stick (Down)"
            case 18: return "4-Way Stick (Left)"
            case 19: return "4-Way Stick (Right)"
            default: return "Key \(slot + 1)"
            }
        }
        switch slot {
        case 0: return "左上键（按键 01）"
        case 1: return "右上键（按键 02）"
        case 2..<10: return String(format: "按键 %02d", slot + 1)
        case 10: return isSplit ? "底行左键（开关 11）" : "底部宽键（2U，开关 11+12）"
        case 11: return isSplit ? "底行中键（开关 12）" : "底部宽键右开关"
        case 12: return "底行右键（开关 13）"
        case 13: return "旋钮左旋"
        case 14: return "旋钮右旋"
        case 15: return "旋钮短按"
        case 16: return "四向摇杆（上）"
        case 17: return "四向摇杆（下）"
        case 18: return "四向摇杆（左）"
        case 19: return "四向摇杆（右）"
        default: return "按键 \(slot + 1)"
        }
    }

    public static func protectedBoundSummary(
        owner: String,
        selectedSurface: String,
        code: String,
        locale: ResolvedLocale
    ) -> String {
        if selectedSurface == "EncoderHold" {
            return locale == .en
                ? "\(owner) Knob Hold (Search Conversations)"
                : "\(owner) 旋钮长按（搜索会话）"
        }
        return locale == .en
            ? "\(owner) Native Passthrough (\(code))"
            : "\(owner) 原生直通 (\(code))"
    }

    public static func ambientLightingColors(_ locale: ResolvedLocale) -> [SelectOption<String>] {
        [
            .init(value: "OceanBlue", title: tr("海洋蓝", locale), subtitle: tr("默认完成与空闲蓝光", locale)),
            .init(value: "CyanTeal", title: tr("青碧", locale), subtitle: tr("冷静低干扰青绿光", locale)),
            .init(value: "PureWhite", title: tr("纯白", locale), subtitle: tr("纯净中性单色背光", locale)),
            .init(value: "EmeraldGreen", title: tr("翡翠绿", locale), subtitle: tr("柔和醒目的运行态绿光", locale)),
            .init(value: "AmberGold", title: tr("琥珀金", locale), subtitle: tr("温暖待确认提示暖光", locale)),
        ]
    }

    public static func workingLightingColors(_ locale: ResolvedLocale) -> [SelectOption<String>] {
        [
            .init(value: "EmeraldGreen", title: tr("翡翠绿", locale), subtitle: tr("柔和醒目的运行态绿光", locale)),
            .init(value: "AmberGold", title: tr("琥珀金", locale), subtitle: tr("温暖待确认提示暖光", locale)),
            .init(value: "CyanTeal", title: tr("青碧", locale), subtitle: tr("冷静低干扰青绿光", locale)),
            .init(value: "PureWhite", title: tr("纯白", locale), subtitle: tr("纯净中性单色背光", locale)),
            .init(value: "OceanBlue", title: tr("海洋蓝", locale), subtitle: tr("默认完成与空闲蓝光", locale)),
        ]
    }

    public static func brightnessOptions(_ locale: ResolvedLocale) -> [SelectOption<String>] {
        [
            .init(value: "Soft", title: tr("柔和 (35%)", locale)),
            .init(value: "Standard", title: tr("标准 (65%)", locale)),
            .init(value: "Bright", title: tr("明亮 (95%)", locale)),
        ]
    }

    public static func autoOffOptions(_ locale: ResolvedLocale) -> [SelectOption<String>] {
        [
            .init(value: "OneMinute", title: tr("1 分钟", locale)),
            .init(value: "ThreeMinutes", title: tr("3 分钟", locale)),
            .init(value: "FiveMinutes", title: tr("5 分钟", locale)),
            .init(value: "Never", title: tr("常亮", locale)),
        ]
    }

    public static let enTable: [String: String] = [
        "关于 Micro Input": "About Micro Input",
        "检查更新…": "Check for Updates…",
        "偏好设置…": "Settings…",
        "服务": "Services",
        "隐藏 Micro Input": "Hide Micro Input",
        "隐藏其他": "Hide Others",
        "全部显示": "Show All",
        "退出 Micro Input": "Quit Micro Input",
        "编辑": "Edit",
        "撤销": "Undo",
        "重做": "Redo",
        "剪切": "Cut",
        "拷贝": "Copy",
        "粘贴": "Paste",
        "全选": "Select All",
        "窗口": "Window",
        "最小化": "Minimize",
        "缩放": "Zoom",
        "前置全部窗口": "Bring All to Front",
        "关闭窗口": "Close Window",
        "打开 Micro Input 工作台": "Open Micro Input Workbench",
        "Micro Input 按键与会话控制": "Micro Input Key & Session Control",
        "Micro Input 菜单栏控制": "Micro Input Menu Bar",
        "打开主窗口": "Open Main Window",
        "禁用按键映射": "Pause Key Mapping",
        "启用按键映射": "Enable Key Mapping",
        "固件更新尚未结束": "Firmware Update in Progress",
        "设备仍在固件更新流程中，现在退出可能中断写入或丢失回读确认。": "The device is still updating firmware. Quitting now may interrupt flashing or lose readback verification.",
        "留在应用中": "Stay in App",

        "启动与语言": "Startup & Language",
        "界面语言": "Interface Language",
        "实时切换工作台、菜单栏弹层与按键速查的显示语言": "Switch Workbench, menu bar popover, and Cheat Sheet language live",
        "跟随系统": "Follow System",
        "登录时打开 Micro Input": "Launch Micro Input at Login",
        "开机登录后自动在后台驻留菜单栏并就绪键盘控制": "Start automatically on login and stay ready in the menu bar",
        "需要在「系统设置 → 通用 → 登录项」中批准后台运行": "Approval required in System Settings → General → Login Items",
        "打开登录项设置…": "Open Login Items…",
        "登录时静默驻留菜单栏": "Start Silently in Menu Bar at Login",
        "由系统登录项拉起时不弹出主窗口，仅驻留右上角菜单栏": "Keep main window closed on login launch; run quietly in the menu bar",
        "登录项变更失败：": "Login item update failed: ",
        "版本与更新": "Version & Updates",
        "版本与软件更新": "Version & Software Updates",
        "当前应用版本": "App Version",
        "应用版本": "App Version",
        "自动检查更新": "Automatically Check for Updates",
        "关于与开源": "About & Open Source",
        "开源许可证": "Open Source License",
        "源码仓库与更新日志": "Repository & Changelog",
        "查看公开代码仓库、构建说明与历史发行记录": "View source repository, build instructions, and release history",
        "GitHub 仓库": "GitHub Repo",
        "发行说明": "Release Notes",
        "正常启用": "Active",
        "上次检查：从未检查": "Last checked: Never",

        "新建自定义层": "New Custom Layer",
        "自定义层": "Custom Layer",
        "新建软件方案": "New Profile",
        "方案名称": "Profile Name",
        "创建": "Create",
        "重命名当前方案": "Rename Profile",
        "新方案名称": "New Profile Name",
        "新建按键层": "New Key Layer",
        "层名称": "Layer Name",
        "重命名当前按键层": "Rename Key Layer",
        "新层名称": "New Layer Name",
        "撤销上一步修改 (⌘Z)": "Undo last change (⌘Z)",
        "重做上一步修改 (⇧⌘Z)": "Redo last change (⇧⌘Z)",
        "隐藏速查": "Hide Sheet",
        "软件方案": "App Profiles",
        "新建配置…": "New Profile…",
        "重命名配置…": "Rename Profile…",
        "复制配置": "Duplicate Profile",
        "上移配置": "Move Up",
        "下移配置": "Move Down",
        "删除配置…": "Delete Profile…",
        "补齐内置软件方案": "Restore Built-in Agent Presets",
        "方案管理": "Manage",
        "导入预设文件…": "Import Preset File…",
        "导出整个本地文档…": "Export Full Local Document…",
        "导出当前配置…": "Export Current Profile…",
        "导出当前层…": "Export Current Layer…",
        "导入与备份": "Import & Backup",
        "方案与硬件层选项": "Profile & Hardware Layer Options",
        "显示其他自定义方案": "Show Other Custom Profiles",
        "在方案列表中一并展示原生保护方案与自定义草稿": "Include native protected profiles and custom drafts in the list",
        "同步切换键盘硬件层": "Sync Hardware Layer on Switch",
        "在桌面端切换或激活方案时，同步切换已连接键盘的硬件层": "Switch the connected keypad's hardware layer when changing profiles",
        "本地六层方案": "Local 6-Layer Plan",
        "默认": "Reset",
        "第 1 层保留 OAI 映射，USB 独占后由 Micro Input 当前方案控制；第 2 / 3 层为 Antigravity / DeepSeek Harness，4 / 5 / 6 层留空": "Layer 1 keeps OAI mapping (controlled by Micro Input over USB); Layers 2/3 map to Antigravity / DeepSeek Harness; Layers 4–6 stay unassigned",
        "由 Micro Input 控制": "Controlled by Micro Input",
        "USB 启动方案": "USB Startup Profile",
        "同步六层到小键盘": "Sync 6 Layers to Keypad",
        "托管层需要 Karabiner 保持运行。": "Managed layers require Karabiner to stay running.",
        "恢复原键盘配置": "Restore Original Config",
        "小键盘": "Keypad",
        "自定义": "Customize",
        "版本": "Version",
        "按键层灯光": "Layer Lighting",
        "按键层双区 RGB 灯效": "Per-Layer Dual-Zone RGB",
        "独立配置所选按键层的键帽背光与底盘光效参数。": "Configure keycap backlight and chassis underglow for the selected layer.",
        "先选择已分配的层或应用方案。": "Select an assigned layer or profile first.",
        "当前层由原生应用控制灯光，请在键盘页面选择可编辑的层。": "Lighting on this layer is controlled by its native app; select an editable layer.",
        "连接": "Connection",
        "当前设备状态": "Device Status",
        "查找键盘": "Find Keyboard",
        "物理层观测": "Observed Hardware Layer",
        "硬件键帽布局": "Keycap Layout",
        "底行按键形态": "Bottom Row Layout",
        "匹配物理拆分为两颗 1u 按键或合并为单颗 2u 宽键": "Match two split 1u keys or a single merged 2u wide key",
        "双 1u 分体": "Split (2 × 1u)",
        "单 2u 合并": "Merged (1 × 2u)",
        "诊断与支持": "Diagnostics & Support",
        "运行诊断归档": "Diagnostic Bundle",
        "导出脱敏连接与状态诊断快照，并在 Finder 中显示": "Export a redacted connection and state snapshot and reveal it in Finder",
        "为当前应用方案新增一个可自由编辑的按键层：": "Add a customizable key layer to the current profile:",
        "新建": "Create",
        "重命名当前层": "Rename Layer",
        "输入新的按键层名称：": "Enter a new layer name:",
        "保存": "Save",
        "取消": "Cancel",
        "图层名称": "Layer Name",
        "目标键盘": "Target Keyboard",
        "切换当前控制与同步的目标 Creator Micro": "Switch active Creator Micro device",
        "自动连接": "Auto Connect",
        "优先连接任意在线的 Creator Micro": "Connect to any available Creator Micro",
        "在线": "Online",
        "未连接": "Offline",
        "切换目标 Creator Micro 键盘": "Switch target Creator Micro",
        "按键映射": "Key Mapping",
        "按键速查": "Cheat Sheet",
        "键盘": "Keyboard",
        "应用方案": "Profiles",
        "管理": "Manage",
        "设置": "Settings",
        "已锁定": "Locked",
        "锁定": "Lock",
        "打开文件": "Reveal File",
        "拷贝配置": "Copy Config",
        "导出诊断": "Diagnostics",
        "重新扫描应用": "Rescan Apps",
        "选择或调整各桌面软件的按键方案，点击「编辑按键」进入键盘视图修改映射。": "Choose or reorder desktop app profiles. Click Edit Keys to customize bindings.",
        "默认兜底": "Fallback",
        "当前启用": "Active",
        "未安装": "Not Installed",
        "启用": "Enable",
        "上移": "Up",
        "下移": "Down",
        "编辑按键": "Edit Keys",
        "自定义方案": "Custom Profiles",
        "+ 新建自定义方案": "+ New Custom Profile",
        "当前正在使用此自定义方案": "Currently active custom profile",
        "手动切换可用的自定义按键方案": "Manually switchable custom key profile",
        "删除": "Delete",
        "点击右上角「新建自定义方案」可创建额外的独立按键层组": "Click '+ New Custom Profile' above to create an independent layer set",
        "编辑层灯光": "Edit Layer Lighting",
        "背光与底光细节": "Backlight & Underglow",
        "单层自定义背光与 RGB 底光参数": "Per-layer custom backlight and RGB underglow",
        "连接键盘": "Connect Keyboard",
        "当前连接": "Connection",
        "多设备优先目标": "Preferred Device",
        "插入多把键盘时固定优先连接所选序列号": "Prefer the selected serial number when multiple pads are connected",
        "预检与备份…": "Preflight & Backup…",
        "六层硬件槽位分配": "6-Layer Hardware Assignment",
        "恢复备份": "Restore Backup",
        "应用到键盘": "Apply to Keyboard",
        "USB 启动默认层": "USB Startup Layer",
        "键盘通过 USB 接入时的初始默认方案": "Initial profile when connected over USB",
        "未分配": "Unassigned",
        "正在使用": "In Use",
        "正常": "OK",
        "八向轮盘": "8-Way Wheel",
        "当前展示内置的固定八向快捷动作": "Built-in 8-way quick actions",
        "启用轮盘菜单": "Enable Wheel Menu",
        "移动摇杆选择，回到中心确认。": "Deflect stick to select; release to center to confirm.",
        "移动摇杆选择，按下旋钮确认；回到中心取消。": "Deflect stick to select, press knob to confirm; center to cancel.",
        "系统权限": "System Permissions",
        "辅助功能权限": "Accessibility Permission",
        "已授权，可正常向目标软件发送快捷键与指令": "Authorized to send shortcuts and commands to target apps",
        "缺少 macOS 辅助功能权限，软件按键映射将被系统拦截": "Missing macOS Accessibility permission; key mappings will be blocked",
        "已就绪": "Ready",
        "打开辅助功能设置": "Open Accessibility Settings",
        "确认导入预设预览": "Review Staged Preset Import",
        "预设名称": "Preset Name",
        "导入来源范围": "Import Scope",
        "单个应用方案": "Single Profile",
        "单个按键层": "Single Layer",
        "整份本地文档": "Full Local Document",
        "覆盖与权限提醒": "Draft & Execution Notice",
        "确认导入将替换当前本地草稿，不会自动写入键盘硬件或启用命令执行。": "Confirming replaces your current local draft without writing to hardware or enabling shell execution.",
        "完整归档来源提示": "Complete Source Archive Notice",
        "该预设文件内包含完整源文档归档，请仅导入来自可信来源的预设文件。": "This preset embeds a complete source document archive. Only import presets from trusted sources.",
        "确认导入草稿": "Import to Draft",
        "键盘层": "Board Layers",
        "使用中": "Active",

        "当前控制": "Active Control",
        "已暂停软件按键映射": "Key Mapping Paused",
        "新建层": "New Layer",
        "复制当前层": "Duplicate Layer",
        "删除当前层": "Delete Layer",
        "按键层": "Layers",
        "启用此方案": "Activate Profile",
        "已锁定（点击解锁）": "Locked (Click to Unlock)",
        "锁定此方案": "Lock Profile",
        "打开应用": "Open App",
        "这一层尚未分配应用": "No Profile Assigned to This Layer",
        "在应用方案中分配后，即可查看与编辑对应按键。": "Assign a profile in Profiles to view and edit its keys.",
        "授权辅助功能": "Grant Accessibility",
        "重新检测键盘": "Detect Keyboard",
        "开启按键映射": "Enable Key Mapping",
        "左旋": "CCW",
        "右旋": "CW",
        "长按": "Hold",
        "短按": "Press",
        "鼠标": "Mouse",
        "滚轮": "Scroll",
        "本机方案，没有选择物理层": "Local profile; no hardware layer selected",
        "本机": "Local",
        "预览": "Preview",
        "方案": "Profile",
        "原生直通": "Native",
        "空白": "Blank",
        "显示键帽文字": "Show Keycap Labels",
        "点击发光键打开会话": "Tap Glowing Key to Open Session",
        "上方六键设为会话键": "Set Top 6 Keys as Session Keys",
        "恢复默认布局": "Restore Default Layout",
        "模型选择（直旋模型 · 按下搜会话）": "Model Picker & Search (Turn Model · Press Search)",
        "模型选择（短按模型 · 长按搜会话）": "Model Picker & Search (Tap / Hold)",
        "旋钮切换会话，摇杆滚动对话": "Knob Switches Sessions, Stick Scrolls Chat",
        "旋钮滚动项目，摇杆滚动对话": "Knob Scrolls Projects, Stick Scrolls Chat",
        "旋钮滚动对话": "Knob Scrolls Chat",
        "旋钮调整推理档位": "Knob Adjusts Reasoning Effort",
        "摇杆鼠标 + 旋钮滚轮": "Stick Mouse + Knob Scroll",
        "全部标为已读": "Mark All as Read",
        "布局选项": "Layout Options",
        "海洋蓝": "Ocean Blue",
        "默认完成与空闲蓝光": "Default completed & idle blue glow",
        "青碧": "Cyan Teal",
        "冷静低干扰青绿光": "Calm low-distraction cyan glow",
        "纯白": "Pure White",
        "纯净中性单色背光": "Clean neutral monochrome backlight",
        "翡翠绿": "Emerald Green",
        "柔和醒目的运行态绿光": "Distinctive running green glow",
        "琥珀金": "Amber Gold",
        "温暖待确认提示暖光": "Warm attention-needed amber glow",
        "柔和 (35%)": "Soft (35%)",
        "标准 (65%)": "Standard (65%)",
        "明亮 (95%)": "Bright (95%)",
        "1 分钟": "1 Min",
        "3 分钟": "3 Min",
        "5 分钟": "5 Min",
        "常亮": "Always On",
        "会话灯光": "Session Lighting",
        "开启灯光": "Enable Lighting",
        "启用小键盘会话状态灯与环境底光反馈": "Enable session status LEDs and ambient underglow",
        "随会话状态变色": "Color by Session Status",
        "按进行中、待确认与已完成状态自动切换灯光颜色": "Switch LED colors for running, awaiting input, and completed states",
        "进行中的会话使用呼吸灯": "Breathing Effect While Running",
        "任务运行期间以柔和呼吸灯效提示后台进展": "Pulse softly while background tasks are running",
        "仅在有新完成或进行中的会话时亮起": "Light Only When Active or Unread",
        "无未读或运行中任务时保持安静熄灯": "Stay dark when no sessions are running or unread",
        "单键状态灯同步": "Per-Key Status RGB Sync",
        "按会话状态自动映射自定义单键灯效，退出或休眠时自动恢复默认": "Sync per-key RGB to session status and restore defaults on sleep or quit",
        "灯效选项": "Lighting Preferences",
        "完成与空闲时的颜色": "Completed & Idle Color",
        "会话完成待检视或空闲待机时的默认灯光": "Default LED color when completed or idle",
        "进行中时的颜色": "Running Color",
        "后台代理持续生成或执行工具时的灯光": "LED color while an agent is generating or running tools",
        "整体亮度": "Brightness",
        "调节 Creator Micro 会话灯光的整体亮度": "Adjust overall Creator Micro session LED brightness",
        "自动熄灯": "Auto-Off Timeout",
        "无操作后自动熄灭灯光，按键或会话变化时自动亮起": "Turn off LEDs after inactivity; wake on key press or session update",
        "在键盘上预览": "Preview on Pad",
        "恢复自动": "Auto",
        "完成与空闲": "Completed",
        "进行中": "Running",
        "待确认": "Attention",
        "未读提醒": "Unread Alerts",
        "临时点亮不同会话状态灯以核对色彩层次": "Preview session LED states directly on the keypad",
        "未读完成灯状态": "Unread Completed LEDs",
        "清除所有已完成会话的未读蓝灯提示": "Clear unread blue LED indicators across completed sessions",
        "尚未分配会话": "Unassigned Session",
        "未绑定": "Unbound",
        "会话": "Sessions",
        "已读熄灯": "Clear Unread",
        "灯光筛选": "Lighting",
        "已完成": "Completed",
        "待回应": "Needs Input",
        "执行异常": "Error",
        "就绪": "Ready",
        "状态未知": "Unknown",

        "会话键": "Session Key",
        "动作键": "Action Key",
        "更多动作…": "More Actions…",
        "绑定宏…": "Bind Macro…",
        "按键用途": "Key Role",
        "会话键（自动跟灯）": "Session Key (Auto LED)",
        "普通动作键": "Action Key",
        "拆分为两个按键（1U + 1U）": "Split into Two Keys (1U + 1U)",
        "底行按键": "Bottom Key",
        "左键（开关 11）": "Left (Switch 11)",
        "中键（开关 12）": "Mid (Switch 12)",
        "无极旋钮动作方向": "Rotary Knob Action",
        "旋钮方向": "Knob Direction",
        "滚动对话预设": "Scroll Chat Preset",
        "调整推理档位": "Reasoning Effort",
        "切活跃/蓝点会话": "Cycle Active/Unread",
        "双滚轮预设": "Dual Scroll Preset",
        "摇杆驱动模式": "Joystick Mode",
        "宿主逻辑扇区": "Host Sectors",
        "硬件 MOUSE": "HW MOUSE",
        "硬件 SCROLL": "HW SCROLL",
        "保留来源": "Keep Source",
        "0.7.0 硬件鼠标指针": "0.7.0 Hardware Mouse Pointer",
        "0.7.0 硬件模拟滚轮": "0.7.0 Hardware Scroll Wheel",
        "键盘当前层已同步此硬件模式": "Synced to active hardware layer",
        "待写入当前键盘层固件": "Pending write to active hardware layer",
        "离线草稿已保存；接入 0.7.0+ 固件后可一键写入键盘": "Saved offline; connect 0.7.0+ firmware to write to pad",
        "重新写入固件": "Rewrite Firmware",
        "写入键盘固件": "Write to Pad",
        "速度档位": "Speed Preset",
        "慢速": "Slow",
        "标准": "Standard",
        "快速": "Fast",
        "精准": "Precise",
        "摇杆全层固定为纯硬件高速鼠标指针，任何时候推动摇杆均直接高速移动光标，不拦截或抢占方向。": "The stick acts as a pure high-speed hardware mouse pointer across all layers without directional interception.",
        "旋转左上角旋钮弹出模型选择页期间，向右轻推摇杆（按右）将临时拦截指针并展开 / 调高模型力度，短按确认后自动恢复硬件指针。": "While rotating the top-left knob for Model Picker, nudging the stick right temporarily adjusts reasoning effort; press to confirm and restore hardware pointer.",
        "短按左上角旋钮弹出模型选择页期间，向右轻推摇杆（按右）将临时拦截指针并展开 / 调高模型力度，短按确认后自动恢复硬件指针。": "While tapping the top-left knob for Model Picker, nudging the stick right temporarily adjusts reasoning effort; press to confirm and restore hardware pointer.",
        "长按左上角旋钮弹出模型选择页期间，向右轻推摇杆（按右）将临时拦截指针并展开 / 调高模型力度，短按确认后自动恢复硬件指针。": "While holding the top-left knob for Model Picker, nudging the stick right temporarily adjusts reasoning effort; press to confirm and restore hardware pointer.",
        "恢复键盘扇区模式": "Restore Sector Mode",
        "四向摇杆方向": "4-Way Stick Direction",
        "摇杆方向": "Stick Direction",
        "上": "Up",
        "左": "Left",
        "下": "Down",
        "右": "Right",
        "滚对话 + 左右切会话": "Scroll Chat + Switch Sessions",
        "自定义快捷键映射": "Custom Shortcut",
        "设为 Fn 听写键": "Bind Apple Fn",
        "设为空键": "Clear Key",
        "录制：": "Record:",
        "点击录制键盘快捷键…": "Click to record keyboard shortcut…",
        "实时会话槽位（自动跟灯）": "Live Session Slot (Auto LED)",
        "等待关联活跃会话": "Waiting for active session",
        "立即跳转到此会话": "Jump to Session",
        "标为已读熄灯": "Mark Read & Clear LED",
        "键帽丝印图标": "Keycap Glyph",
        "搜索图标…": "Search glyphs…",
        "常用功能动作": "Common Actions",
        "搜索动作名称或快捷键…": "Search action or shortcut…",
        "已启用": "Bound",
        "全部": "All",
        "搜索动作名称…": "Search actions…",
        "已绑定": "Bound",
        "绑定": "Bind",
        "键盘宏与四分支动作": "Macros & Multi-Tap Actions",
        "+ 新建宏…": "+ New Macro…",
        "+ 新建四分支…": "+ New Multi-Tap…",
        "键盘宏序列": "Macro Sequences",
        "暂无已保存的键盘宏": "No saved macros",
        "点击右上角「新建宏」录制或编写按键序列": "Click '+ New Macro…' above to create a key sequence",
        "四分支复用动作 (Multi)": "Multi-Tap Actions (Multi)",
        "暂无四分支复用动作": "No multi-tap actions",
        "支持同一按键区分单击、双击、长按与单击后按住": "Bind single tap, double tap, hold, and tap-hold to one key",

        "切换目标键盘": "Switch Target Keyboard",
        "切换应用方案": "Switch Profile",
        "当前": "Active",
        "缺少 macOS 辅助功能权限": "Missing macOS Accessibility permission",
        "授权": "Grant",
        "退出": "Quit",
        " · 实时连接": " · Live",
        "关闭 (Esc)": "Close (Esc)",
        "旋钮": "Knob",
        "摇杆": "Stick",
        "展开": "Expand",
        "左上键": "Top-Left",
        "右上键": "Top-Right",
        "按键 03": "Key 03",
        "按键 04": "Key 04",
        "按键 05": "Key 05",
        "按键 06": "Key 06",
        "按键 07": "Key 07",
        "按键 08": "Key 08",
        "按键 09": "Key 09",
        "按键 10": "Key 10",
        "底行左键": "Bottom-Left",
        "底行中键": "Bottom-Mid",
        "底部宽键 (2U)": "Bottom Wide (2U)",
        "底行右键": "Bottom-Right",
        "无极旋钮": "Rotary Knob",
        "硬件摇杆": "HW Stick",
        "四向摇杆": "4-Way Stick",
        "鼠标指针": "Pointer",
        "物理坐标对齐": "Spatial 4×4",
        "已连接": "Connected",
        "上次优先": "Preferred",
        "可用": "Available",
        "设置…": "Settings…",
        "打开主配置台": "Open Workbench",
        "通用设置…": "General Settings…",
        "暂停按键映射": "Pause Key Mapping",
        "设备固件更新尚未结束": "Firmware Update in Progress",
        "正在准备、备份或写入设备。请保持 USB 连接，等待当前操作完成后再退出。": "Preparing, backing up, or flashing the device. Keep USB connected until the current step finishes.",
        "继续等待": "Keep Waiting",
        "全部可用动作库": "All Available Actions",
        "已锁定 · 已暂停软件按键映射": "Locked · Key Mapping Paused",
        "缺少 macOS 辅助功能权限，按键动作已被系统拦截": "Missing macOS Accessibility permission; key actions are blocked",
        "锁定当前方案": "Lock Active Profile",
        "固定响应当前方案，屏蔽前台切换与层漂移": "Stay on the current profile and ignore foreground app or layer switches",
        "常用软件": "Favorites",
        "状态灯效": "Status Lighting",
        "当前未选择可展示的应用方案": "No profile selected",
        "新建键盘宏序列": "New Macro Sequence",
        "编辑键盘宏序列": "Edit Macro Sequence",
        "宏名称": "Macro Name",
        "输入宏名称…": "Macro name…",
        "添加单击步骤": "Add Key Tap",
        "输入键名（如 a, return, space）与间隔毫秒数": "Enter key name (e.g. a, return, space) and delay in ms",
        "键名": "Key",
        "+ 添加": "+ Add",
        "清空": "Clear",
        "尚未添加步骤": "No steps added yet",
        "在上方输入键名后点击「添加」生成敲击与等待序列": "Enter a key above and click '+ Add' to build the sequence",
        "保存后立即绑定到当前选中按键": "Bind to selected key on save",
        "保存宏序列": "Save Macro",
        "不设置": "None",
        "清空此触发分支动作": "Clear this multi-tap branch",
        "新建四分支复用动作": "New Multi-Tap Action",
        "编辑四分支复用动作": "Edit Multi-Tap Action",
        "动作名称": "Action Name",
        "四分支动作名称…": "Multi-tap action name…",
        "双击判定窗口 (ms)": "Multi-Tap Window (ms)",
        "区分单击与连续双击的时间窗口": "Timing window separating single tap and double tap",
        "保存后立即绑定到当前按键": "Bind to selected key on save",
        "保存四分支动作": "Save Multi-Tap",
        "关闭 (Off)": "Off",
        "完全熄灭当前分区的硬件 LED": "Turn off hardware LEDs in this zone",
        "常亮 (Solid)": "Solid",
        "保持均匀稳定的静态单色背光": "Steady single-color illumination",
        "渐变 (Gradient)": "Gradient",
        "柔和过渡的环境双色渐变光效": "Smooth dual-color ambient gradient",
        "追光 (Snake)": "Snake",
        "沿键位轨迹依次流动的动态光效": "Dynamic trail sweeping across keys",
        "光谱 (Rainbow)": "Rainbow",
        "连续循环的全彩柔和色相过渡": "Continuous full-spectrum hue cycle",
        "呼吸 (Breath)": "Breath",
        "随节奏明暗起伏的低干扰呼吸光": "Gentle rhythmic breathing pulse",
        "同步应用到所有可编辑层": "Apply to All Editable Layers",
        "尚未启用自定义硬件 RGB 配置": "Custom Hardware RGB Not Initialized",
        "初始化后可独立配置当前层的键帽背光与底盘环绕光效。": "Initialize to configure per-layer keycap backlight and chassis underglow.",
        "初始化默认双区灯效": "Initialize Dual-Zone RGB",
        "重置当前层灯效": "Reset Layer Lighting",
        "完成": "Done",
        "关闭": "Close",
        "灯效模式": "Effect Mode",
        "选择此分区的发光方式": "Choose the lighting effect for this zone",
        "颜色": "Color",
        "选择静态背光或动态呼吸主色": "Choose primary color for static or breathing mode",
        "亮度": "Brightness",
        "调节此分区的整体发光亮度": "Adjust overall zone brightness",
        "速度": "Speed",
        "调节呼吸或追光灯效的循环速率": "Adjust breathing or chase animation speed",
        "渐变跨度": "Gradient Span",
        "调节渐变跨度与动态光效细节": "Adjust gradient span and dynamic effect detail",
        "设备重连三方比较": "Reconnect 3-Way Comparison",
        "检测到 USB 接口路径变化": "USB Port Path Changed",
        "请先确认当前连接设备与原基线为同一台 Creator Micro。": "Confirm the connected device is the same Creator Micro as the baseline.",
        "确认是同一台设备": "Confirm Same Device",
        "配置完全一致": "Configurations Match",
        "基线、本地草稿与设备端配置无差异": "Baseline, local draft, and device config are identical",
        "保留本地草稿": "Keep Local Draft",
        "采用设备配置": "Use Device Config",
        "自动合并": "Auto Merge",
        "配置应用预检与备份": "Apply Preflight & Backup",
        "暂无待应用变更": "No pending changes",
        "当前本地草稿与基线一致": "Local draft matches baseline",
        "1. 生成本地回滚备份": "1. Create Rollback Backup",
        "2. 确认候选配置": "2. Confirm Candidate Config",
        "未知（尚未从设备读取）": "Unknown (not read from device yet)",
        "检查 Creator Micro 2 官方稳定版固件": "Check official stable firmware for Creator Micro 2",
        "准备更新设备固件？": "Prepare Device for Firmware Update?",
        "确认安装到所选 Creator Micro 2？": "Confirm Flashing Selected Creator Micro 2?",
        "暂停控制并准备": "Pause Control & Prepare",
        "确认设备并安装": "Confirm Device & Install",
        "设备固件": "Device Firmware",
        "连接有线键盘": "Connect USB Keyboard",
        "检查固件更新": "Check Firmware",
        "更新通道": "Update Channel",
        "预发布固件与现有键位和配置协议的兼容性尚未验证。": "Prerelease compatibility with existing keymaps and config protocol is unverified.",
        "只检查官方稳定版。": "Check official stable releases only.",
        "稳定版": "Stable",
        "预发布版": "Prerelease",
        "版本比较": "Version Status",
        "下载完成，官方 SHA-256 摘要核对通过。": "Downloaded and verified against official SHA-256 digest.",
        "官方未提供摘要；可下载查看，不能自动安装。": "No official SHA-256 digest provided; manual inspection only.",
        "从 Work Louder 官方 GitHub 发布下载。": "Download from official Work Louder GitHub Releases.",
        "重新下载": "Redownload",
        "下载并校验": "Download & Verify",
        "准备设备": "Prepare Device",
        "按键映射已暂停。选择这把键盘对应的引导模式串口。": "Key mapping paused. Select the bootloader serial port for this keypad.",
        "会暂停按键映射并请求进入引导模式，键盘暂时不可用。": "Pauses key mapping and enters bootloader mode; keypad will be temporarily inactive.",
        "准备更新": "Prepare Update",
        "引导模式设备": "Bootloader Device",
        "Espressif 名称不能确认键盘型号。请确认所选串口属于这把 Creator Micro 2。": "Espressif USB names do not identify keypad models. Confirm this port belongs to your Creator Micro 2.",
        "请选择设备串口": "Select Serial Port",
        "查找串口": "Scan Ports",
        "安装固件": "Install Firmware",
        "先保存完整 Flash 备份，再写入并校验。期间请勿断开 USB 或退出应用。": "Saves a full Flash backup before writing and verifying. Do not unplug USB or quit.",
        "安装固件…": "Install Firmware…",
        "写入校验通过": "Flash Verified",
        "更新状态": "Update Status",
        "结束与备份": "Finish & Backup",
        "完成后需重新连接并核对运行版本与原键位；写入校验不代表运行验收。": "Reconnect after flashing to verify the running firmware version and keymap.",
        "显示备份": "Reveal Backup",
        "结束更新": "Finish Update",
        "Micro Input 将暂停宿主按键映射并请求键盘进入引导模式，键盘会暂时停止工作。下载的固件不会在此步骤写入。": "Micro Input will pause host key mapping and request bootloader mode. Downloaded firmware is not flashed in this step.",
        "请确认步进在 1–127、加速上限在 1–100、摇杆参数在 1–255": "Ensure deltas are 1–127, max speeds 1–100, and stick values 1–255",
        "需连接 0.7.0-rc.16+ 独占固件设备": "Requires a connected 0.7.0-rc.16+ device over USB",
        "宿主扇区": "Host Sectors",
        "设备识别与状态同步": "Device Identity & Sync",
        "刷新状态": "Refresh",
        "设备名称": "Device Name",
        "应用名称": "Apply Name",
        "状态同步": "Sync Status",
        "异常": "Error",
        "已同步": "Synced",
        "待同步": "Unsynced",
        "已记忆设备序列号": "Remembered Serial Number",
        "多台 Creator Micro 2 同时接入时将优先匹配此设备": "Prefer this device when multiple Creator Micro 2 units are connected",
        "鼠标速度与摇杆死区": "Mouse Speed & Stick Deadzone",
        "速度参数仅对绑定了硬件鼠标或滚轮的按键层生效；摇杆死区同时影响轮盘菜单离中阈值。": "Speed settings apply to hardware mouse/scroll layers; deadzone also governs Wheel Menu deflection.",
        "摇杆固件模式": "Stick Firmware Mode",
        "速度预设": "Speed Preset",
        "摇杆死区": "Stick Deadzone",
        "防漂移盲区（出厂默认 0.15，UI 范围 0.02–0.45）": "Anti-drift deadzone (factory default 0.15, range 0.02–0.45)",
        "保存死区": "Save Deadzone",
        "高级曲线参数": "Advanced Curve Parameters",
        "指针初速": "Move Delta",
        "按键鼠标单步位移（推荐 4–32，固件上限 127）": "Per-tick pointer step (recommended 4–32, max 127)",
        "指针上限": "Max Speed",
        "按键鼠标加速上限（推荐 4–40，固件上限 100）": "Pointer acceleration cap (recommended 4–40, max 100)",
        "滚轮步进": "Wheel Delta",
        "旋钮或按键单步滚动（推荐 1–16，固件上限 127）": "Per-tick scroll step (recommended 1–16, max 127)",
        "滚轮上限": "Wheel Max",
        "旋钮或按键滚轮上限（推荐 1–32，固件上限 100）": "Scroll acceleration cap (recommended 1–32, max 100)",
        "摇杆指针": "Stick Pointer",
        "模拟摇杆满偏转步长（推荐 1–16，固件上限 255）": "Full-deflection stick pointer step (recommended 1–16, max 255)",
        "摇杆滚轮": "Stick Scroll",
        "模拟摇杆滚轮分频间隔（10–120，越小越快；上限 255）": "Stick scroll divider interval (10–120, lower is faster; max 255)",
        "自定义参数": "Custom Curve",
        "仅将上方 6 项自定义曲线参数写入设备": "Write the 6 custom curve parameters above to the device",
        "应用鼠标参数": "Apply Curve",
        "休眠跟随与电源策略": "Sleep & Power Policy",
        "自动休眠": "Auto Sleep",
        "空闲达到设定时长后自动进入低功耗待机": "Enter low-power standby after the idle timeout",
        "跟随宿主休眠": "Sleep with Host",
        "随 Mac USB 挂起与唤醒动态调整设备电源地板": "Adjust device power floor with Mac USB suspend and wake",
        "休眠等待时长": "Idle Sleep Timeout",
        "Mac 正在休眠，扩展硬件配置已暂停": "Mac is sleeping; extended hardware controls paused",
        "Mac 已唤醒，正在重连设备": "Mac woke up; reconnecting device",
        "扩展硬件配置暂不可用": "Extended Hardware Controls Unavailable",
        "需通过 USB 连接支持扩展控制的 Creator Micro 2 固件。": "Connect a Creator Micro 2 running extended-control firmware over USB.",
        "宿主休眠": "Host Asleep",
        "唤醒同步中": "Waking",
        "宿主清醒": "Host Awake",
        "出厂名称未知": "Factory name unknown",
        "硬件指针 (MOUSE)": "HW Pointer (MOUSE)",
        "硬件滚轮 (SCROLL)": "HW Scroll (SCROLL)",
        "检测到主机睡眠，已交还键盘控制权": "Host sleep detected; released keyboard control",
        "主机唤醒中，正在等待设备重连": "Host waking; waiting for device reconnect",
        "USB 设备": "USB Device",
        "共同值": "Common",
        "设备": "Device",
        "本地": "Local",
        "冲突": "Conflict",
    ]
}
