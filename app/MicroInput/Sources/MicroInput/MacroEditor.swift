// Local sequence editing only. No input capture, preview execution, or device encoding.
import SwiftUI

struct MacroEditorStep: Identifiable {
    var id = UUID()
    var kind: String
    var key = "a"
    var milliseconds = "200"
    var payload: JSONValue {
        kind == "delay" ? .object(["type": .string(kind), "ms": .unsigned(UInt64(milliseconds) ?? 0)]) : .object(["type": .string(kind), "key": .string(key)])
    }
}
struct MacroEditorDraft {
    private var source: CatalogAction
    private(set) var steps: [MacroEditorStep]
    private var undo: [[MacroEditorStep]] = []
    private var redo: [[MacroEditorStep]] = []
    var canUndo: Bool { !undo.isEmpty }
    var canRedo: Bool { !redo.isEmpty }
    init?(action: CatalogAction) {
        guard action.payload["type"]?.text == "local_macro",
              case .object(let sequence) = action.payload["macro_sequence"], sequence["format_version"]?.text == "1",
              case .array(let events) = sequence["events"] else { return nil }
        var result: [MacroEditorStep] = []
        for event in events {
            guard case .object(let fields) = event, let kind = fields["type"]?.text, ["press", "release", "delay"].contains(kind) else { return nil }
            result.append(MacroEditorStep(kind: kind, key: fields["key"]?.text ?? "a", milliseconds: fields["ms"]?.text ?? "200"))
        }
        source = action; steps = result
    }
    private mutating func change(_ edit: (inout [MacroEditorStep]) -> Void) {
        undo.append(steps); redo = []; edit(&steps)
    }
    mutating func insert(kind: String, at index: Int) { change { $0.insert(MacroEditorStep(kind: kind), at: index) } }
    mutating func update(index: Int, key: String) { change { $0[index].key = key } }
    mutating func update(index: Int, milliseconds: String) { change { $0[index].milliseconds = milliseconds } }
    mutating func duplicate(index: Int) { change { var copy = $0[index]; copy.id = UUID(); $0.insert(copy, at: index + 1) } }
    mutating func remove(index: Int) { change { $0.remove(at: index) } }
    mutating func move(index: Int, by offset: Int) {
        guard steps.indices.contains(index + offset) else { return }
        change { $0.swapAt(index, index + offset) }
    }
    mutating func restore(redo forward: Bool = false) {
        if forward { guard let next = redo.popLast() else { return }; undo.append(steps); steps = next }
        else { guard let old = undo.popLast() else { return }; redo.append(steps); steps = old }
    }
    func action(named name: String) throws -> CatalogAction {
        for (index, step) in steps.enumerated() where step.kind == "delay" {
            guard let value = UInt32(step.milliseconds), value > 0 else {
                throw NSError(domain: "LocalMacro", code: 1, userInfo: [NSLocalizedDescriptionKey: "第 \(index + 1) 步：请输入大于 0 的整数延时（本地格式使用 UInt32 毫秒，不代表固件范围）"])
            }
        }
        var result = source; result.name = name
        result.payload["macro_sequence"] = .object(["format_version": .integer(1), "events": .array(steps.map(\.payload))])
        return result
    }
}

