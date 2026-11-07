import SwiftUI

enum KeycapRole: String, CaseIterable, Identifiable {
    case taskKey = "task"
    case actionKey = "action"

    var id: String { rawValue }

    var title: String {
        switch self {
        case .taskKey: return "会话键"
        case .actionKey: return "动作键"
        }
    }
}

enum ControlSlot: Equatable {
    case topRowKey(Int)
    case actionRowKey(Int)
    case bottomWideKey(Int)
    case rotaryEncoder(Int)
    case joystickSector(Int)

    init(rawSlot: Int) {
        switch rawSlot {
        case 0..<6:
            self = .topRowKey(rawSlot)
        case 6..<10, 12:
            self = .actionRowKey(rawSlot)
        case 10, 11:
            self = .bottomWideKey(rawSlot)
        case 13..<16:
            self = .rotaryEncoder(rawSlot)
        default:
            self = .joystickSector(rawSlot)
        }
    }

    var rawSlot: Int {
        switch self {
        case .topRowKey(let s),
             .actionRowKey(let s),
             .bottomWideKey(let s),
             .rotaryEncoder(let s),
             .joystickSector(let s):
            return s
        }
    }

    var isKeycap: Bool {
        rawSlot < 13
    }

    var supportsTaskRole: Bool {
        (0..<6).contains(rawSlot)
    }
}

enum KeycapPresentationResolver {
    static func isSessionSlotAction(_ action: CatalogAction?) -> Bool {
        guard let action else { return false }
        let cmd = action.payload["command"]?.text ?? ""
        if cmd.hasPrefix("slot-open:") || cmd.hasPrefix("KV_OAI_AG") {
            return true
        }
        return action.name.hasPrefix("KV_OAI_AG")
    }

    static func resolveRole(
        slot: Int,
        agent: AgentProfileKind,
        boundAction: CatalogAction?,
        rawBindingLabel: String = "",
        isProtectedLayer: Bool = false
    ) -> KeycapRole {
        guard (0..<6).contains(slot) else {
            return .actionKey
        }
        guard agent.supportsLiveSessionSlots else {
            return .actionKey
        }
        if isProtectedLayer {
            return .taskKey
        }
        if let action = boundAction {
            return isSessionSlotAction(action) ? .taskKey : .actionKey
        }
        if rawBindingLabel.contains("KV_OAI_AG") {
            return .taskKey
        }
        if !rawBindingLabel.isEmpty && rawBindingLabel != "未分配" {
            return .actionKey
        }
        return agent.topSixDefaultRole
    }

    static func sanitizeShortLabel(_ raw: String, maxChars: Int = 4) -> String {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        switch trimmed {
        case "按住说话，松开停止", "按住说话 · 松开停止", "按住说话":
            return String("按住说话".prefix(maxChars))
        case "发送消息（先停听写）", "发送并提交", "停止听写并提交":
            return String("发送提交".prefix(maxChars))
        case "打开 / 聚焦应用":
            return String("打开应用".prefix(maxChars))
        case "聚焦当前输入框", "聚焦输入框":
            return String("输入框".prefix(maxChars))
        case "打开内置浏览器":
            return String("内置浏览".prefix(maxChars))
        case "打开终端面板":
            return String("终端面板".prefix(maxChars))
        case "快速向右分栏", "向右分屏":
            return String("向右分栏".prefix(maxChars))
        case "Gemini Live 实时语音":
            return String("实时语音".prefix(maxChars))
        case "选择 Space / Tab":
            return String("切换标签".prefix(maxChars))
        case "搜索全部任务":
            return String("搜索任务".prefix(maxChars))
        case "在项目中向上滚动":
            return String("上滚项目".prefix(maxChars))
        case "在项目中向下滚动":
            return String("下滚项目".prefix(maxChars))
        case "切换到上一项目", "上一项目":
            return String("上一项目".prefix(maxChars))
        case "切换到下一项目", "下一项目":
            return String("下一项目".prefix(maxChars))
        case "上一个 Working 会话":
            return String("上一运行".prefix(maxChars))
        case "下一个 Working 会话":
            return String("下一运行".prefix(maxChars))
        case "上一个蓝点已完成会话":
            return String("上一蓝点".prefix(maxChars))
        case "下一个蓝点已完成会话":
            return String("下一蓝点".prefix(maxChars))
        case "上一个 Working 或蓝点会话":
            return String("上一待办".prefix(maxChars))
        case "下一个 Working 或蓝点会话":
            return String("下一待办".prefix(maxChars))
        default:
            break
        }

        if trimmed.hasPrefix("打开 / 聚焦 ") {
            let suffix = trimmed.replacingOccurrences(of: "打开 / 聚焦 ", with: "")
            let combined = "打开\(suffix)".replacingOccurrences(of: " · ", with: "").replacingOccurrences(of: "·", with: "")
            return String(combined.prefix(maxChars))
        }

        var cleaned = trimmed
            .replacingOccurrences(of: " · ", with: "")
            .replacingOccurrences(of: "·", with: "")

        if let range = cleaned.range(of: #"^\d+"#, options: .regularExpression) {
            cleaned.removeSubrange(range)
        }
        cleaned = cleaned.trimmingCharacters(in: .whitespacesAndNewlines)
        return String(cleaned.prefix(maxChars))
    }

    static func resolveCaption(
        slot: Int,
        role: KeycapRole,
        actionDisplayName: String,
        liveSessionTitle: String? = nil
    ) -> String {
        if role == .taskKey && (0..<6).contains(slot) {
            if let title = liveSessionTitle?.trimmingCharacters(in: .whitespacesAndNewlines), !title.isEmpty {
                return sanitizeShortLabel(title, maxChars: 4)
            }
            return "会话 \(slot + 1)"
        }
        return sanitizeShortLabel(actionDisplayName, maxChars: 4)
    }
}

enum CodexKeycapId: String, CaseIterable, Identifiable {
    case blank = "blank"
    case fast = "fast"
    case approve = "approve"
    case decline = "decline"
    case split = "split"
    case mic = "mic"
    case codex = "codex"
    case bug = "bug"
    case openai = "openai"
    case term = "term"
    case download = "download"
    case delete = "delete"
    case new = "new"
    case nav = "nav"
    case magic = "magic"
    case diff = "diff"
    case play = "play"
    case git = "git"
    case branchAlt = "branch-alt"
    case branch = "branch"
    case merge = "merge"
    case pr = "pr"
    case paint = "paint"
    case lab = "lab"
    case party = "party"
    case time = "time"
    case mindPlus = "mind-plus"
    case mindMinus = "mind-minus"
    case setup = "setup"
    case folder = "folder"
    case upload = "upload"
    case apps = "apps"
    case yolo = "yolo"
    case yeet = "yeet"

    var id: String { rawValue }

    var silkCode: String {
        switch self {
        case .blank: return "BLANK"
        case .fast: return "FAST"
        case .approve: return "APPR"
        case .decline: return "REJ"
        case .split: return "SPLIT"
        case .mic: return "MIC"
        case .codex: return "CODEX"
        case .bug: return "BUG"
        case .openai: return "OAI"
        case .term: return "TERM"
        case .download: return "DWN"
        case .delete: return "DEL"
        case .new: return "NEW"
        case .nav: return "NAV"
        case .magic: return "MAGIC"
        case .diff: return "DIFF"
        case .play: return "PLAY"
        case .git: return "GIT"
        case .branchAlt: return "BRCH"
        case .branch: return "BRANCH"
        case .merge: return "MRG"
        case .pr: return "PR"
        case .paint: return "PAINT"
        case .lab: return "LAB"
        case .party: return "PARTY"
        case .time: return "TIME"
        case .mindPlus: return "MIND+"
        case .mindMinus: return "MIND-"
        case .setup: return "SETUP"
        case .folder: return "FOLD"
        case .upload: return "UPL"
        case .apps: return "APPS"
        case .yolo: return "YOLO"
        case .yeet: return "YEET"
        }
    }

