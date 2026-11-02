// R-P03/R-E01/R-E02/R-K01: selected workbench; all edits flow through UniFFI.
import SwiftUI
struct Workbench: View {
    @ObservedObject var store: Store
    @State private var choosing = false
    @State private var reviewing = false
    @State private var search = ""
    @State private var category = "全部"
    var bindings: [JSONValue] { guard let l = store.layer else { return [] }; return l.keys + l.encoders + l.joystick.sectors }
    func title(_ slot: Int) -> String { slot < 13 ? String(format: "按键 %02d", slot + 1) : slot < 16 ? ["旋钮 · 逆时针", "旋钮 · 顺时针", "旋钮 · 按下"][slot-13] : "摇杆 · 扇区 \(slot-15)" }
    func action(_ slot: Int) -> CatalogAction? { let key = "\(store.profileIndex):\(store.layerIndex):\(slot)"; return store.catalog.first { $0.id == store.draft.edits[key] } }
    func label(_ slot: Int) -> String { if let a = action(slot) {return a.name}; guard bindings.indices.contains(slot) else {return "未知"}; return bindings[slot].bindingLabel }
    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                VStack(alignment: .leading, spacing: 24) {
                    Label("Micro Input", systemImage: "keyboard").font(.headline)
                    Text("我的配置 · 仅浏览").font(.caption).foregroundStyle(.secondary)
                    if let snapshot = store.snapshot {
                        ForEach(snapshot.profiles.indices, id: \.self) { i in
                            Button { store.selectProfile(i) } label: {
                                HStack { Text(snapshot.profiles[i].name); Spacer(); if i == store.profileIndex { Image(systemName: "checkmark") } }.padding(10).background(i == store.profileIndex ? Color.accentColor.opacity(0.12) : Color.clear, in: RoundedRectangle(cornerRadius: 8))
                            }.buttonStyle(.plain)
                        }
                    }
                    Text("切换配置不会应用到设备。").font(.caption).foregroundStyle(.secondary)
                    Spacer()
                    Label("离线演示", systemImage: "circle.dotted")
                    Text("设备活动配置：未知\n草稿仅保留在本次运行中").font(.caption).foregroundStyle(.secondary)
                }.padding(20).frame(width: 215).background(.quaternary.opacity(0.3))
                Divider()
                VStack(spacing: 0) {
                    HStack {
                        VStack(alignment: .leading, spacing: 7) { Text(store.profile?.name ?? "载入中").font(.title.bold()); Text("点选控件，设置它要做的事。").foregroundStyle(.secondary) }
                        Spacer()
                        Button { Task { await store.undoDraft() } } label: { Image(systemName: "arrow.uturn.backward") }.help("撤销").accessibilityLabel("撤销").disabled(!store.draft.can_undo || store.busy)
                        Button { Task { await store.undoDraft(redo: true) } } label: { Image(systemName: "arrow.uturn.forward") }.help("重做").accessibilityLabel("重做").disabled(!store.draft.can_redo || store.busy)
                        Button("查看变更 · \(store.draft.edits.count)") { reviewing = true }
                    }.padding(24)
                    Divider()
                    HStack(alignment: .top, spacing: 0) {
                        ScrollView {
                            VStack(spacing: 18) {
                                if let profile = store.profile {
                                    Picker("层", selection: Binding(get: {store.layerIndex}, set: {store.selectLayer($0)})) {
                                        ForEach(profile.layers.indices, id: \.self) { i in Text(profile.layers[i].name).tag(i) }
                                    }.pickerStyle(.segmented)
                                }
                                VStack(spacing: 12) {
                                    HStack { ForEach(13..<16, id: \.self) { i in control(i, round: true) } }
                                    ForEach(0..<4, id: \.self) { row in
                                        HStack(spacing: 10) {
                                            ForEach([0,2,6,10][row]..<([0,2,6,10][row]+[2,4,4,3][row]), id: \.self) { i in control(i) }
                                        }
                                    }
                                    HStack { ForEach(16..<max(16,bindings.count), id: \.self) { i in control(i) } }
                                    Text("CREATOR MICRO").font(.caption2).tracking(3).foregroundStyle(.secondary)
                                }.padding(20).background(.background, in: RoundedRectangle(cornerRadius: 22)).overlay(RoundedRectangle(cornerRadius: 22).stroke(.quaternary)).shadow(color: .black.opacity(0.08), radius: 12, y: 8)
                                Text("13 开关逻辑位置示意 · 实体宽键和方向待核对").font(.caption).foregroundStyle(.secondary)
                            }.padding(24)
                        }.frame(maxWidth: .infinity)
                        Divider()
                        inspector.frame(width: 270).padding(22)
                    }
                }
            }
            Divider()
            HStack { Text(store.status); Spacer(); Text("草稿修订 \(store.draft.revision) · 不写设备") }.font(.caption).foregroundStyle(.secondary).padding(10)
        }.frame(minWidth: 1080, minHeight: 650)
        .sheet(isPresented: $choosing) { actionSheet }
        .sheet(isPresented: $reviewing) { diffSheet }
        .task { if store.snapshot == nil { await store.loadWorkbench() } }
    }
    func control(_ slot: Int, round: Bool = false) -> some View {
        Button { store.slot = slot } label: {
            VStack(spacing: 5) { Text(slot < 13 ? String(format: "%02d",slot+1) : slot < 16 ? ["↶","↷","↓"][slot-13] : "✣").font(.headline); Text(label(slot)).font(.caption2).lineLimit(2) }
                .frame(width: 73, height: 62).padding(3)
                .background(store.slot == slot ? Color.accentColor.opacity(0.14) : Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: round ? 25 : 9))
                .overlay(RoundedRectangle(cornerRadius: round ? 25 : 9).stroke(store.slot == slot ? Color.accentColor : Color.gray.opacity(0.2)))
        }.buttonStyle(.plain).accessibilityLabel("\(title(slot))，\(label(slot))")
    }
    var inspector: some View {
        VStack(alignment: .leading, spacing: 22) {
            Text(title(store.slot)).font(.title3.bold())
            Text(store.layer?.name ?? "").font(.caption).foregroundStyle(.secondary)
            Text("动作").foregroundStyle(.secondary)
            Button { choosing = true } label: { HStack { Text(label(store.slot)).lineLimit(3); Spacer(); Text("更换").foregroundStyle(.blue) }.padding(14).frame(maxWidth: .infinity).background(.quaternary.opacity(0.4), in: RoundedRectangle(cornerRadius: 9)) }.buttonStyle(.plain)
            if let a = action(store.slot) { Text(a.category).font(.caption).foregroundStyle(.secondary); Text(a.detail).textSelection(.enabled) }
            else if bindings.indices.contains(store.slot) {
                Text("原始绑定保留；未知值不会当成空绑定。").font(.caption).foregroundStyle(.secondary)
                DisclosureGroup("原始值与引用") { ScrollView { Text(store.snapshot?.referenceDetail(bindings[store.slot]) ?? bindings[store.slot].text).font(.caption.monospaced()).textSelection(.enabled) }.frame(maxHeight: 160) }
            }
            Spacer()
            Label("先改草稿，再检查", systemImage: "doc.badge.clock")
            Text("工具动作尚未接入。修改不执行动作，不生成设备载荷。").font(.caption).foregroundStyle(.secondary)
        }.frame(maxHeight: .infinity, alignment: .topLeading)
    }
    var actionSheet: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack { Text("选择一个动作").font(.title2.bold()); Spacer(); Button("关闭") {choosing = false} }
            TextField("搜索动作、工具或快捷键", text: $search)
            Picker("分类", selection: $category) { ForEach(["全部","快捷键","文本","Herdr","DSH"], id: \.self) { Text($0) } }.pickerStyle(.segmented)
            ForEach(store.catalog.filter { (category == "全部" || $0.category == category) && (search.isEmpty || ($0.name+$0.category+$0.detail).localizedCaseInsensitiveContains(search)) }) { a in
                Button { Task { await store.assign(a.id); choosing = false } } label: { VStack(alignment: .leading, spacing: 5) {Text(a.name).bold(); Text(a.detail).font(.caption).foregroundStyle(.secondary)}.frame(maxWidth: .infinity, alignment: .leading).padding(10) }.buttonStyle(.plain).disabled(store.busy)
            }
            Text("自有演示动作 · 不执行 · 设备格式未验证").font(.caption).foregroundStyle(.secondary)
        }.padding(24).frame(width: 530)
    }
    var diffSheet: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("草稿变更 · \(store.draft.edits.count)").font(.title2.bold())
            ScrollView {
                ForEach(store.draft.edits.keys.sorted(), id: \.self) { key in
                    let ids = key.split(separator: ":").compactMap { Int($0) }
                    if ids.count == 3, let p = store.snapshot?.profiles[ids[0]] {
                        let l = p.layers[ids[1]]
                        let original = (l.keys+l.encoders+l.joystick.sectors)[ids[2]]
                        VStack(alignment: .leading, spacing: 5) { Text("\(p.name) / \(l.name) / \(title(ids[2]))").bold(); Text("\(original.bindingLabel) → \(store.catalog.first {$0.id == store.draft.edits[key]}?.name ?? "未知")").font(.caption) }.frame(maxWidth: .infinity, alignment: .leading).padding(.vertical,8)
                    }
                }
            }
            Text("仅当前运行内保留，退出后重置。原始数据未修改。").font(.caption).foregroundStyle(.secondary)
            Button("继续编辑") { reviewing = false }.keyboardShortcut(.defaultAction)
        }.padding(24).frame(width: 550, height: 380)
    }
}