struct MacroEditor: View {
    @State private var draft: MacroEditorDraft
    @State private var name: String
    @State private var error: String?
    var macroID: String?
    var saved: (() -> Void)?
    @ObservedObject var store: Store
    @Environment(\.dismiss) private var dismiss
    init(action: CatalogAction, macroID: String? = nil, store: Store, saved: (() -> Void)? = nil) {
        self.store = store; self.macroID = macroID; self.saved = saved
        _draft = State(initialValue: MacroEditorDraft(action: action)!)
        _name = State(initialValue: action.name)
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("编辑普通宏").font(.title2.bold())
            Text("自有本地格式 · 按键名称仅为本地标签 · 不执行输入、不写设备").font(.caption).foregroundStyle(.secondary)
            if let id = macroID, let macro = store.macros.first(where: { $0.id == id }) {
                Text("保存将更新以下 \(macro.references.count) 个共享引用：").font(.caption).bold()
                Text(macro.references.map { "\($0.profile_name) / \($0.layer_name) / \($0.logical_control)" }.joined(separator: "、")).font(.caption).textSelection(.enabled)
            }
            TextField("宏名称", text: $name).accessibilityLabel("宏名称")
            ScrollView {
                VStack(spacing: 10) {
                    ForEach(Array(draft.steps.enumerated()), id: \.element.id) { index, step in
                        HStack(spacing: 8) {
                            Text("\(index + 1)").monospacedDigit().frame(width: 24)
                            Text(step.kind == "press" ? "按下" : step.kind == "release" ? "释放" : "延时").frame(width: 40)
                            if step.kind == "delay" {
                                TextField("毫秒", text: Binding(get: { draft.steps[index].milliseconds }, set: { draft.update(index: index, milliseconds: $0) })).accessibilityLabel("第 \(index + 1) 步延时毫秒")
                                Text("ms").foregroundStyle(.secondary)
                            } else {
                                TextField("本地按键名称", text: Binding(get: { draft.steps[index].key }, set: { draft.update(index: index, key: $0) })).accessibilityLabel("第 \(index + 1) 步按键名称")
                            }
                            Button { draft.move(index: index, by: -1) } label: { Image(systemName: "arrow.up") }.disabled(index == 0).accessibilityLabel("上移第 \(index + 1) 步")
                            Button { draft.move(index: index, by: 1) } label: { Image(systemName: "arrow.down") }.disabled(index == draft.steps.count - 1).accessibilityLabel("下移第 \(index + 1) 步")
                            Button { draft.duplicate(index: index) } label: { Image(systemName: "plus.square.on.square") }.accessibilityLabel("复制第 \(index + 1) 步")
                            Button { draft.remove(index: index) } label: { Image(systemName: "trash") }.accessibilityLabel("删除第 \(index + 1) 步")
                        }.padding(8).background(.quaternary.opacity(0.3), in: RoundedRectangle(cornerRadius: 8))
                    }
                }
            }
            HStack {
                Button("添加按下") { draft.insert(kind: "press", at: draft.steps.count) }
                Button("添加释放") { draft.insert(kind: "release", at: draft.steps.count) }
                Button("添加延时") { draft.insert(kind: "delay", at: draft.steps.count) }
                Spacer()
                Button("撤销步骤") { draft.restore() }.disabled(!draft.canUndo)
                Button("重做步骤") { draft.restore(redo: true) }.disabled(!draft.canRedo)
            }
            if let error { Text(error).foregroundStyle(.red).textSelection(.enabled) }
            Text("按下与释放需配对；延时保存为正整数毫秒。容量与设备兼容性未核实。保存形成一次草稿修改，可在工作台整体撤销。").font(.caption).foregroundStyle(.secondary)
            HStack {
                Button("取消", role: .cancel) { dismiss() }
                Spacer()
                Button("保存宏到草稿") {
                    Task {
                        do {
                            let action = try draft.action(named: name)
                            if await store.saveMacro(action, id: macroID) { saved?(); dismiss() } else { error = store.status }
                        } catch { self.error = error.localizedDescription }
                    }
                }.keyboardShortcut(.defaultAction).disabled(store.busy || !store.canEditDraft)
            }
        }.padding(24).frame(width: 760, height: 510)
    }
}


struct MacroLibraryView: View {
    @ObservedObject var store: Store
    @Environment(\.dismiss) private var dismiss
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack { Text("已保存的普通宏").font(.title2.bold()); Spacer(); Button("关闭") { dismiss() } }
            Text("引用同一宏的控件共享后续编辑；移除绑定保留宏。仅本地草稿，不执行。").font(.caption).foregroundStyle(.secondary)
            if store.macros.isEmpty { Text("尚无本地宏。请在动作库选择“普通宏”并保存步骤。").foregroundStyle(.secondary) }
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    ForEach(store.macros) { macro in
                        VStack(alignment: .leading, spacing: 6) {
                            HStack { Text(macro.action.name).bold(); Spacer(); Text("\(macro.references.count) 个引用").foregroundStyle(.secondary) }
                            Text(macro.references.isEmpty ? "未绑定，仍保留在本地草稿" : macro.references.map { "\($0.profile_name) / \($0.layer_name) / \($0.logical_control)" }.joined(separator: "、")).font(.caption)
                            HStack {
                                Button("引用到当前控件") { Task { await store.bindMacro(macro.id); dismiss() } }.disabled(store.busy || !store.canEditDraft || store.layer == nil)
                                Button("删除宏", role: .destructive) { Task { await store.deleteMacro(macro.id) } }.disabled(store.busy || !store.canEditDraft || !macro.references.isEmpty)
                                if !macro.references.isEmpty { Text("先移除以上绑定再删除").font(.caption).foregroundStyle(.secondary) }
                            }
                        }.padding(12).background(.quaternary.opacity(0.3), in: RoundedRectangle(cornerRadius: 8))
                    }
                }
            }
            Text(store.status).font(.caption).foregroundStyle(.secondary)
        }.padding(24).frame(width: 640, height: 450)
    }
}
