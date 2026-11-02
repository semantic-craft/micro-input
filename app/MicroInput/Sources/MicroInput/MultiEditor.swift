// Original local editing format; no gesture execution or firmware encoding.
import SwiftUI

enum MultiBranch: String, CaseIterable, Identifiable {
    case tap, doubleTap = "double_tap", hold, tapHold = "tap_hold"
    var id: String { rawValue }
    var label: String { switch self { case .tap: return "单击"; case .doubleTap: return "双击"; case .hold: return "长按"; case .tapHold: return "点击后长按" } }
    var explanation: String { switch self {
    case .tap: return "一次点击的本地动作意图"
    case .doubleTap: return "连续两次点击的本地动作意图"
    case .hold: return "按住的本地动作意图"
    case .tapHold: return "先点击，再按住的本地动作意图"
    } }
}
struct MultiTarget {
    var actionID: String
    var isMacro: Bool
    var payload: JSONValue { .object(["type":.string(isMacro ? "macro" : "action"), "action_id":.string(actionID)]) }
}
struct MultiEditorDraft {
    private var source: CatalogAction
    var term: String
    var branches: [MultiBranch:MultiTarget] = [:]
    init?(action: CatalogAction) {
        guard action.payload["type"]?.text == "local_multi", case .object(let m) = action.payload["multi"], m["format_version"]?.text == "1" else { return nil }
        source = action; term = m["term_ms"]?.text ?? ""
        for branch in MultiBranch.allCases {
            if case .object(let ref) = m[branch.rawValue] {
                guard let id = ref["action_id"]?.text, let kind = ref["type"]?.text, ["macro","action"].contains(kind) else { return nil }
                branches[branch] = MultiTarget(actionID: id, isMacro: kind == "macro")
            } else if let value = m[branch.rawValue], value.text != "null" { return nil }
        }
    }
    func action(named name: String) throws -> CatalogAction {
        guard let value = UInt32(term), value > 0 else {
            throw NSError(domain: "LocalMulti", code: 1, userInfo: [NSLocalizedDescriptionKey:"本地判定时长须为正整数毫秒（UInt32）；这不是固件范围。"])
        }
        var fields: [String:JSONValue] = ["format_version":.integer(1), "term_ms":.unsigned(UInt64(value))]
        for branch in MultiBranch.allCases { fields[branch.rawValue] = branches[branch]?.payload ?? .null }
        var result = source; result.name = name; result.payload["multi"] = .object(fields); return result
    }
}

struct MultiEditor: View {
    @ObservedObject var store: Store
    var multiID: String?
    var saved: (() -> Void)?
    @State private var draft: MultiEditorDraft
    @State private var name: String
    @State private var additions: [String:CatalogAction] = [:]
    @State private var error: String?
    @Environment(\.dismiss) private var dismiss
    init(action: CatalogAction, multiID: String? = nil, store: Store, saved: (() -> Void)? = nil) {
        self.store = store; self.multiID = multiID; self.saved = saved
        _draft = State(initialValue: MultiEditorDraft(action: action)!)
        _name = State(initialValue: action.name)
    }
    private var targets: [(String,CatalogAction)] {
        store.graphActions.filter { $0.key != multiID }.sorted { $0.value.name < $1.value.name }
    }
    private func choose(_ value: String, branch: MultiBranch) {
        if value.isEmpty { draft.branches[branch] = nil; return }
        if value.hasPrefix("new:"), let action = store.catalog.first(where: { "new:" + $0.id == value }) {
            let id = UUID().uuidString.lowercased(); additions[id] = action
            draft.branches[branch] = MultiTarget(actionID: id, isMacro: false)
        } else if let action = store.graphActions[value] ?? additions[value] {
            draft.branches[branch] = MultiTarget(actionID: value, isMacro: action.payload["type"]?.text == "local_macro")
        }
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(multiID == nil ? "新建 Multi 动作" : "编辑共享 Multi").font(.title2.bold())
            Text("仅保存本地意图 · 不执行 · 不写设备").font(.caption).foregroundStyle(.secondary)
            if let shared = store.multis.first(where: { $0.id == multiID }) {
                Text("保存将更新以下 \(shared.references.count) 个共享引用：").font(.caption).bold()
                Text(shared.references.map(\.displayName).joined(separator: "、")).font(.caption).textSelection(.enabled)
            }
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    TextField("Multi 名称", text: $name).accessibilityLabel("Multi 名称")
                    HStack { Text("本地判定时长"); TextField("毫秒", text: $draft.term).frame(width: 100).accessibilityLabel("本地判定时长毫秒"); Text("ms") }
                    Text("保存手势判定的本地时序意图。实际阈值、边界行为、容量与设备格式尚未验证；本地正整数校验不代表固件范围。").font(.caption).foregroundStyle(.secondary)
                    ForEach(MultiBranch.allCases) { branch in
                        VStack(alignment: .leading, spacing: 7) {
                            Text(branch.label).font(.headline)
                            Text(branch.explanation).font(.caption).foregroundStyle(.secondary)
                            Picker("\(branch.label)动作", selection: Binding(get: { draft.branches[branch]?.actionID ?? "" }, set: { choose($0, branch: branch) })) {
                                Text("未分配").tag("")
                                Section("已保存的共享动作") {
                                    ForEach(targets, id: \.0) { id,action in Text("\(action.name) · \(action.category)").tag(id) }
                                }
                                Section("新建分支动作") {
                                    ForEach(store.catalog.filter { !["local_macro","local_multi"].contains($0.payload["type"]?.text ?? "") }) { action in Text("新建：\(action.name)").tag("new:" + action.id) }
                                }
                                if let id = draft.branches[branch]?.actionID, let a = additions[id] { Text("新建：\(a.name)").tag(id) }
                            }
                            if let id = draft.branches[branch]?.actionID {
                                if additions[id] != nil {
                                    MultiBranchParameters(action: Binding(get: { additions[id]! }, set: { additions[id] = $0 }))
                                } else if let a = store.graphActions[id] {
                                    Text("共享引用：\(a.name) · \(a.category)").font(.caption)
                                    Text(store.parameterSummary(a)).font(.caption).foregroundStyle(.secondary)
                                    if a.payload["type"]?.text == "local_macro" { Text("编辑此宏请使用“已保存的普通宏”；共享修改会影响所有引用。").font(.caption).foregroundStyle(.secondary) }
                                } else { Text("引用不可用，请重新选择动作").foregroundStyle(.red) }
                            }
                        }.padding(12).frame(maxWidth: .infinity, alignment: .leading).background(.quaternary.opacity(0.35), in: RoundedRectangle(cornerRadius: 9))
                    }
                    Text("空分支明确保留为未分配；其他分支不变。普通宏复用现有宏库，请先在宏库保存需要的宏。").font(.caption).foregroundStyle(.secondary)
                }
            }
            if let error { Text(error).font(.caption).foregroundStyle(.red).textSelection(.enabled) }
            HStack {
                Button("取消") { dismiss() }.keyboardShortcut(.cancelAction)
                Spacer()
                Button("保存四分支到草稿") {
                    Task {
                        do {
                            let used = Set(draft.branches.values.map(\.actionID))
                            if await store.saveMulti(try draft.action(named: name), id: multiID, additions: additions.filter { used.contains($0.key) }) { saved?(); dismiss() }
                            else { error = store.status }
                        } catch { self.error = error.localizedDescription }
                    }
                }.keyboardShortcut(.defaultAction).disabled(store.busy || !store.canEditDraft)
            }
        }.padding(24).frame(width: 620, height: 700)
    }
}