    var shortLabel: String {
        switch self {
        case .blank: return "空白"
        case .fast: return "快档"
        case .approve: return "批准"
        case .decline: return "拒绝"
        case .split: return "分栏"
        case .mic: return "听写"
        case .codex: return "输入"
        case .bug: return "调试"
        case .openai: return "唤醒"
        case .term: return "终端"
        case .download: return "到底"
        case .delete: return "归档"
        case .new: return "新建"
        case .nav: return "导航"
        case .magic: return "指令"
        case .diff: return "变更"
        case .play: return "播报"
        case .git: return "版本"
        case .branchAlt: return "切支"
        case .branch: return "分支"
        case .merge: return "合并"
        case .pr: return "评审"
        case .paint: return "灯光"
        case .lab: return "实验"
        case .party: return "语音"
        case .time: return "历史"
        case .mindPlus: return "深思"
        case .mindMinus: return "浅思"
        case .setup: return "设置"
        case .folder: return "项目"
        case .upload: return "附件"
        case .apps: return "应用"
        case .yolo: return "自动"
        case .yeet: return "发送"
        }
    }

    var chineseHint: String {
        switch self {
        case .blank: return "无图标键帽"
        case .fast: return "快速模式与档位切换"
        case .approve: return "批准待决请求"
        case .decline: return "拒绝或停止操作"
        case .split: return "向右分栏或分叉任务"
        case .mic: return "按住说话语音输入"
        case .codex: return "发送提交或聚焦输入"
        case .bug: return "诊断与调试"
        case .openai: return "唤醒目标软件"
        case .term: return "打开终端面板"
        case .download: return "导出或跳到底部"
        case .delete: return "归档或清空"
        case .new: return "新建会话或任务"
        case .nav: return "搜索与导航"
        case .magic: return "计划模式或命令面板"
        case .diff: return "代码变更或向下分栏"
        case .play: return "语音回复或面板缩放"
        case .git: return "版本状态"
        case .branchAlt: return "切换分支"
        case .branch: return "新建分支"
        case .merge: return "合并分支"
        case .pr: return "拉取请求"
        case .paint: return "外观与灯光"
        case .lab: return "实验特性"
        case .party: return "实时语音对话"
        case .time: return "历史会话轮转"
        case .mindPlus: return "选择模型或提升思考层级"
        case .mindMinus: return "降低思考层级"
        case .setup: return "侧栏与工作区设置"
        case .folder: return "选择项目或打开目录"
        case .upload: return "上传附件或注入上下文"
        case .apps: return "打开应用或内置浏览器"
        case .yolo: return "自动执行"
        case .yeet: return "发送消息"
        }
    }

    func matchesSearch(_ query: String) -> Bool {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !trimmed.isEmpty else { return true }
        return rawValue.lowercased().contains(trimmed)
            || silkCode.lowercased().contains(trimmed)
            || shortLabel.lowercased().contains(trimmed)
            || chineseHint.lowercased().contains(trimmed)
    }
}

struct CodexKeycapGlyphView: View {
    let keycap: CodexKeycapId
    var size: CGFloat = 22
    var color: Color = .white

