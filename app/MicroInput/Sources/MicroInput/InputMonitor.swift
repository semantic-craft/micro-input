import SwiftUI
import MicroInputCore

@MainActor final class InputMonitorState: ObservableObject {
    @Published private(set) var count: UInt64 = 0
    @Published private(set) var lost: UInt64 = 0
    @Published private(set) var lastEvent: InputEventInfo?
    @Published private(set) var status = "连接设备后持续监听"
    @Published private(set) var paused = false
    private var subscription: InputSubscription?
    private var generation = 0

    var eventSummary: String? {
        guard let event = lastEvent else { return nil }
        switch event.kind {
        case .pressed, .released:
            let phase = event.kind == .pressed ? "按下" : "释放"
            let binding = event.bindingId ?? "未知绑定"
            let slot = event.logicalSwitch ?? "未知槽位"
            let phys = event.physicalControl ?? "未知控件"
            let group = event.keycapGroup.map { " · 键帽组 \($0)" } ?? ""
            return "已验证输入 \(binding) → \(slot) (\(phys)\(group)) · \(phase)"
        case .candidatePressed, .candidateReleased:
            guard let binding = event.bindingId else { return "未知通知 · 绑定身份未核实" }
            let phase = event.kind == .candidatePressed ? "按下" : "释放"
            return "候选绑定 \(binding) · \(phase)（未核实）"
        case .inputStreamGap:
            return "事件流有缺口"
        default:
            return "未知通知 · 物理控件、键帽覆盖和动作角色尚未核实"
        }
    }
    var candidateSourceSummary: String? {
        guard let event = lastEvent,
              event.kind == .candidatePressed || event.kind == .candidateReleased else { return nil }
        return "Dispatch 0.6.2 候选 · 当前固件输入与物理映射未核实"
    }

    func stop() {
        generation += 1
        subscription?.close(); subscription = nil
    }
    func listen(store: Store) async {
        stop()
        let token = generation
        count = 0; lost = 0; lastEvent = nil; paused = false
        guard store.connectedID != nil else { status = "连接设备后持续监听"; return }
        do {
            let useVerified = store.hostExecutionEnabled || ["0.6.3", "synthetic-0.6.3"].contains(store.identity?.version ?? "")
            let stream = useVerified ? try await store.subscribeVerifiedInput() : try await store.subscribeInput()
            guard token == generation, !Task.isCancelled else { stream.close(); return }
            subscription = stream
            status = useVerified ? "正在监听 · 已验证物理映射与宽键合并生效" : "正在监听 · 仅观察已有绑定，物理映射待核实"
            await withTaskCancellationHandler(operation: {
                while !Task.isCancelled, let event = await stream.nextEvent() {
                    guard token == generation else { break }
                    lastEvent = event
                    if event.kind == .inputStreamGap {
                        lost += event.lostCount; paused = true
                        status = "输入中断 · 已清除按住状态，请重建监听"
                        break
                    }
                    count += 1
                }
            }, onCancel: { stream.close() })
            stream.close()
            if token == generation {
                subscription = nil
                if !paused { status = "监听已停止 · 可重新连接或重建监听" }
            }
        } catch {
            if token == generation { status = "监听不可用 · \(readableError(error))" }
        }
    }
}
struct InputMonitorPanel: View {
    @ObservedObject var store: Store
    @StateObject private var monitor = InputMonitorState()
    @State private var revision = 0
    var body: some View {
        HStack(alignment: .top, spacing: 16) {
            Label("输入监视", systemImage: "waveform.path").font(.caption.bold())
            VStack(alignment: .leading, spacing: 4) {
                Text(monitor.status)
                Text("已收 \(monitor.count) 条通知 · 丢失 \(monitor.lost) 条 · 监视器自身不触发动作")
                    .foregroundStyle(.secondary)
                if let event = monitor.lastEvent {
                    if let summary = monitor.eventSummary { Text(summary) }
                    if let source = monitor.candidateSourceSummary {
                        Text(source).foregroundStyle(.secondary)
                    }
                    Text("会话 \(event.sessionId) · 本地序号 \(event.sequence) · 接收时间 \(Date(timeIntervalSince1970: Double(event.timestampMs) / 1000).formatted(date: .omitted, time: .standard))")
                        .foregroundStyle(.secondary)
                }
            }.font(.caption)
            Spacer()
            Button("重建监听") { revision += 1 }
                .disabled(store.connectedID == nil)
        }.padding(.horizontal, 20).padding(.vertical, 8)
        .task(id: "\(store.connectedID ?? "")/\(store.hostExecutionEnabled)/\(store.identity?.version ?? "")/\(revision)") { await monitor.listen(store: store) }
        .onDisappear { monitor.stop() }
    }
}
