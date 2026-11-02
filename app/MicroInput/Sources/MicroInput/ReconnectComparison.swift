import SwiftUI
struct ReconnectComparison: Decodable, Identifiable {
    let token: String
    var id:String {token}
    let rows: [Row], conflicts: [String]
    let identity_matches: Bool, identity_conflict: Bool, can_merge: Bool
    let identity_confirmation_required: Bool, identity_user_confirmed: Bool
    let revision: UInt64
    struct Row: Decodable {
        let base_summary: String, local_summary: String, device_summary: String
        let path: String, label: String, base: JSONValue?, local: JSONValue?, device: JSONValue?, result: String
    }
}
struct ReconnectComparisonView: View {
    @ObservedObject var store:Store
    let initialComparison:ReconnectComparison
    private var comparison:ReconnectComparison {store.comparison ?? initialComparison}
    @State private var confirming:String?
    var body:some View {
        VStack(alignment:.leading,spacing:16){
            Text("设备版本与本地草稿").font(.title2.bold())
            Text("基线是上次接受的设备版本。这里的选择只保存本地文档；设备保持当前配置。").foregroundStyle(.secondary)
            if !comparison.identity_matches {
                Label(identityExplanation,systemImage:"exclamationmark.triangle").foregroundStyle(.orange)
            }
            if comparison.identity_confirmation_required && !comparison.identity_user_confirmed {
                Button("核对并确认是同一设备…"){confirming="identity"}.disabled(store.busy)
            }
            if comparison.rows.isEmpty {Text("可编辑字段没有变化。")}
            ScrollView {
                VStack(alignment:.leading,spacing:12){
                    ForEach(Array(comparison.rows.enumerated()),id:\.offset){_,row in
                        VStack(alignment:.leading,spacing:6){
                            Text("\(row.result) · \(row.label)").font(.headline)
                            HStack(alignment:.top,spacing:12){
                                value("基线",row.base,summary:row.base_summary);value("本地草稿",row.local,summary:row.local_summary);value("当前设备",row.device,summary:row.device_summary)
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
        .alert(confirming=="identity" ? "确认当前候选是原设备？" : confirming=="local" ? "保留整份本地草稿？" : "采用整份设备版本？",isPresented:Binding(get:{confirming != nil},set:{if !$0 {confirming=nil}})) {
            Button("取消",role:.cancel){confirming=nil}
            Button(confirming=="identity" ? "确认同一设备" : "确认保存"){if let choice=confirming {Task {if choice=="identity" {await store.confirmComparisonIdentity()} else {await store.resolveComparison(choice)}}};confirming=nil}
        } message:{
            Text(confirming=="identity" ? "USB 接口记录已变化。请先核对当前所选的 \(store.selectedDevice?.product ?? "设备") 确实是创建此草稿的原设备。确认仅适用于当前会话的这份核验快照，允许随后明确选择整份版本；它不代表技术身份核验，也不会开启自动合并或写入设备。若无法核对，请取消。" : confirming=="local" ? "忽略设备侧的可编辑变化，保留本地草稿。设备的未知原值会保留为只读；保存后仍有本地差异，旧撤销历史将清空。前态会先保存为恢复副本。" : "舍弃全部本地编辑，采用当前设备快照。保存后差异与旧撤销历史将清空。前态会先保存为恢复副本。")
        }
    }
    private var identityExplanation:String {
        if comparison.identity_user_confirmed {return "你已核对当前候选是原设备；技术身份仍未确认。仅可明确选择整份版本，不自动合并。"}
        if comparison.identity_confirmation_required {return "USB 接口记录已变化。先核对并确认当前候选是原设备，再选择整份版本。"}
        if comparison.identity_conflict {return "已知设备身份不同，不能接受此版本。"}
        return "设备身份只有当前接口记录，跨重连尚未确认。请核对所选设备，再明确选择整份版本。"
    }
    private func value(_ title:String,_ value:JSONValue?,summary:String)->some View {
        VStack(alignment:.leading){
            Text(title).font(.caption).foregroundStyle(.secondary)
            Text(summary).font(.caption).textSelection(.enabled)
            if let value {
                switch value {
                case .object,.array:
                    DisclosureGroup("查看完整原值") {ScrollView {Text(value.text).font(.caption.monospaced()).textSelection(.enabled)}.frame(maxHeight:140)}
                default: EmptyView()
                }
            }
        }.frame(maxWidth:.infinity,alignment:.leading)
    }
}