    var body: some View {
        Canvas { context, canvasSize in
            let scale = min(canvasSize.width, canvasSize.height) / 20.0
            func pt(_ x: CGFloat, _ y: CGFloat) -> CGPoint {
                CGPoint(x: x * scale, y: y * scale)
            }
            func rect(_ x: CGFloat, _ y: CGFloat, _ w: CGFloat, _ h: CGFloat) -> CGRect {
                CGRect(x: x * scale, y: y * scale, width: w * scale, height: h * scale)
            }
            let stroke = StrokeStyle(lineWidth: 1.65 * scale, lineCap: .round, lineJoin: .round)
            let thinStroke = StrokeStyle(lineWidth: 1.4 * scale, lineCap: .round, lineJoin: .round)

            switch keycap {
            case .blank:
                let r = RoundedRectangle(cornerRadius: 2.5 * scale).path(in: rect(5.5, 5.5, 9, 9))
                context.stroke(r, with: .color(color.opacity(0.35)), style: thinStroke)

            case .fast:
                var p = Path()
                p.move(to: pt(11.2, 2.8))
                p.addLine(to: pt(5.6, 10.4))
                p.addLine(to: pt(9.6, 10.4))
                p.addLine(to: pt(8.8, 17.2))
                p.addLine(to: pt(14.4, 9.6))
                p.addLine(to: pt(10.4, 9.6))
                p.closeSubpath()
                context.stroke(p, with: .color(color), style: stroke)

            case .approve:
                let c = Circle().path(in: rect(3.6, 3.6, 12.8, 12.8))
                context.stroke(c, with: .color(color), style: stroke)
                var p = Path()
                p.move(to: pt(7.3, 10.1))
                p.addLine(to: pt(9.2, 12.0))
                p.addLine(to: pt(12.9, 8.1))
                context.stroke(p, with: .color(color), style: stroke)

            case .decline:
                let c = Circle().path(in: rect(3.6, 3.6, 12.8, 12.8))
                context.stroke(c, with: .color(color), style: stroke)
                var p = Path()
                p.move(to: pt(7.8, 7.8))
                p.addLine(to: pt(12.2, 12.2))
                p.move(to: pt(12.2, 7.8))
                p.addLine(to: pt(7.8, 12.2))
                context.stroke(p, with: .color(color), style: stroke)

            case .split:
                var p = Path()
                p.move(to: pt(4.2, 10))
                p.addLine(to: pt(8.4, 10))
                p.addLine(to: pt(11.2, 6.2))
                p.addLine(to: pt(15.4, 6.2))
                p.move(to: pt(13.2, 4.2))
                p.addLine(to: pt(15.6, 6.2))
                p.addLine(to: pt(13.2, 8.2))
                p.move(to: pt(8.4, 10))
                p.addLine(to: pt(11.2, 13.8))
                p.addLine(to: pt(15.4, 13.8))
                p.move(to: pt(13.2, 11.8))
                p.addLine(to: pt(15.6, 13.8))
                p.addLine(to: pt(13.2, 15.8))
                context.stroke(p, with: .color(color), style: stroke)

            case .mic:
                let cap = RoundedRectangle(cornerRadius: 2.6 * scale).path(in: rect(7.4, 3.6, 5.2, 7.8))
                context.stroke(cap, with: .color(color), style: stroke)
                var p = Path()
                p.addArc(center: pt(10, 9.6), radius: 4.5 * scale, startAngle: .degrees(0), endAngle: .degrees(180), clockwise: false)
                p.move(to: pt(10, 14.1))
                p.addLine(to: pt(10, 16.6))
                p.move(to: pt(7.5, 16.6))
                p.addLine(to: pt(12.5, 16.6))
                context.stroke(p, with: .color(color), style: stroke)

            case .codex:
                let c = RoundedRectangle(cornerRadius: 4.6 * scale).path(in: rect(3.6, 3.8, 12.8, 12.4))
                context.stroke(c, with: .color(color), style: thinStroke)
                var p = Path()
                p.move(to: pt(7.2, 8.4))
                p.addLine(to: pt(9.2, 10.0))
                p.addLine(to: pt(7.2, 11.6))
                p.move(to: pt(10.4, 11.6))
                p.addLine(to: pt(12.8, 11.6))
                context.stroke(p, with: .color(color), style: thinStroke)

            case .bug:
                let bodyRect = RoundedRectangle(cornerRadius: 3.4 * scale).path(in: rect(6.6, 6.8, 6.8, 8.2))
                context.stroke(bodyRect, with: .color(color), style: stroke)
                var p = Path()
                p.move(to: pt(4.2, 9.2))
                p.addLine(to: pt(6.5, 9.2))
                p.move(to: pt(13.5, 9.2))
                p.addLine(to: pt(15.8, 9.2))
                p.move(to: pt(4.6, 13.0))
                p.addLine(to: pt(6.6, 12.2))
                p.move(to: pt(15.4, 13.0))
                p.addLine(to: pt(13.4, 12.2))
                context.stroke(p, with: .color(color), style: thinStroke)

            case .openai:
                let outer = Circle().path(in: rect(3.8, 3.8, 12.4, 12.4))
                let inner = Circle().path(in: rect(7.6, 7.6, 4.8, 4.8))
                context.stroke(outer, with: .color(color), style: thinStroke)
                context.stroke(inner, with: .color(color), style: thinStroke)

            case .term:
                let box = RoundedRectangle(cornerRadius: 2.4 * scale).path(in: rect(3.4, 4.4, 13.2, 11.2))
                context.stroke(box, with: .color(color), style: stroke)
                var p = Path()
                p.move(to: pt(6.6, 8.2))
                p.addLine(to: pt(8.8, 10.0))
                p.addLine(to: pt(6.6, 11.8))
                p.move(to: pt(10.2, 11.8))
                p.addLine(to: pt(13.4, 11.8))
                context.stroke(p, with: .color(color), style: stroke)

            case .download:
                var p = Path()
                p.move(to: pt(10, 4.0))
                p.addLine(to: pt(10, 12.4))
                p.move(to: pt(7.0, 9.6))
                p.addLine(to: pt(10, 12.6))
                p.addLine(to: pt(13.0, 9.6))
                p.move(to: pt(4.8, 15.8))
                p.addLine(to: pt(15.2, 15.8))
                context.stroke(p, with: .color(color), style: stroke)

            case .delete:
                var p = Path()
                p.move(to: pt(5.2, 6.2))
                p.addLine(to: pt(14.8, 6.2))
                p.move(to: pt(8.0, 6.2))
                p.addLine(to: pt(8.0, 4.6))
                p.addLine(to: pt(12.0, 4.6))
                p.addLine(to: pt(12.0, 6.2))
                p.move(to: pt(6.6, 6.2))
                p.addLine(to: pt(7.2, 15.4))
                p.addLine(to: pt(12.8, 15.4))
                p.addLine(to: pt(13.4, 6.2))
                context.stroke(p, with: .color(color), style: thinStroke)

            case .new:
                var p = Path()
                p.move(to: pt(11.6, 4.4))
                p.addLine(to: pt(5.4, 4.4))
                p.addLine(to: pt(5.4, 15.4))
                p.addLine(to: pt(15.4, 15.4))
                p.addLine(to: pt(15.4, 9.2))
                p.move(to: pt(9.4, 10.6))
                p.addLine(to: pt(15.6, 4.4))
                context.stroke(p, with: .color(color), style: stroke)

            case .nav:
                var p = Path()
                p.move(to: pt(4.2, 9.4))
                p.addLine(to: pt(15.6, 4.6))
                p.addLine(to: pt(10.8, 16.0))
                p.addLine(to: pt(8.9, 11.3))
                p.closeSubpath()
                context.stroke(p, with: .color(color), style: stroke)

            case .magic:
                var p = Path()
                p.move(to: pt(10, 3.6))
                p.addLine(to: pt(11.9, 7.6))
                p.addLine(to: pt(16.2, 8.2))
                p.addLine(to: pt(13.1, 11.2))
                p.addLine(to: pt(13.8, 15.5))
                p.addLine(to: pt(10, 13.5))
                p.addLine(to: pt(6.2, 15.5))
                p.addLine(to: pt(6.9, 11.2))
                p.addLine(to: pt(3.8, 8.2))
                p.addLine(to: pt(8.1, 7.6))
                p.closeSubpath()
                context.stroke(p, with: .color(color), style: thinStroke)

            case .diff:
                let box = RoundedRectangle(cornerRadius: 2.4 * scale).path(in: rect(4.2, 4.2, 11.6, 11.6))
                context.stroke(box, with: .color(color), style: stroke)
                var p = Path()
                p.move(to: pt(10, 6.8))
                p.addLine(to: pt(10, 10.4))
                p.move(to: pt(8.2, 8.6))
                p.addLine(to: pt(11.8, 8.6))
                p.move(to: pt(8.2, 13.0))
                p.addLine(to: pt(11.8, 13.0))
                context.stroke(p, with: .color(color), style: stroke)

            case .git:
                let n1 = Circle().path(in: rect(4.8, 4.2, 3.2, 3.2))
                let n2 = Circle().path(in: rect(4.8, 12.6, 3.2, 3.2))
                let n3 = Circle().path(in: rect(12.0, 8.4, 3.2, 3.2))
                context.stroke(n1, with: .color(color), style: thinStroke)
                context.stroke(n2, with: .color(color), style: thinStroke)
                context.stroke(n3, with: .color(color), style: thinStroke)
                var p = Path()
                p.move(to: pt(6.4, 7.4))
                p.addLine(to: pt(6.4, 12.6))
                p.move(to: pt(8.0, 5.8))
                p.addLine(to: pt(12.0, 9.2))
                context.stroke(p, with: .color(color), style: thinStroke)

            case .branchAlt:
                var p = Path()
                p.move(to: pt(6.0, 15.6))
                p.addLine(to: pt(6.0, 4.8))
                p.move(to: pt(6.0, 10.6))
                p.addLine(to: pt(13.8, 6.2))
                p.move(to: pt(11.2, 5.6))
                p.addLine(to: pt(14.0, 6.0))
                p.addLine(to: pt(13.2, 8.8))
                context.stroke(p, with: .color(color), style: stroke)

            case .branch:
                let topNode = Circle().path(in: rect(12.0, 4.4, 3.2, 3.2))
                let botNode = Circle().path(in: rect(4.8, 12.4, 3.2, 3.2))
                context.stroke(topNode, with: .color(color), style: thinStroke)
                context.stroke(botNode, with: .color(color), style: thinStroke)
                var p = Path()
                p.move(to: pt(6.4, 4.6))
                p.addLine(to: pt(6.4, 12.4))
                p.move(to: pt(6.4, 10.0))
                p.addLine(to: pt(13.6, 10.0))
                p.addLine(to: pt(13.6, 7.6))
                context.stroke(p, with: .color(color), style: stroke)

            case .merge:
                var p = Path()
                p.move(to: pt(5.4, 4.8))
                p.addLine(to: pt(10.0, 10.2))
                p.move(to: pt(14.6, 4.8))
                p.addLine(to: pt(10.0, 10.2))
                p.addLine(to: pt(10.0, 15.6))
                p.move(to: pt(7.6, 13.2))
                p.addLine(to: pt(10.0, 15.6))
                p.addLine(to: pt(12.4, 13.2))
                context.stroke(p, with: .color(color), style: stroke)

            case .pr:
                let leftDot = Circle().path(in: rect(4.6, 4.4, 3.0, 3.0))
                let rightDot = Circle().path(in: rect(12.4, 12.6, 3.0, 3.0))
                context.stroke(leftDot, with: .color(color), style: thinStroke)
                context.stroke(rightDot, with: .color(color), style: thinStroke)
                var p = Path()
                p.move(to: pt(6.1, 7.4))
                p.addLine(to: pt(6.1, 15.4))
                p.move(to: pt(10.4, 5.8))
                p.addLine(to: pt(13.9, 5.8))
                p.addLine(to: pt(13.9, 12.6))
                context.stroke(p, with: .color(color), style: stroke)

            case .paint:
                let palette = Circle().path(in: rect(3.8, 3.8, 12.4, 12.4))
                context.stroke(palette, with: .color(color), style: stroke)
                for (x, y) in [(7.2, 6.8), (11.2, 6.8), (6.8, 10.8)] {
                    let dot = Circle().path(in: rect(x, y, 1.8, 1.8))
                    context.fill(dot, with: .color(color))
                }

            case .lab:
                var p = Path()
                p.move(to: pt(8.0, 4.4))
                p.addLine(to: pt(12.0, 4.4))
                p.move(to: pt(8.8, 4.4))
                p.addLine(to: pt(8.8, 8.2))
                p.addLine(to: pt(5.0, 15.2))
                p.addLine(to: pt(15.0, 15.2))
                p.addLine(to: pt(11.2, 8.2))
                p.addLine(to: pt(11.2, 4.4))
                context.stroke(p, with: .color(color), style: stroke)

            case .party:
                var p = Path()
                for (x, y1, y2) in [(4.8, 8.2, 11.8), (7.4, 5.8, 14.2), (10.0, 4.0, 16.0), (12.6, 6.4, 13.6), (15.2, 8.4, 11.6)] {
                    p.move(to: pt(x, y1))
                    p.addLine(to: pt(x, y2))
                }
                context.stroke(p, with: .color(color), style: stroke)

            case .time:
                let c = Circle().path(in: rect(3.8, 3.8, 12.4, 12.4))
                context.stroke(c, with: .color(color), style: stroke)
                var p = Path()
                p.move(to: pt(10.0, 6.4))
                p.addLine(to: pt(10.0, 10.2))
                p.addLine(to: pt(12.8, 11.8))
                context.stroke(p, with: .color(color), style: stroke)

            case .yolo:
                var p = Path()
                p.move(to: pt(5.0, 5.6))
                p.addLine(to: pt(9.8, 10.0))
                p.addLine(to: pt(5.0, 14.4))
                p.move(to: pt(10.4, 5.6))
                p.addLine(to: pt(15.2, 10.0))
                p.addLine(to: pt(10.4, 14.4))
                context.stroke(p, with: .color(color), style: stroke)

            case .yeet:
                var p = Path()
                p.move(to: pt(4.6, 15.4))
                p.addLine(to: pt(15.2, 4.8))
                p.move(to: pt(9.6, 4.8))
                p.addLine(to: pt(15.2, 4.8))
                p.addLine(to: pt(15.2, 10.4))
                context.stroke(p, with: .color(color), style: stroke)

            case .play:
                var p = Path()
                p.move(to: pt(7.0, 5.2))
                p.addLine(to: pt(7.0, 14.8))
                p.addLine(to: pt(14.8, 10.0))
                p.closeSubpath()
                context.stroke(p, with: .color(color), style: stroke)

            case .mindPlus, .mindMinus:
                let lobe = RoundedRectangle(cornerRadius: 3.2 * scale).path(in: rect(3.4, 5.0, 6.8, 10.0))
                context.stroke(lobe, with: .color(color), style: thinStroke)
                var p = Path()
                p.move(to: pt(12.2, 10.0))
                p.addLine(to: pt(16.8, 10.0))
                if keycap == .mindPlus {
                    p.move(to: pt(14.5, 7.7))
                    p.addLine(to: pt(14.5, 12.3))
                }
                context.stroke(p, with: .color(color), style: stroke)

            case .folder:
                var p = Path()
                p.move(to: pt(3.6, 6.2))
                p.addLine(to: pt(8.0, 6.2))
                p.addLine(to: pt(9.6, 7.8))
                p.addLine(to: pt(16.4, 7.8))
                p.addLine(to: pt(16.4, 15.0))
                p.addLine(to: pt(3.6, 15.0))
                p.closeSubpath()
                context.stroke(p, with: .color(color), style: stroke)

            case .upload:
                var p = Path()
                p.move(to: pt(10, 13.6))
                p.addLine(to: pt(10, 5.2))
                p.move(to: pt(7.0, 8.0))
                p.addLine(to: pt(10, 5.0))
                p.addLine(to: pt(13.0, 8.0))
                p.move(to: pt(4.8, 15.8))
                p.addLine(to: pt(15.2, 15.8))
                context.stroke(p, with: .color(color), style: stroke)

            case .apps:
                for (x, y) in [(4.2, 4.2), (11.0, 4.2), (4.2, 11.0), (11.0, 11.0)] {
                    let r = RoundedRectangle(cornerRadius: 1.4 * scale).path(in: rect(x, y, 4.8, 4.8))
                    context.stroke(r, with: .color(color), style: thinStroke)
                }

            case .setup:
                let c = Circle().path(in: rect(6.6, 6.6, 6.8, 6.8))
                context.stroke(c, with: .color(color), style: stroke)
                var p = Path()
                p.move(to: pt(10, 3.6)); p.addLine(to: pt(10, 5.4))
                p.move(to: pt(10, 14.6)); p.addLine(to: pt(10, 16.4))
                p.move(to: pt(3.6, 10)); p.addLine(to: pt(5.4, 10))
                p.move(to: pt(14.6, 10)); p.addLine(to: pt(16.4, 10))
                context.stroke(p, with: .color(color), style: stroke)
            }
        }
        .frame(width: size, height: size)
    }
}

