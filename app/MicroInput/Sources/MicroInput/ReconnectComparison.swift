import SwiftUI
struct ReconnectComparison: Decodable, Identifiable {
    let token: String
    var id:String {token}
    let rows: [Row], conflicts: [String]
    let identity_matches: Bool, identity_conflict: Bool, can_merge: Bool
    let revision: UInt64
    struct Row: Decodable {
        let path: String, label: String, base: JSONValue?, local: JSONValue?, device: JSONValue?, result: String
    }
}
struct ReconnectComparisonView: View {
    @ObservedObject var store:Store
    let comparison:ReconnectComparison
    @State private var confirming:String?
    var body:some View {
        VStack(alignment:.leading,spacing:16){
            Text("设备版本与本地草稿").font(.title2.bold())
            Text("基线是上次接受的设备版本。这里的选择只保存本地文档；设备保持当前配置。").foregroundStyle(.secondary)
            if !comparison.identity_matches {
                Label(comparison.identity_conflict ? "设备身份不同，不能接受此版本" : "设备身份只有当前接口记录，跨重连尚未确认。请核对所选设备，再明确选择整份版本。",systemImage:"exclamationmark.triangle").foregroundStyle(.orange)
            }
            if comparison.rows.isEmpty {Text("可编辑字段没有变化。")}
            ScrollView {
                VStack(alignment:.leading,spacing:12){
                    ForEach(Array(comparison.rows.enumerated()),id:\.offset){_,row in
                        VStack(alignment:.leading,spacing:6){
                            Text("\(row.result) · \(row.label)").font(.headline)
                            HStack(alignment:.top,spacing:12){
                                value("基线",row.base);value("本地草稿",row.local);value("当前设备",row.device)
                            }
                        }.padding(10).background(.quaternary,in:RoundedRectangle(cornerRadius:8))
                    }
                    ForEach(comparison.conflicts,id:\.self){Text($0).foregroundStyle(.orange)}
                }
            }.frame(minHeight:180,maxHeight:420)
            HStack {
                Button("取消"){Task {await store.resolveComparison("cancel")}}.keyboardShortcut(.cancelAction)
                Spacer()
                Button("保留整份本地草稿"){confirming="local"}.disabled(comparison.identity_conflict)
                Button("采用整份设备版本"){confirming="device"}.disabled(comparison.identity_conflict)
                Button("合并无冲突变化"){Task {await store.resolveComparison("merge")}}.disabled(!comparison.can_merge)
            }.disabled(store.busy)
            Text(store.status).font(.caption).foregroundStyle(.secondary)
        }.padding(24).frame(minWidth:820)
        .alert(confirming=="local" ? "保留整份本地草稿？" : "采用整份设备版本？",isPresented:Binding(get:{confirming != nil},set:{if !$0 {confirming=nil}})) {
            Button("取消",role:.cancel){confirming=nil}
            Button("确认保存"){if let choice=confirming {Task {await store.resolveComparison(choice)}};confirming=nil}
        } message:{
            Text(confirming=="local" ? "忽略设备侧的可编辑变化，保留本地草稿。设备的未知原值会保留为只读；保存后仍有本地差异，旧撤销历史将清空。" : "舍弃全部本地编辑，采用当前设备快照。保存后差异与旧撤销历史将清空。前态会先保存为恢复副本。")
        }
    }
    private func value(_ title:String,_ value:JSONValue?)->some View {
        VStack(alignment:.leading){
            Text(title).font(.caption).foregroundStyle(.secondary)
            if let text=value?.text,text.count>180 {
                DisclosureGroup("查看完整原值") {ScrollView {Text(text).font(.caption.monospaced()).textSelection(.enabled)}.frame(maxHeight:140)}
            } else {Text(value?.text ?? "已删除 / 不存在").font(.caption.monospaced()).textSelection(.enabled)}
        }.frame(maxWidth:.infinity,alignment:.leading)
    }
}