private struct MultiBranchParameters: View {
    @Binding var action: CatalogAction
    private var fields: [String] { action.payload.keys.filter { !["type","modifiers"].contains($0) }.sorted() }
    private var labels: [String:String] { ["key":"按键", "target":"目标应用条件", "text":"文本", "url":"网址（HTTP / HTTPS）", "bundle_id":"应用标识", "adapter":"适配器（未验证）", "command":"命令引用", "session":"会话引用", "skill":"Skill 引用"] }
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            TextField("分支动作名称", text: $action.name)
            ForEach(fields, id: \.self) { field in
                TextField(labels[field] ?? field, text: Binding(get: { action.payload[field]?.text ?? "" }, set: { action.payload[field] = .string($0) }), axis: .vertical).lineLimit(1...3)
                    .accessibilityLabel(labels[field] ?? field)
            }
            if action.payload["type"]?.text == "shortcut" {
                HStack {
                    ForEach(["command","shift","option","control"], id: \.self) { modifier in
                        Toggle(modifier, isOn: Binding(get: { modifiers.contains(modifier) }, set: { enabled in
                            var m = modifiers.filter { $0 != modifier }; if enabled { m.append(modifier) }
                            action.payload["modifiers"] = .array(m.map { .string($0) })
                        })).toggleStyle(.checkbox)
                    }
                }
            }
            Text("与四分支一起原子保存；当前动作不可执行").font(.caption).foregroundStyle(.secondary)
        }
    }
    private var modifiers: [String] { if case .array(let a) = action.payload["modifiers"] { return a.map(\.text) }; return [] }
}

struct MultiLibraryView: View {
    @ObservedObject var store: Store
    @Environment(\.dismiss) private var dismiss
    @State private var editing: MacroSummary?
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("已保存的 Multi 动作").font(.title2.bold())
            if store.multis.isEmpty { Text("尚无本地 Multi。请在动作库选择“Multi 动作”并配置四分支。").foregroundStyle(.secondary) }
            ScrollView {
                ForEach(store.multis) { multi in
                    VStack(alignment: .leading, spacing: 8) {
                        Text(multi.action.name).bold()
                        Text(store.parameterSummary(multi.action)).font(.caption)
                        Text(multi.references.isEmpty ? "未绑定，保留在本地草稿" : multi.references.map(\.displayName).joined(separator: "、")).font(.caption).foregroundStyle(.secondary)
                        HStack {
                            Button("编辑共享 Multi") { editing = multi }
                            Button("引用到当前控件") { Task { await store.bindMulti(multi.id); dismiss() } }
                            Button("删除 Multi", role: .destructive) { Task { await store.deleteMulti(multi.id) } }.disabled(!multi.references.isEmpty)
                        }.disabled(store.busy || !store.canEditDraft)
                    }.padding(12).frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            Text(store.status).font(.caption).foregroundStyle(.secondary)
            Button("完成") { dismiss() }.keyboardShortcut(.cancelAction)
        }.padding(24).frame(width: 620, height: 600)
        .sheet(item: $editing) { multi in MultiEditor(action: multi.action, multiID: multi.id, store: store) }
    }
}