enum AgentProfileKind: String {
    case cindy = "Cindy"
    case codex = "Codex"
    case antigravity = "Antigravity"
    case zcode = "ZCode"
    case dsh = "DeepSeek Coder (DSH)"
    case qwenwork = "千问办公"
    case herdr = "Herdr"
    case custom = "Custom"

    static func from(profileName: String?) -> AgentProfileKind {
        guard let name = profileName else { return .custom }
        return AgentProfileKind(rawValue: name) ?? .custom
    }

    var adapterId: String {
        switch self {
        case .cindy: return "cindy"
        case .codex: return "codex"
        case .antigravity: return "antigravity"
        case .zcode: return "zcode"
        case .dsh: return "dsh-desktop"
        case .qwenwork: return "qwenwork"
        case .herdr: return "herdr"
        case .custom: return "host"
        }
    }

    var bundleId: String {
        switch self {
        case .cindy: return "com.xd.cindy"
        case .codex: return "com.openai.codex"
        case .antigravity: return "com.google.antigravity"
        case .zcode: return "dev.zcode.app"
        case .dsh: return "com.deepseek.dsh"
        case .qwenwork: return "cn.qwenwork.desktop.mac"
        case .herdr: return "dev.herdr.app"
        case .custom: return "frontmost-editor"
        }
    }

    var topSixDefaultRole: KeycapRole {
        switch self {
        case .herdr, .qwenwork, .custom:
            return .actionKey
        case .cindy, .codex, .antigravity, .zcode, .dsh:
            return .taskKey
        }
    }

    var supportsLiveSessionSlots: Bool {
        switch self {
        case .cindy, .codex, .antigravity, .zcode, .dsh:
            return true
        case .qwenwork, .herdr, .custom:
            return false
        }
    }

    var bridgeDescription: String {
        switch self {
        case .cindy:
            return "支持 Cindy 原生控制与本机会话切换"
        case .codex:
            return "支持 Codex 会话切换、模型选择与语音听写"
        case .antigravity:
            return "支持跨工作区会话切换、审批与实时语音"
        case .zcode:
            return "支持跨项目任务切换、插话引导与语音回复"
        case .dsh:
            return "支持实时插话、停止生成与语音回复"
        case .qwenwork:
            return "千问办公高频动作直达，模型与档位合一"
        case .herdr:
            return "终端工作区全动作键模式，直控标签、分栏与面板焦点"
        case .custom:
            return "自定义软件控制方案"
        }
    }
}

struct AgentActionPresetItem: Identifiable {
    let id: String
    let title: String
    let subtitle: String
    let keycap: CodexKeycapId
    let action: CatalogAction
}

struct AgentActionGroup: Identifiable {
    let id: String
    let title: String
    let items: [AgentActionPresetItem]
}

