// R-P03/R-E01/R-E02/R-K01: selected workbench; all edits flow through UniFFI.
import SwiftUI
import UniformTypeIdentifiers
struct Workbench: View {
    @ObservedObject var store: Store
    @FocusState private var searchFocused: Bool
    @State private var choosing = false
    @State private var reviewing = false
    @State private var choosingMacro = false
    @State private var choosingMulti = false
    @State private var search = ""
    @State private var category = "全部"
    @State private var favoritesOnly = false
    @State private var candidate: CatalogAction?
    @State private var naming = false
    @State private var nameOperation = ""
    @State private var objectName = ""
    @State private var deleting = false
    @State private var deleteLayer = false
    var changeCount: Int {store.draft.edits.count + store.draft.structure_changes.count}
    func promptName(_ operation: String) {
        nameOperation = operation
        objectName = operation.contains("配置") ? (store.profile?.name ?? "新配置") : (store.layer?.name ?? "新层")
        if operation.hasPrefix("新建") {objectName = operation.contains("配置") ? "新配置" : "新层"}
        if operation.hasPrefix("复制") {objectName += " 副本"}
        naming = true
    }
    func choose(_ action: CatalogAction) {
        if action.payload["type"]?.text == "shortcut" {
            Task { if await store.saveAction(action) { choosing = false } }
        } else { candidate = action }
    }
    var bindings: [JSONValue] { guard let l = store.layer else { return [] }; return l.keys + l.encoders + l.joystick.sectors }
    func title(_ slot: Int) -> String { slot < 13 ? String(format: "按键 %02d", slot + 1) : slot < 16 ? "旋钮槽 \(slot-12) · 方向未核实" : "摇杆 · 逻辑扇区 \(slot-15)" }
    func action(_ slot: Int) -> CatalogAction? { return store.browsingDeviceSnapshot ? nil : store.draft.edits[store.key(slot)] }
    func label(_ slot: Int) -> String { if let a = action(slot) {return a.name}; guard bindings.indices.contains(slot) else {return "未知"}; return bindings[slot].bindingLabel }
    var body: some View {
        VStack(spacing: 0) {
            discoveryPanel
            InputMonitorPanel(store: store)
            Divider()
            HStack(spacing: 0) {
                VStack(alignment: .leading, spacing: 24) {
                    Label("Micro Input", systemImage: "keyboard").font(.headline)
                    Text(store.browsingDeviceSnapshot ? "设备配置 · 只读" : "我的本地配置").font(.caption).foregroundStyle(.secondary)
                    if let snapshot = store.snapshot {
                        ForEach(snapshot.profiles.indices, id: \.self) { i in
                            Button { store.selectProfile(i) } label: {
                                HStack { Text(snapshot.profiles[i].name); Spacer(); if i == store.profileIndex { Image(systemName: "checkmark") } }.padding(10).background(i == store.profileIndex ? Color.accentColor.opacity(0.12) : Color.clear, in: RoundedRectangle(cornerRadius: 8))
                            }.buttonStyle(.plain)
                        }
                    }
                    Menu("管理配置") {
                        Button("新建配置") { promptName("新建配置") }
                        Button("重命名配置") { promptName("重命名配置") }.disabled(store.profile == nil)
                        Button("复制配置") { promptName("复制配置") }.disabled(store.profile == nil)
                        Divider()
                        Button("上移配置") { Task { await store.moveProfile(by: -1) } }.disabled(store.profileIndex == 0 || store.profile == nil)
                        Button("下移配置") { Task { await store.moveProfile(by: 1) } }.disabled(store.profileIndex + 1 >= (store.snapshot?.profiles.count ?? 0))
                        Button("删除配置…", role: .destructive) {deleteLayer=false;deleting=true}.disabled(store.profile == nil)
                    }.disabled(!store.canEditDraft || store.busy)
                    PresetMenu(store: store)
                    DocumentRecoveryMenu(store: store)
                    AgentProposalButton(store: store)
                    Button("准备基础绑定应用…") { Task {await store.previewApply()} }.disabled(!store.canEditDraft || store.busy)
                    Text("切换配置不会应用到设备。").font(.caption).foregroundStyle(.secondary)
                    Spacer()
                    Label(store.browsingDeviceSnapshot ? (store.snapshot?.source == "DeviceSnapshot" ? "设备快照 · 只读" : "合成夹具 · 只读") : "本地草稿", systemImage: store.browsingDeviceSnapshot ? "eye" : "doc")
                    Text(store.browsingDeviceSnapshot ? "浏览不会激活设备配置\n原本地草稿已保留" : "设备活动配置：未知\n草稿自动保存到本机").font(.caption).foregroundStyle(.secondary)
                }.padding(20).frame(width: 215).background(.quaternary.opacity(0.3))
                Divider()
                VStack(spacing: 0) {
                    HStack {
                        VStack(alignment: .leading, spacing: 7) { Text(store.profile?.name ?? (store.snapshot == nil ? "载入中" : "暂无配置")).font(.title.bold()); Text(store.browsingDeviceSnapshot ? "设备快照只读；点选控件查看绑定。" : "点选控件，设置它要做的事。").foregroundStyle(.secondary) }
                        Spacer()
                        Button { Task { await store.undoDraft() } } label: { Image(systemName: "arrow.uturn.backward") }.help("撤销").accessibilityLabel("撤销").disabled(!store.canEditDraft || !store.draft.can_undo || store.busy)
                        Button { Task { await store.undoDraft(redo: true) } } label: { Image(systemName: "arrow.uturn.forward") }.help("重做").accessibilityLabel("重做").disabled(!store.canEditDraft || !store.draft.can_redo || store.busy)
                        Button("查看变更 · \(changeCount)") { reviewing = true }.disabled(store.browsingDeviceSnapshot)
                    }.padding(24)
                    Divider()
                    HStack(alignment: .top, spacing: 0) {
                        ScrollView {
                            VStack(spacing: 18) {
                                if let profile = store.profile {
                                    Picker("层", selection: Binding(get: {store.layerIndex}, set: {store.selectLayer($0)})) {
                                        ForEach(profile.layers.indices, id: \.self) { i in Text(profile.layers[i].name).tag(i) }
                                    }.pickerStyle(.segmented)
                                    Menu("管理层") {
                                        Button("新建层") {promptName("新建层")}
                                        Button("重命名层") {promptName("重命名层")}.disabled(store.layer == nil)
                                        Button("复制层") {promptName("复制层")}.disabled(store.layer == nil)
                                        Divider()
                                        Button("上移层") {Task {await store.moveLayer(by:-1)}}.disabled(store.layerIndex == 0 || store.layer == nil)
                                        Button("下移层") {Task {await store.moveLayer(by:1)}}.disabled(store.layerIndex+1 >= (store.profile?.layers.count ?? 0))
                                        Button("删除层…",role:.destructive) {deleteLayer=true;deleting=true}.disabled(store.layer == nil)
                                    }.disabled(!store.canEditDraft || store.busy)
                                }
                                if store.layer == nil {
                                    VStack(spacing:14) {
                                        Text(store.profile == nil ? "暂无配置" : "此配置暂无层").font(.title2)
                                        Text("新建本地配置或层后即可继续编辑。").foregroundStyle(.secondary)
                                        Button(store.profile == nil ? "新建配置" : "新建层") {promptName(store.profile == nil ? "新建配置" : "新建层")}.disabled(!store.canEditDraft || store.busy)
                                    }.frame(maxWidth:.infinity,minHeight:280)
                                } else {
                                VStack(spacing: 12) {
                                    HStack { ForEach(13..<16, id: \.self) { i in control(i, round: true) } }
                                    ForEach(0..<4, id: \.self) { row in
                                        HStack(spacing: 10) {
                                            ForEach([0,2,6,10][row]..<([0,2,6,10][row]+[2,4,4,3][row]), id: \.self) { i in control(i) }
                                        }
                                    }
                                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 100))], spacing: 10) { ForEach(16..<max(16,bindings.count), id: \.self) { i in control(i) } }
                                    Text("CREATOR MICRO").font(.caption2).tracking(3).foregroundStyle(.secondary)
                                }.padding(20).background(.background, in: RoundedRectangle(cornerRadius: 22)).overlay(RoundedRectangle(cornerRadius: 22).stroke(.quaternary)).shadow(color: .black.opacity(0.08), radius: 12, y: 8)
                                Text("13 开关逻辑位置示意 · 实体宽键和方向待核对").font(.caption).foregroundStyle(.secondary)
                                }
                            }.padding(24)
                        }.frame(maxWidth: .infinity)
                        Divider()
                        ScrollView { inspector }.frame(width: 270).padding(22)
                    }
                }
            }
            Divider()
            HStack { Text(store.status); Spacer(); Text("草稿修订 \(store.draft.revision) · 不写设备") }.font(.caption).foregroundStyle(.secondary).padding(10)
        }.frame(minWidth: 1020, minHeight: 620)
        .background {
            Button("前一个控件") { store.slot = max(0, store.slot - 1) }.keyboardShortcut("[", modifiers: [.command]).hidden()
            Button("后一个控件") { store.slot = min(max(0,bindings.count - 1), store.slot + 1) }.keyboardShortcut("]", modifiers: [.command]).hidden()
            Button("更换所选动作") { if store.canEditDraft && store.layer != nil { choosing = true } }.keyboardShortcut("e", modifiers: [.command]).hidden()
            Button("查看差异") { reviewing = true }.keyboardShortcut("d", modifiers: [.command]).hidden()
            Button("撤销草稿") { Task { await store.undoDraft() } }.keyboardShortcut("z", modifiers: [.command]).hidden()
            Button("重做草稿") { Task { await store.undoDraft(redo: true) } }.keyboardShortcut("z", modifiers: [.command, .shift]).hidden()
        }
        .sheet(isPresented: $choosing) { actionSheet }
        .sheet(isPresented: $choosingMulti) { MultiLibraryView(store: store) }
        .sheet(isPresented: $choosingMacro) { MacroLibraryView(store: store) }
        .sheet(isPresented: $reviewing) { diffSheet }
        .sheet(isPresented: $naming) { nameSheet }
        .alert(deleteLayer ? "删除所选层？" : "删除所选配置？", isPresented:$deleting) {
            Button("取消",role:.cancel) {}
            Button("删除",role:.destructive) {Task {if deleteLayer {await store.deleteLayer()} else {await store.deleteProfile()}}}
        } message: {Text("仅删除本地草稿中的对象及其所属内容。原始来源保留，可撤销；设备不会改变。") }
        .sheet(item: $store.applyPreview, onDismiss: { Task { await store.cancelApplyPreview() } }) { preview in ApplyPreparationView(store:store,preview:preview) }
        .sheet(item: $store.comparison) { comparison in ReconnectComparisonView(store: store, initialComparison: comparison) }
        .task { if store.snapshot == nil { await store.loadWorkbench() } }
    }
    var discoveryPanel: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Label(store.connectionSummary, systemImage: "cable.connector")
                Spacer()
                if !store.devices.isEmpty {
                    Picker("USB 候选", selection: Binding(get: { store.selectedDeviceID ?? "" }, set: { store.selectDevice($0) })) {
                        Text("请选择设备").tag("")
                        ForEach(Array(store.devices.enumerated()), id: \.element.id) { index, device in
                            Text("\(device.product) · \(String(format: "%04X", device.pid)) · 接口 \(index + 1)\(device.verifiedModel ? "" : " · 未验型号")").tag(device.id)
                        }
                    }.frame(maxWidth: 410)
                }
                Button("查找 / 刷新 USB") { Task { await store.startDiscovery(); await store.refresh() } }
                Button("返回本地草稿") { store.showLocalDraft() }.disabled(!store.browsingDeviceSnapshot || store.busy)
            }
            HStack {
                Button("连接选中设备") { if let id = store.selectedDeviceID { Task { await store.connect(id) } } }
                    .disabled(store.busy || store.selectedDevice?.verifiedModel != true)
                Button("读取身份") { Task { await store.readIdentity() } }
                    .disabled(store.busy || store.connectedID == nil || store.connectedID != store.selectedDeviceID)
                Button("读取设备配置") { Task { await store.readDeviceConfig() } }
                    .disabled(store.busy || store.connectedID == nil || store.connectedID != store.selectedDeviceID || !["0.6.3", "synthetic-0.6.3"].contains(store.identity?.version ?? ""))
                Button("断开") { Task { await store.suspend() } }.disabled(!store.canDisconnect)
                if store.browsingDeviceSnapshot {
                    Button("从快照新建草稿") {Task {await store.createDraftFromSnapshot()}}
                        .disabled(store.busy || store.stale || store.connectedID == nil)
                }

                Button("比较设备与草稿") { Task {await store.compareSnapshot()} }.disabled(store.busy || store.connectedID == nil || store.stale)
                Text(store.identityStatus).font(.caption).foregroundStyle(.secondary)
            }
            if let device = store.selectedDevice {
                Text("OS 名称：\(device.product) · USB · VID \(String(format: "%04X", device.vid)) / PID \(String(format: "%04X", device.pid)) · 固件：\(store.connectedID == device.id ? (store.identity?.version ?? "未知") : "未知")")
                    .font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                Text("主 usage \(String(format: "%04X/%d", device.usagePage, device.usage)) · 完整 usage pairs：\(device.usagePairs.map { String(format: "%04X/%d", $0.page, $0.usage) }.joined(separator: ", "))")
                    .font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
            }
            if let identity = store.identity, store.connectedID == store.selectedDeviceID {
                Text("设备活动 Profile 索引：\(identity.profileIndex.map(String.init) ?? "未知") · Layer 索引：\(identity.layerIndex.map(String.init) ?? "未知") · 来源：\(identity.sourceMethod)")
                    .font(.caption).foregroundStyle(.secondary)
                Text("身份查询已回包 · 配置兼容与生命周期仍待验收")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Text("连接、身份与配置分别读取；仅核对支持的身份后读取文件。浏览不激活配置。")
                .font(.caption).foregroundStyle(.secondary)
            if store.browsingDeviceSnapshot, let snapshot = store.snapshot {
                HStack {
                    Label(store.stale ? "旧设备快照 · 已过期" : (snapshot.source == "DeviceSnapshot" ? "设备快照 · 只读" : "合成夹具 · 只读"), systemImage: store.stale ? "exclamationmark.triangle" : "eye")
                    Text("固件 \(snapshot.firmware ?? "未知") · 读取于 \(Date(timeIntervalSince1970: TimeInterval(snapshot.read_at)).formatted(date: .abbreviated, time: .standard))")
                }.font(.caption).foregroundStyle(store.stale ? .orange : .secondary)
                if let files = snapshot.device_files {
                    DisclosureGroup("文件验证 · \(files.count) 个文件 · smart_actions.json \(files.contains(where: {$0.name == "smart_actions.json"}) ? "存在" : "不存在")") {
                        ForEach(files, id: \.name) { file in Text("\(file.name) · \(file.size) 字节 · SHA-1 \(file.sha1)").font(.caption.monospaced()).textSelection(.enabled) }
                        Text("前后元数据一致；不保证跨文件原子性。").font(.caption).foregroundStyle(.secondary)
                    }
                }
                if !snapshot.warnings.isEmpty {
                    DisclosureGroup("只读投影提示 · \(snapshot.warnings.count)") {
                        ForEach(snapshot.warnings, id: \.self) { Text($0).font(.caption).foregroundStyle(.secondary) }
                    }
                }
            }
        }.padding(.horizontal, 20).padding(.vertical, 12)
    }
    func control(_ slot: Int, round: Bool = false) -> some View {
        Button { store.slot = slot } label: {
            VStack(spacing: 5) { Text(slot < 13 ? String(format: "%02d",slot+1) : slot < 16 ? "E\(slot-12)" : "J\(slot-15)").font(.headline); Text(label(slot)).font(.caption2).lineLimit(2) }
                .frame(width: 73, height: 62).padding(3)
                .background(store.slot == slot ? Color.accentColor.opacity(0.14) : Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: round ? 25 : 9))
                .overlay(RoundedRectangle(cornerRadius: round ? 25 : 9).stroke(store.slot == slot ? Color.accentColor : Color.gray.opacity(0.2)))
        }.buttonStyle(.plain).accessibilityLabel("\(title(slot))，\(label(slot))")
    }
    var inspector: some View {
        VStack(alignment: .leading, spacing: 22) {
            Text(title(store.slot)).font(.title3.bold())
            Text(store.layer?.name ?? "").font(.caption).foregroundStyle(.secondary)
            if store.slot >= 16, let settings = store.currentControlSettings, !store.browsingDeviceSnapshot {
                Text(settings.joystick_mode == "logical_sectors" && store.slot - 16 < Int(settings.active_sectors) ? "活动逻辑扇区 · 物理方向未核实" : "保留槽位 · 绑定未删除").font(.caption).foregroundStyle(.secondary)
            }
            if store.browsingDeviceSnapshot { Text("层颜色：\(store.layer?.displayColor ?? "未知（原值保留）")").font(.caption).foregroundStyle(.secondary) }
            if store.slot >= 13, !store.browsingDeviceSnapshot {
                ControlSettingsEditor(store: store).id((store.layer?.id.text ?? "") + String(store.draft.revision))
                Divider()
            }
            Text("动作").foregroundStyle(.secondary)
            Button { choosing = true } label: { HStack { Text(label(store.slot)).lineLimit(3); Spacer(); Text(store.browsingDeviceSnapshot ? "只读" : "更换").foregroundStyle(.blue) }.padding(14).frame(maxWidth: .infinity).background(.quaternary.opacity(0.4), in: RoundedRectangle(cornerRadius: 9)) }.buttonStyle(.plain).disabled(!store.canEditDraft || store.layer == nil)
            Button("已保存的普通宏（\(store.macros.count)）") { choosingMacro = true }.disabled(!store.canEditDraft)
            Button("已保存的 Multi（\(store.multis.count)）") { choosingMulti = true }.disabled(!store.canEditDraft)
            if let multi = store.selectedMulti {
                Text("共享 Multi · \(multi.references.count) 个引用").font(.caption).foregroundStyle(.secondary)
                Button("移除此控件的 Multi 引用") { Task { await store.bindMulti(nil) } }.disabled(store.busy || !store.canEditDraft)
            }
            if let macro = store.selectedMacro {
                Text("共享宏 · \(macro.references.count) 个控件引用").font(.caption).foregroundStyle(.secondary)
                Button("移除此控件的宏引用") { Task { await store.bindMacro(nil) } }.disabled(store.busy || !store.canEditDraft)
            }
            if let a = action(store.slot) {
                Text(a.category).font(.caption).foregroundStyle(.secondary)
                if let capability = store.actionCapabilities[store.slotKey] { ActionCapabilityView(capability: capability) }
                ActionParameters(action: a, store: store, macroID: store.selectedMacro?.id, multiID: store.selectedMulti?.id).id(store.slotKey + String(store.draft.revision))
                DisclosureGroup("高级参数原值") { Text(JSONValue.object(a.payload).text).font(.caption.monospaced()).textSelection(.enabled) }
            }
            else if bindings.indices.contains(store.slot) {
                Text("原始绑定保留；未知值不会当成空绑定。").font(.caption).foregroundStyle(.secondary)
                DisclosureGroup("原始值与引用") { ScrollView { Text(store.snapshot?.referenceDetail(bindings[store.slot]) ?? bindings[store.slot].text).font(.caption.monospaced()).textSelection(.enabled) }.frame(maxHeight: 160) }
            }
            if store.browsingDeviceSnapshot, let snapshot = store.snapshot, !snapshot.smartRecords.isEmpty {
                DisclosureGroup("Smart Action 原始记录（只读）") {
                    ForEach(snapshot.smartRecords, id: \.0) { key, value in
                        DisclosureGroup(key) { Text(value.text).font(.caption.monospaced()).textSelection(.enabled) }
                    }
                }
            }
            Spacer()
            Label(store.browsingDeviceSnapshot ? "设备快照仅供查看" : "先改草稿，再检查", systemImage: "doc.badge.clock")
            Text("工具动作尚未接入。修改不执行动作，不生成设备载荷。").font(.caption).foregroundStyle(.secondary)
        }.frame(maxHeight: .infinity, alignment: .topLeading)
    }
    var actionSheet: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack { Text("选择一个动作").font(.title2.bold()); Spacer(); Button("关闭") {choosing = false}.keyboardShortcut(.cancelAction) }
            if let candidate {
                Button("返回动作库") { self.candidate = nil }
                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        Text("设置 \(candidate.name)").font(.headline)
                        if let info = store.library.entries.first(where: {$0.id == candidate.id})?.capability { ActionCapabilityView(capability: info) }
                        ActionParameters(action: candidate, store: store, saved: { choosing = false }).id(candidate.id)
                    }.frame(maxWidth: .infinity, alignment: .leading)
                }
            } else {
                TextField("搜索动作、工具、目标或快捷键", text: $search).focused($searchFocused)
                    .onSubmit { if let first = store.library.entries.first { choose(first.action) } }
                HStack {
                    Picker("分类", selection: $category) { Text("全部").tag("全部"); ForEach(store.library.categories, id: \.self) { Text($0).tag($0) } }
                    Toggle("只看收藏", isOn: $favoritesOnly)
                }
                if let error = store.library.preferences_error { Text(error).font(.caption).foregroundStyle(.orange) }
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 12) {
                        if store.library.entries.isEmpty { Text("没有匹配动作").foregroundStyle(.secondary).padding() }
                        if !favoritesOnly && (category == "全部" || category == "普通宏") {
                            ForEach(store.macros.filter { search.isEmpty || $0.action.name.localizedCaseInsensitiveContains(search) }) { macro in
                                Button { Task { await store.bindMacro(macro.id); choosing = false } } label: {
                                    VStack(alignment: .leading, spacing: 5) {
                                        Text(macro.action.name).bold()
                                        Text("已保存本地宏 · 共享引用 · \(macro.references.count) 个控件").font(.caption).foregroundStyle(.secondary)
                                    }.frame(maxWidth: .infinity, alignment: .leading).padding(10)
                                }.buttonStyle(.plain).disabled(store.busy || !store.canEditDraft)
                            }
                        }
                        ForEach(store.library.entries) { entry in
                            HStack(alignment: .top) {
                                Button { choose(entry.action) } label: {
                                    VStack(alignment: .leading, spacing: 5) {
                                        Text(entry.action.name).bold()
                                        Text(entry.capability.unavailable_reason).font(.caption).foregroundStyle(.secondary)
                                        Text("目标：\(entry.capability.target)").font(.caption).foregroundStyle(.secondary)
                                    }.frame(maxWidth: .infinity, alignment: .leading).padding(10)
                                }.buttonStyle(.plain).disabled(store.busy || !store.canEditDraft)
                                Button { Task { await store.setFavorite(entry.id, favorite: !entry.favorite) } } label: { Image(systemName: entry.favorite ? "star.fill" : "star") }
                                    .buttonStyle(.borderless).padding(10).accessibilityLabel("\(entry.favorite ? "取消收藏" : "收藏") \(entry.action.name)")
                                    .disabled(store.library.preferences_error != nil)
                            }
                        }
                    }
                }
            }
            Text(store.status).font(.caption).foregroundStyle(.secondary).lineLimit(3)
            Text("可保存本地意图；所有条目当前均不可执行。收藏是本机偏好。").font(.caption).foregroundStyle(.secondary)
        }.padding(24).frame(width: 600, height: 560)
        .onAppear { search = ""; category = "全部"; favoritesOnly = false; candidate = nil; searchFocused = true }
        .task(id: "\(search)\u{0}\(category)\u{0}\(favoritesOnly)") { await store.searchActions(query: search, category: category, favoritesOnly: favoritesOnly) }
    }
    var nameSheet: some View {
        VStack(alignment:.leading,spacing:16) {
            Text(nameOperation).font(.title2.bold())
            TextField("名称",text:$objectName)
            Text("仅保存到本地草稿；原始设备数据保持不变。").font(.caption).foregroundStyle(.secondary)
            HStack {
                Button("取消") {naming=false}.keyboardShortcut(.cancelAction)
                Spacer()
                Button("保存") {
                    let name=objectName;let operation=nameOperation;naming=false
                    Task {switch operation {
                    case "新建配置":await store.createProfile(name:name)
                    case "重命名配置":await store.renameProfile(name:name)
                    case "复制配置":await store.duplicateProfile(name:name)
                    case "新建层":await store.createLayer(name:name)
                    case "重命名层":await store.renameLayer(name:name)
                    case "复制层":await store.duplicateLayer(name:name)
                    default:break
                    }}
                }.keyboardShortcut(.defaultAction).disabled(objectName.trimmingCharacters(in:.whitespacesAndNewlines).isEmpty)
            }
        }.padding(24).frame(width:380)
    }
    var diffSheet: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("草稿变更 · \(changeCount)").font(.title2.bold())
            ScrollView {
                ForEach(Array(store.draft.structure_changes.enumerated()),id: \.offset) { _,change in
                    Text(change).frame(maxWidth:.infinity,alignment:.leading).padding(.vertical,5)
                }
                ForEach(store.draft.edits.keys.sorted(), id: \.self) { key in
                    if let snapshot = store.snapshot {
                        ForEach(snapshot.profiles.indices, id: \.self) { pi in
                            let p = snapshot.profiles[pi]
                            ForEach(p.layers.indices, id: \.self) { li in
                                let l = p.layers[li]
                                let values = l.keys + l.encoders + l.joystick.sectors
                                ForEach(values.indices, id: \.self) { si in
                                    if bindingKey(profile: p.id, layer: l.id, slot: si) == key, let action = store.draft.edits[key] {
                                        VStack(alignment: .leading, spacing: 5) { Text("\(p.name) / \(l.name) / \(title(si))").bold(); Text("\(values[si].bindingLabel) → \(action.name)").font(.caption); Text(store.parameterSummary(action)).font(.caption).foregroundStyle(.secondary) }.frame(maxWidth: .infinity, alignment: .leading).padding(.vertical,8)
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Text("草稿自动保存，重开后恢复。原始数据未修改。").font(.caption).foregroundStyle(.secondary)
            Button("继续编辑") { reviewing = false }.keyboardShortcut(.defaultAction)
            Button("关闭变更") { reviewing = false }.keyboardShortcut(.cancelAction)
        }.padding(24).frame(width: 550, height: 380)
    }
}

// R-K05: typed parameters edited locally; adapters remain unavailable.
struct ActionParameters: View {
    @State var action: CatalogAction
    @State private var editingMacro = false
    @State private var editingMulti = false
    @ObservedObject var store: Store
    var macroID: String? = nil
    var multiID: String? = nil
    var saved: (() -> Void)? = nil
    var type: String { action.payload["type"]?.text ?? "unknown" }
    var fields: [(String,String)] {
        switch type {
        case "text": return [("text","文本内容")]
        case "open_url": return [("url","网址（HTTP / HTTPS）")]
        case "shortcut": return [("key","按键"),("target","目标应用条件")]
        case "agent_command": return [("adapter","适配器（未验证）"),("command","命令引用")]
        case "focus_session": return [("adapter","适配器（未验证）"),("session","会话引用")]
        case "skill_reference": return [("adapter","适配器（未验证）"),("skill","Skill 引用")]
        default: return []
        }
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if type == "local_multi", MultiEditorDraft(action: action) != nil {
                Button("编辑四分支 Multi") { editingMulti = true }.disabled(!store.canEditDraft || store.busy)
                Text(store.parameterSummary(action)).font(.caption).foregroundStyle(.secondary)
            }
            if type == "local_macro", MacroEditorDraft(action: action) != nil {
                Button("编辑宏步骤") { editingMacro = true }.disabled(!store.canEditDraft || store.busy)
                Text("自有本地序列 · 按下 / 释放 / 延时 · 不执行").font(.caption).foregroundStyle(.secondary)
            }
            if !["local_macro", "local_multi"].contains(type) {
            Text("动作名称").font(.caption).foregroundStyle(.secondary)
            TextField("动作名称", text: $action.name)
            ForEach(fields, id: \.0) { key,label in
                Text(label).font(.caption).foregroundStyle(.secondary)
                TextField(label, text: Binding(get: {action.payload[key]?.text ?? ""}, set: {action.payload[key] = .string($0)}), axis: .vertical).lineLimit(1...4)
            }
            if type == "device_key" {
                Picker("普通设备按键", selection: Binding(get: {action.payload["token"]?.text ?? "KC_B"}, set: {action.payload["token"] = .string($0)})) {
                    ForEach(["KC_A","KC_B","KC_ENTER","KC_ESC","KC_SPACE","KC_TAB"],id:\.self) { Text($0).tag($0) }
                }
                Text("本地设备按键意图 · 保存后仍未应用 · 实体位置未核实").font(.caption).foregroundStyle(.secondary)
            }
            if type == "open_app" {
                Text("目标应用：\(action.payload["bundle_id"]?.text ?? "未选择")").font(.caption).textSelection(.enabled)
                Button("选择应用…") {
                    let panel = NSOpenPanel(); panel.allowedContentTypes = [.applicationBundle]
                    panel.canChooseDirectories = false; panel.allowsMultipleSelection = false
                    panel.directoryURL = URL(fileURLWithPath: "/Applications")
                    panel.begin { result in
                        if result == .OK, let url = panel.url, let bundleID = Bundle(url: url)?.bundleIdentifier {
                            action.payload["bundle_id"] = .string(bundleID)
                            action.name = "打开 \(url.deletingPathExtension().lastPathComponent)"
                        }
                    }
                }
            }
            if type == "shortcut" {
                ForEach(["command","shift","option","control"], id: \.self) { modifier in
                    Toggle(modifier, isOn: Binding(get: { modifiers.contains(modifier) }, set: { enabled in
                        var next = modifiers.filter {$0 != modifier}; if enabled {next.append(modifier)}
                        action.payload["modifiers"] = .array(next.map { .string($0) })
                    }))
                }
            }
            Text("参数修改后点击保存").font(.caption).foregroundStyle(.secondary)
            Button("保存参数到草稿") { Task { if await store.saveAction(action) { saved?() } } }.disabled(!store.canEditDraft || store.busy)
            }
        }.sheet(isPresented: $editingMulti) { MultiEditor(action: action, multiID: multiID, store: store, saved: saved) }
        .sheet(isPresented: $editingMacro) { MacroEditor(action: action, macroID: macroID, store: store, saved: saved) }
    }
    var modifiers: [String] { if case .array(let values) = action.payload["modifiers"] { return values.map(\.text) }; return [] }
}

struct ActionCapabilityView: View {
    let capability: ActionCapability
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Label("当前不可执行", systemImage: "pause.circle").font(.caption.bold())
            Text(capability.unavailable_reason).font(.caption)
            Text("目标：\(capability.target)").font(.caption).textSelection(.enabled)
            Text(capability.host_required ? "需要 Mac 宿主常驻" : "宿主常驻要求及设备独立执行能力待验证").font(.caption).foregroundStyle(.secondary)
            ForEach(capability.prerequisites, id: \.self) { Text($0).font(.caption).foregroundStyle(.secondary) }
            DisclosureGroup("来源与结果语义") {
                Text(capability.source).font(.caption)
                Text(capability.result_semantics).font(.caption)
            }
        }
    }
}