enum CodexKeycapCatalog {
    static func inferKeycap(slot: Int, action: CatalogAction?, agent: AgentProfileKind) -> CodexKeycapId {
        if let action {
            let cmd = action.payload["command"]?.text ?? ""
            let key = (action.payload["key"]?.text ?? "").lowercased()
            let type = action.payload["type"]?.text ?? ""
            let name = action.name.lowercased()

            if type == "open_app" { return .apps }
            if cmd.hasPrefix("slot-open:") { return .nav }
            if cmd == "four-app-attention" { return .nav }
            if cmd == "approve" || cmd == "approve-current" || name.contains("批准") || name.contains("同意") { return .approve }
            if cmd == "deny" || cmd == "deny-current" || cmd == "new-or-stop" || name.contains("拒绝") || name.contains("停止") { return .decline }
            if cmd == "push-to-talk" || cmd == "dictation" || name.contains("听写") || name.contains("说话") { return .mic }
            if cmd == "live-voice" || name.contains("live") || name.contains("实时语音") { return .party }
            if cmd == "toggle-voice-reply" || name.contains("语音回复") { return .play }
            if cmd == "submit" || cmd == "focus-input" || name.contains("发送") || name.contains("输入框") { return .codex }
            if cmd == "model" || cmd == "effort" || name.contains("模型") || name.contains("思考") { return .mindPlus }
            if cmd == "project" || name.contains("项目") { return .folder }
            if cmd == "new-chat" || name.contains("新建") || (type == "shortcut" && (key == "n" || key == "t")) {
                return key == "t" ? .term : .new
            }
            if cmd == "search" || cmd == "history-search" || name.contains("搜索") || name.contains("选择 space") { return .nav }
            if cmd == "browser" || name.contains("浏览器") { return .apps }
            if cmd == "terminal" || name.contains("终端") { return .term }
            if cmd == "split-right" || name.contains("分栏") || name.contains("分屏") {
                return name.contains("向下") ? .diff : .split
            }
            if cmd == "inject-context" || name.contains("上传") || name.contains("上下文") { return .upload }
            if cmd == "clear" || name.contains("清空") || name.contains("删除") { return .delete }
            if name.contains("放大") || name.contains("缩放") { return .play }
            if name.contains("上一") || name.contains("下一") { return .time }
        }

        switch agent {
        case .herdr:
            let defaults: [Int: CodexKeycapId] = [
                0: .apps, 1: .nav, 2: .magic, 3: .new, 4: .term, 5: .split,
                6: .diff, 7: .time, 8: .time, 9: .branchAlt, 10: .play, 11: .play, 12: .codex
            ]
            return defaults[slot] ?? .codex
        case .qwenwork:
            let defaults: [Int: CodexKeycapId] = [
                0: .apps, 1: .codex, 2: .nav, 3: .new, 4: .mindPlus, 5: .upload,
                6: .mic, 7: .apps, 8: .term, 9: .magic, 10: .mic, 11: .mic, 12: .codex
            ]
            return defaults[slot] ?? .codex
        default:
            let defaults: [Int: CodexKeycapId] = [
                6: .fast, 7: .approve, 8: .decline, 9: .split, 10: .mic, 11: .mic, 12: .codex
            ]
            return defaults[slot] ?? .fast
        }
    }

    static func defaultAction(for keycap: CodexKeycapId, agent: AgentProfileKind, slot: Int) -> CatalogAction {
        let groups = actionGroups(for: agent, slot: slot)
        if let matched = groups.flatMap(\.items).first(where: { $0.keycap == keycap }) {
            return matched.action
        }
        return groups.first?.items.first?.action ?? makeAgentAction(
            id: "agent-focus-\(agent.adapterId)",
            name: "聚焦输入框",
            category: "Agent",
            detail: agent.bundleId,
            adapter: agent.adapterId,
            command: "focus-input"
        )
    }

    static func makeAgentAction(
        id: String,
        name: String,
        category: String = "Agent",
        detail: String,
        adapter: String,
        command: String
    ) -> CatalogAction {
        CatalogAction(
            id: id,
            name: name,
            category: category,
            detail: detail,
            payload: [
                "type": .string("agent_command"),
                "adapter": .string(adapter),
                "command": .string(command)
            ]
        )
    }

    static func makeShortcutAction(
        id: String,
        name: String,
        detail: String,
        key: String,
        modifiers: [String],
        target: String
    ) -> CatalogAction {
        CatalogAction(
            id: id,
            name: name,
            category: "快捷键",
            detail: detail,
            payload: [
                "type": .string("shortcut"),
                "key": .string(key),
                "modifiers": .array(modifiers.map { .string($0) }),
                "target": .string(target)
            ]
        )
    }

    static func makeOpenAppAction(bundleId: String, appName: String) -> CatalogAction {
        CatalogAction(
            id: "desktop-open-\(bundleId)",
            name: "打开 / 聚焦 \(appName)",
            category: "应用",
            detail: bundleId,
            payload: [
                "type": .string("open_app"),
                "bundle_id": .string(bundleId)
            ]
        )
    }

    static func makeSlotOpenAction(slotIndex: Int) -> CatalogAction {
        let clamped = max(0, min(5, slotIndex))
        return makeAgentAction(
            id: "cockpit-slot-\(clamped)",
            name: "会话 \(clamped + 1)",
            category: "四应用总控",
            detail: "切换到第 \(clamped + 1) 个活跃会话，长按可钉住当前会话",
            adapter: "four-app",
            command: "slot-open:\(clamped)"
        )
    }

    static func actionGroups(for agent: AgentProfileKind, slot: Int) -> [AgentActionGroup] {
        switch agent {
        case .antigravity:
            return [
                AgentActionGroup(id: "ag-core", title: "会话、思考与审批", items: [
                    .init(id: "ag-model", title: "选择模型与思考层级", subtitle: "打开思考档位与模型菜单", keycap: .mindPlus, action: makeAgentAction(id: "desktop-model", name: "选择模型", detail: "切换 Cascade 模型与思考档位", adapter: "antigravity", command: "model")),
                    .init(id: "ag-approve", title: "批准待决请求", subtitle: "批准当前挂起的命令执行或文件权限请求", keycap: .approve, action: makeAgentAction(id: "cockpit-approve", name: "审批通过", category: "四应用总控", detail: "批准当前或最高优先级待决交互请求", adapter: "host", command: "approve")),
                    .init(id: "ag-deny", title: "拒绝待决请求", subtitle: "拒绝当前挂起的交互请求", keycap: .decline, action: makeAgentAction(id: "cockpit-deny", name: "审批拒绝", category: "四应用总控", detail: "拒绝当前或最高优先级待决交互请求", adapter: "host", command: "deny")),
                    .init(id: "ag-new-stop", title: "新建或停止会话", subtitle: "运行中则停止当前生成，空闲时新建会话", keycap: .new, action: makeAgentAction(id: "cockpit-new-or-stop", name: "新建或停止会话", category: "四应用总控", detail: "运行中停止生成，空闲时新建会话", adapter: "host", command: "new-or-stop")),
                    .init(id: "ag-attention", title: "跳到下一个待办会话", subtitle: "按待审批、后台完成、异常优先级轮询跳转", keycap: .nav, action: makeAgentAction(id: "cockpit-attention", name: "四应用全局待办跳转", category: "四应用总控", detail: "一键切到最高优先级待关注会话", adapter: "four-app", command: "four-app-attention"))
                ]),
                AgentActionGroup(id: "ag-voice", title: "语音与工作区", items: [
                    .init(id: "ag-live", title: "Gemini Live 实时语音对话", subtitle: "启动或结束实时双向语音", keycap: .party, action: makeAgentAction(id: "desktop-live-voice", name: "Gemini Live 实时语音", detail: "启动或结束 Antigravity Live 实时语音对话", adapter: "antigravity", command: "live-voice")),
                    .init(id: "ag-ptt", title: "按住说话，松开停止听写", subtitle: "按住触发语音录制，松开停止", keycap: .mic, action: makeAgentAction(id: "desktop-dictation", name: "按住说话，松开停止", detail: "按住启动听写，松开停止", adapter: "antigravity", command: "push-to-talk")),
                    .init(id: "ag-submit", title: "停止听写并提交消息", subtitle: "若听写激活则先停止，再聚焦输入区回车发送", keycap: .codex, action: makeAgentAction(id: "desktop-submit", name: "发送消息（先停听写）", category: "提交控制", detail: "先停活跃听写再回车提交", adapter: "antigravity", command: "submit")),
                    .init(id: "ag-project", title: "选择或打开项目目录", subtitle: "切换侧栏项目或打开新目录", keycap: .folder, action: makeAgentAction(id: "desktop-project", name: "选项目 / 新建项目", detail: "切换侧栏项目或打开新项目目录", adapter: "antigravity", command: "project")),
                    .init(id: "ag-split", title: "快速向右分栏", subtitle: "将当前编辑器或会话向右分栏", keycap: .split, action: makeAgentAction(id: "desktop-split", name: "快速向右分栏", detail: "将当前会话向右分栏", adapter: "antigravity", command: "split-right")),
                    .init(id: "ag-browser", title: "切换内置浏览器", subtitle: "打开或收起右侧内置预览浏览器", keycap: .apps, action: makeAgentAction(id: "desktop-browser", name: "打开内置浏览器", detail: "切换右侧内置浏览器", adapter: "antigravity", command: "browser")),
                    .init(id: "ag-term", title: "切换终端面板", subtitle: "打开或收起底部终端面板", keycap: .term, action: makeAgentAction(id: "desktop-terminal", name: "打开终端面板", detail: "切换底部终端面板", adapter: "antigravity", command: "terminal"))
                ]),
                AgentActionGroup(id: "ag-navigation", title: "对话滚动与项目切换（旋钮与摇杆）", items: [
                    .init(id: "ag-scroll-up", title: "向上滚动对话", subtitle: "在当前打开的对话正文中向上滚动", keycap: .upload, action: makeAgentAction(id: "desktop-scroll-up", name: "向上滚动对话", detail: "com.google.antigravity", adapter: "antigravity", command: "scroll-up")),
                    .init(id: "ag-scroll-down", title: "向下滚动对话", subtitle: "在当前打开的对话正文中向下滚动", keycap: .download, action: makeAgentAction(id: "desktop-scroll-down", name: "向下滚动对话", detail: "com.google.antigravity", adapter: "antigravity", command: "scroll-down")),
                    .init(id: "ag-scroll-bottom", title: "跳到对话底部并聚焦输入", subtitle: "一键滚到底部最新回复并定位输入框", keycap: .download, action: makeAgentAction(id: "desktop-scroll-bottom", name: "跳到底部并聚焦输入", detail: "com.google.antigravity", adapter: "antigravity", command: "scroll-to-bottom")),
                    .init(id: "ag-sidebar-up", title: "在项目中向上滚动", subtitle: "在左侧项目列表中向上滚动并切换到上一项目", keycap: .folder, action: makeAgentAction(id: "ag-sidebar-scroll-up", name: "在项目中向上滚动", detail: "com.google.antigravity", adapter: "antigravity", command: "sidebar-scroll-up")),
                    .init(id: "ag-sidebar-down", title: "在项目中向下滚动", subtitle: "在左侧项目列表中向下滚动并切换到下一项目", keycap: .folder, action: makeAgentAction(id: "ag-sidebar-scroll-down", name: "在项目中向下滚动", detail: "com.google.antigravity", adapter: "antigravity", command: "sidebar-scroll-down")),
                    .init(id: "ag-prev-proj", title: "切换到上一项目", subtitle: "在左侧栏项目列表中向上展开一个项目", keycap: .folder, action: makeAgentAction(id: "ag-prev-project", name: "切换到上一项目", detail: "com.google.antigravity", adapter: "antigravity", command: "prev-project")),
                    .init(id: "ag-next-proj", title: "切换到下一项目", subtitle: "在左侧栏项目列表中向下展开一个项目", keycap: .folder, action: makeAgentAction(id: "ag-next-project", name: "切换到下一项目", detail: "com.google.antigravity", adapter: "antigravity", command: "next-project")),
                    .init(id: "ag-prev-chat", title: "切换到上一会话", subtitle: "在左侧栏会话列表中向上切换一条会话", keycap: .time, action: makeAgentAction(id: "desktop-prev-chat", name: "上一会话", detail: "com.google.antigravity", adapter: "antigravity", command: "prev-conversation")),
                    .init(id: "ag-next-chat", title: "切换到下一会话", subtitle: "在左侧栏会话列表中向下切换一条会话", keycap: .time, action: makeAgentAction(id: "desktop-next-chat", name: "下一会话", detail: "com.google.antigravity", adapter: "antigravity", command: "next-conversation")),
                    .init(id: "ag-cycle-attention-prev", title: "上一个 Working 或蓝点会话", subtitle: "轮询切换到上一个 Working 运行中或蓝点已完成的会话", keycap: .nav, action: makeAgentAction(id: "ag-cycle-attention-prev", name: "上一个 Working 或蓝点会话", detail: "com.google.antigravity", adapter: "antigravity", command: "cycle-attention-prev")),
                    .init(id: "ag-cycle-attention-next", title: "下一个 Working 或蓝点会话", subtitle: "轮询切换到下一个 Working 运行中或蓝点已完成的会话", keycap: .nav, action: makeAgentAction(id: "ag-cycle-attention-next", name: "下一个 Working 或蓝点会话", detail: "com.google.antigravity", adapter: "antigravity", command: "cycle-attention-next")),
                    .init(id: "ag-cycle-working-prev", title: "上一个 Working 会话", subtitle: "轮询切换到上一个 Working 运行中的会话（::）", keycap: .play, action: makeAgentAction(id: "ag-cycle-working-prev", name: "上一个 Working 会话", detail: "com.google.antigravity", adapter: "antigravity", command: "cycle-working-prev")),
                    .init(id: "ag-cycle-working-next", title: "下一个 Working 会话", subtitle: "轮询切换到下一个 Working 运行中的会话（::）", keycap: .play, action: makeAgentAction(id: "ag-cycle-working-next", name: "下一个 Working 会话", detail: "com.google.antigravity", adapter: "antigravity", command: "cycle-working-next")),
                    .init(id: "ag-cycle-completed-prev", title: "上一个蓝点已完成会话", subtitle: "轮询切换到上一个蓝点已完事待看的会话（🔵）", keycap: .approve, action: makeAgentAction(id: "ag-cycle-completed-prev", name: "上一个蓝点已完成会话", detail: "com.google.antigravity", adapter: "antigravity", command: "cycle-completed-prev")),
                    .init(id: "ag-cycle-completed-next", title: "下一个蓝点已完成会话", subtitle: "轮询切换到下一个蓝点已完事待看的会话（🔵）", keycap: .approve, action: makeAgentAction(id: "ag-cycle-completed-next", name: "下一个蓝点已完成会话", detail: "com.google.antigravity", adapter: "antigravity", command: "cycle-completed-next"))
                ])
            ]

        case .cindy:
            return [
                AgentActionGroup(id: "cindy-core", title: "任务、审批与模式", items: [
                    .init(id: "cindy-fast", title: "切换快速模式与思考力度", subtitle: "切换推理速度与思考深度", keycap: .fast, action: makeAgentAction(id: "desktop-effort", name: "调整思考力度", detail: "com.xd.cindy", adapter: "cindy", command: "effort")),
                    .init(id: "cindy-approve", title: "批准待决请求", subtitle: "批准当前权限、计划或问题请求", keycap: .approve, action: makeAgentAction(id: "cockpit-approve", name: "审批通过", category: "四应用总控", detail: "批准当前待决请求", adapter: "host", command: "approve")),
                    .init(id: "cindy-deny", title: "拒绝待决请求", subtitle: "拒绝当前权限或计划请求", keycap: .decline, action: makeAgentAction(id: "cockpit-deny", name: "审批拒绝", category: "四应用总控", detail: "拒绝当前待决请求", adapter: "host", command: "deny")),
                    .init(id: "cindy-new", title: "新建或停止任务", subtitle: "运行中停止生成，空闲时新建任务", keycap: .new, action: makeAgentAction(id: "cockpit-new-or-stop", name: "新建或停止会话", category: "四应用总控", detail: "运行中停止，空闲新建", adapter: "host", command: "new-or-stop")),
                    .init(id: "cindy-model", title: "选择模型", subtitle: "打开模型选择器", keycap: .mindPlus, action: makeAgentAction(id: "desktop-model", name: "选择模型", detail: "com.xd.cindy", adapter: "cindy", command: "model"))
                ]),
                AgentActionGroup(id: "cindy-voice", title: "语音对话与工作区", items: [
                    .init(id: "cindy-ptt", title: "按住说话，松开停止", subtitle: "按住进行语音听写", keycap: .mic, action: makeAgentAction(id: "desktop-dictation", name: "按住说话，松开停止", detail: "com.xd.cindy", adapter: "cindy", command: "push-to-talk")),
                    .init(id: "cindy-voice-reply", title: "切换语音对话回复", subtitle: "开启或关闭语音播报回复", keycap: .play, action: makeAgentAction(id: "cindy-voice-reply", name: "切换语音回复", detail: "开启或关闭 Cindy 语音对话回复", adapter: "cindy", command: "toggle-voice-reply")),
                    .init(id: "cindy-submit", title: "停止听写并提交", subtitle: "停止当前听写并提交输入框内容", keycap: .codex, action: makeAgentAction(id: "cockpit-submit", name: "停止听写并提交", category: "四应用总控", detail: "停止听写并回车提交", adapter: "host", command: "submit")),
                    .init(id: "cindy-search", title: "搜索任务", subtitle: "打开任务搜索", keycap: .nav, action: makeAgentAction(id: "desktop-search", name: "搜索任务", detail: "com.xd.cindy", adapter: "cindy", command: "search")),
                    .init(id: "cindy-browser", title: "打开内置浏览器", subtitle: "切换内置浏览器标签页", keycap: .apps, action: makeAgentAction(id: "desktop-browser", name: "打开内置浏览器", detail: "com.xd.cindy", adapter: "cindy", command: "browser")),
                    .init(id: "cindy-term", title: "打开终端面板", subtitle: "切换终端面板", keycap: .term, action: makeAgentAction(id: "desktop-terminal", name: "打开终端面板", detail: "com.xd.cindy", adapter: "cindy", command: "terminal"))
                ])
            ]

        case .zcode, .dsh:
            let appName = agent == .zcode ? "ZCode" : "DeepSeek"
            let adapter = agent.adapterId
            let bundle = agent.bundleId
            return [
                AgentActionGroup(id: "\(adapter)-core", title: "会话、插话引导与审批", items: [
                    .init(id: "\(adapter)-approve", title: "批准待决请求", subtitle: "批准当前挂起的权限或计划请求", keycap: .approve, action: makeAgentAction(id: "cockpit-approve", name: "审批通过", category: "四应用总控", detail: "批准当前待决交互请求", adapter: "host", command: "approve")),
                    .init(id: "\(adapter)-deny", title: "拒绝待决请求或停止生成", subtitle: "拒绝待决交互或取消当前运行", keycap: .decline, action: makeAgentAction(id: "cockpit-deny", name: "审批拒绝", category: "四应用总控", detail: "拒绝当前待决交互请求", adapter: "host", command: "deny")),
                    .init(id: "\(adapter)-new-stop", title: "新建或停止会话", subtitle: "运行中停止生成，空闲时新建会话", keycap: .new, action: makeAgentAction(id: "cockpit-new-or-stop", name: "新建或停止会话", category: "四应用总控", detail: "运行中停止，空闲新建", adapter: "host", command: "new-or-stop")),
                    .init(id: "\(adapter)-model", title: "选择模型", subtitle: "切换 \(appName) 模型配置", keycap: .mindPlus, action: makeAgentAction(id: "desktop-model", name: "选择模型", detail: bundle, adapter: adapter, command: "model")),
                    .init(id: "\(adapter)-effort", title: "调整思考力度", subtitle: "切换 \(appName) 思考深度档位", keycap: .fast, action: makeAgentAction(id: "desktop-effort", name: "调整思考力度", detail: bundle, adapter: adapter, command: "effort"))
                ]),
                AgentActionGroup(id: "\(adapter)-voice", title: "语音回复与工作区面板", items: [
                    .init(id: "\(adapter)-ptt", title: "按住说话，松开停止听写", subtitle: "按住启动 \(appName) 语音输入，松开自动停止", keycap: .mic, action: makeAgentAction(id: "desktop-dictation", name: "按住说话，松开停止", detail: bundle, adapter: adapter, command: "push-to-talk")),
                    .init(id: "\(adapter)-tts", title: "切换语音回复播报", subtitle: "开启或关闭 \(appName) 语音回复服务", keycap: .play, action: makeAgentAction(id: "\(adapter)-voice-reply", name: "切换语音回复", detail: "开启或关闭 \(appName) 语音回复", adapter: adapter, command: "toggle-voice-reply")),
                    .init(id: "\(adapter)-submit", title: "停止听写并提交", subtitle: "停止活跃听写并提交当前输入框内容", keycap: .codex, action: makeAgentAction(id: "cockpit-submit", name: "停止听写并提交", category: "四应用总控", detail: "停止听写并回车提交", adapter: "host", command: "submit")),
                    .init(id: "\(adapter)-search", title: "搜索会话", subtitle: "打开 \(appName) 会话搜索", keycap: .nav, action: makeAgentAction(id: "desktop-search", name: "搜索会话", detail: bundle, adapter: adapter, command: "search")),
                    .init(id: "\(adapter)-browser", title: "打开内置浏览器", subtitle: "切换 \(appName) 内置网页浏览器", keycap: .apps, action: makeAgentAction(id: "desktop-browser", name: "打开内置浏览器", detail: bundle, adapter: adapter, command: "browser")),
                    .init(id: "\(adapter)-term", title: "打开终端面板", subtitle: "在当前会话工作区打开终端", keycap: .term, action: makeAgentAction(id: "desktop-terminal", name: "打开终端面板", detail: bundle, adapter: adapter, command: "terminal"))
                ])
            ]

        case .qwenwork:
            return [
                AgentActionGroup(id: "qwen-core", title: "千问办公高频动作", items: [
                    .init(id: "qwen-open", title: "打开或聚焦千问办公", subtitle: "拉起千问办公主窗口", keycap: .apps, action: makeOpenAppAction(bundleId: "cn.qwenwork.desktop.mac", appName: "千问办公")),
                    .init(id: "qwen-input", title: "聚焦当前输入框", subtitle: "直接定位到千问办公底部输入框", keycap: .codex, action: makeAgentAction(id: "desktop-input", name: "聚焦当前输入框", detail: "cn.qwenwork.desktop.mac", adapter: "qwenwork", command: "focus-input")),
                    .init(id: "qwen-search", title: "搜索全部任务", subtitle: "打开千问办公全局任务搜索", keycap: .nav, action: makeShortcutAction(id: "desktop-search", name: "搜索全部任务", detail: "cn.qwenwork.desktop.mac", key: "g", modifiers: ["command"], target: "cn.qwenwork.desktop.mac")),
                    .init(id: "qwen-new", title: "新建任务", subtitle: "开启一条新的千问办公任务", keycap: .new, action: makeShortcutAction(id: "desktop-new", name: "新建任务", detail: "cn.qwenwork.desktop.mac", key: "n", modifiers: ["command"], target: "cn.qwenwork.desktop.mac")),
                    .init(id: "qwen-model", title: "选择模型与档位", subtitle: "千问办公将模型与思考档位合并在同一菜单", keycap: .mindPlus, action: makeAgentAction(id: "desktop-model", name: "选择模型", detail: "cn.qwenwork.desktop.mac", adapter: "qwenwork", command: "model")),
                    .init(id: "qwen-ptt", title: "按住说话，松开停止", subtitle: "自动匹配千问办公语音输入按钮", keycap: .mic, action: makeAgentAction(id: "desktop-dictation", name: "按住说话，松开停止", detail: "cn.qwenwork.desktop.mac", adapter: "qwenwork", command: "push-to-talk")),
                    .init(id: "qwen-browser", title: "打开内置浏览器", subtitle: "切换千问办公内置浏览器面板", keycap: .folder, action: makeAgentAction(id: "desktop-browser", name: "打开内置浏览器", detail: "cn.qwenwork.desktop.mac", adapter: "qwenwork", command: "browser")),
                    .init(id: "qwen-term", title: "打开终端面板", subtitle: "切换千问办公终端面板", keycap: .term, action: makeAgentAction(id: "desktop-terminal", name: "打开终端面板", detail: "cn.qwenwork.desktop.mac", adapter: "qwenwork", command: "terminal"))
                ])
            ]

        case .herdr:
            return [
                AgentActionGroup(id: "herdr-space", title: "终端工作区与分屏", items: [
                    .init(id: "herdr-open", title: "打开或聚焦 Herdr", subtitle: "激活 Herdr 终端工作区窗口", keycap: .apps, action: makeOpenAppAction(bundleId: "dev.herdr.app", appName: "Herdr")),
                    .init(id: "herdr-picker", title: "选择 Space / Tab", subtitle: "快速跳转任意终端工作区或标签页", keycap: .nav, action: makeShortcutAction(id: "herdr-desktop", name: "选择 Space / Tab", detail: "dev.herdr.app", key: "p", modifiers: ["command"], target: "dev.herdr.app")),
                    .init(id: "herdr-cmd", title: "搜索命令", subtitle: "打开 Herdr 命令面板", keycap: .magic, action: makeShortcutAction(id: "herdr-desktop", name: "搜索命令", detail: "dev.herdr.app", key: "p", modifiers: ["command", "shift"], target: "dev.herdr.app")),
                    .init(id: "herdr-new-space", title: "新建工作区", subtitle: "创建新的终端工作区", keycap: .new, action: makeShortcutAction(id: "herdr-desktop", name: "新建 Space", detail: "dev.herdr.app", key: "n", modifiers: ["command"], target: "dev.herdr.app")),
                    .init(id: "herdr-new-tab", title: "新建终端标签页", subtitle: "在当前工作区新建终端标签页", keycap: .term, action: makeShortcutAction(id: "herdr-desktop", name: "新建 Tab / 终端", detail: "dev.herdr.app", key: "t", modifiers: ["command"], target: "dev.herdr.app")),
                    .init(id: "herdr-split-r", title: "向右分屏", subtitle: "将当前终端面板向右垂直分屏", keycap: .split, action: makeShortcutAction(id: "herdr-desktop", name: "向右分屏", detail: "dev.herdr.app", key: "d", modifiers: ["command"], target: "dev.herdr.app")),
                    .init(id: "herdr-split-d", title: "向下分屏", subtitle: "将当前终端面板向下水平分屏", keycap: .diff, action: makeShortcutAction(id: "herdr-desktop", name: "向下分屏", detail: "dev.herdr.app", key: "d", modifiers: ["command", "shift"], target: "dev.herdr.app"))
                ]),
                AgentActionGroup(id: "herdr-pane", title: "标签切换与面板缩放", items: [
                    .init(id: "herdr-prev-space", title: "上一个工作区", subtitle: "切换到左侧工作区", keycap: .time, action: makeShortcutAction(id: "herdr-desktop", name: "上一个 Space", detail: "dev.herdr.app", key: "[", modifiers: ["command", "option"], target: "dev.herdr.app")),
                    .init(id: "herdr-next-space", title: "下一个工作区", subtitle: "切换到右侧工作区", keycap: .branchAlt, action: makeShortcutAction(id: "herdr-desktop", name: "下一个 Space", detail: "dev.herdr.app", key: "]", modifiers: ["command", "option"], target: "dev.herdr.app")),
                    .init(id: "herdr-prev-tab", title: "上一个标签页", subtitle: "切换到前一个终端标签页", keycap: .mindMinus, action: makeShortcutAction(id: "herdr-desktop", name: "上一个 Tab", detail: "dev.herdr.app", key: "Tab", modifiers: ["control", "shift"], target: "dev.herdr.app")),
                    .init(id: "herdr-next-tab", title: "下一个标签页", subtitle: "切换到后一个终端标签页", keycap: .mindPlus, action: makeShortcutAction(id: "herdr-desktop", name: "下一个 Tab", detail: "dev.herdr.app", key: "Tab", modifiers: ["control"], target: "dev.herdr.app")),
                    .init(id: "herdr-zoom", title: "放大或还原当前面板", subtitle: "最大化当前终端分屏面板或恢复原状", keycap: .play, action: makeShortcutAction(id: "herdr-desktop", name: "放大 / 还原面板", detail: "dev.herdr.app", key: "Return", modifiers: ["command", "shift"], target: "dev.herdr.app")),
                    .init(id: "herdr-sidebar", title: "显示或隐藏侧栏", subtitle: "切换左侧工作区导航栏", keycap: .setup, action: makeShortcutAction(id: "herdr-desktop", name: "显示 / 隐藏侧栏", detail: "dev.herdr.app", key: "b", modifiers: ["command"], target: "dev.herdr.app"))
                ])
            ]

        case .codex, .custom:
            return [
                AgentActionGroup(id: "codex-core", title: "会话、模型与语音", items: [
                    .init(id: "codex-open", title: "打开或聚焦 Codex", subtitle: "拉起 OpenAI Codex 窗口", keycap: .openai, action: makeOpenAppAction(bundleId: "com.openai.codex", appName: "Codex")),
                    .init(id: "codex-input", title: "聚焦当前输入框", subtitle: "定位到 Codex 消息输入框", keycap: .codex, action: makeAgentAction(id: "desktop-input", name: "聚焦当前输入框", detail: "com.openai.codex", adapter: "codex", command: "focus-input")),
                    .init(id: "codex-search", title: "搜索任务", subtitle: "打开会话搜索", keycap: .nav, action: makeAgentAction(id: "desktop-search", name: "搜索任务", detail: "com.openai.codex", adapter: "codex", command: "search")),
                    .init(id: "codex-model", title: "切换模型与推理强度", subtitle: "打开模型选择器", keycap: .mindPlus, action: makeShortcutAction(id: "desktop-model", name: "选择模型", detail: "com.openai.codex", key: "M", modifiers: ["control", "shift"], target: "com.openai.codex")),
                    .init(id: "codex-ptt", title: "按住说话，松开停止", subtitle: "按住进行语音听写", keycap: .mic, action: makeAgentAction(id: "desktop-dictation", name: "按住说话，松开停止", detail: "com.openai.codex", adapter: "codex", command: "push-to-talk")),
                    .init(id: "codex-browser", title: "新建标签页或浏览器", subtitle: "打开新标签页", keycap: .apps, action: makeShortcutAction(id: "desktop-browser", name: "打开内置浏览器", detail: "com.openai.codex", key: "T", modifiers: ["command"], target: "com.openai.codex")),
                    .init(id: "codex-term", title: "切换终端面板", subtitle: "打开或收起终端", keycap: .term, action: makeShortcutAction(id: "desktop-terminal", name: "打开终端面板", detail: "com.openai.codex", key: "`", modifiers: ["control"], target: "com.openai.codex"))
                ])
            ]
        }
    }
}
