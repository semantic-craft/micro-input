import Foundation

enum T3Failure: String, Error {
    case discovery, incompatible, authDenied, transport, oversized, expired, revocation, process
    var message: String {
        switch self {
        case .discovery: return "未找到本机 T3 服务或 CLI"
        case .incompatible: return "T3 状态格式不兼容"
        case .authDenied: return "T3 只读授权失败"
        case .transport: return "T3 连接中断"
        case .oversized: return "T3 响应超过读取上限"
        case .expired: return "T3 只读连接已到期，请重连"
        case .revocation: return "T3 撤销未确认，短期授权将自动到期"
        case .process: return "T3 CLI 未正常完成"
        }
    }
}

struct T3ThreadIdentity: Hashable {
    let environmentID: String
    let threadID: String
    var deepLink: URL? {
        let safe = CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "-._~"))
        guard let environment = environmentID.addingPercentEncoding(withAllowedCharacters: safe),
              let thread = threadID.addingPercentEncoding(withAllowedCharacters: safe) else { return nil }
        return URL(string: "t3code://threads/\(environment)/\(thread)")
    }
}
enum T3AttentionReason: String {
    case request, guest, failure, plan, completion
    var priority: Int {
        switch self {
        case .request: return 0
        case .guest: return 1
        case .failure: return 2
        case .plan: return 3
        case .completion: return 4
        }
    }
    var label: String {
        switch self {
        case .request: return "等待回应"
        case .guest: return "访客请求"
        case .failure: return "失败未读"
        case .plan: return "计划待查看"
        case .completion: return "完成未读"
        }
    }
}
struct T3AttentionEvent: Hashable {
    let identity: T3ThreadIdentity
    let reason: T3AttentionReason
    let eventID: String
}
struct T3AttentionItem: Identifiable {
    let event: T3AttentionEvent
    let title: String
    let since: Date
    let firstObserved: Bool
    var id: T3AttentionEvent { event }
}
struct T3Selection: Equatable {
    let event: T3AttentionEvent
    let generation: UInt64
}
struct T3Visit: Decodable {
    let date: Date?
    init(from decoder: Decoder) throws {
        let value = try decoder.singleValueContainer()
        if value.decodeNil() { date = nil }
        else {
            guard let parsed = T3Shell.date(try value.decode(String.self)) else { throw T3Failure.incompatible }
            date = parsed
        }
    }
}
struct T3NullableString: Decodable {
    let value: String?
    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        value = container.decodeNil() ? nil : try container.decode(String.self)
    }
}
struct T3ShellThread: Decodable {
    struct Lineage: Decodable { let relationshipToParent: String?; let parentThreadId: String?; let sideChat: Bool? }
    struct Request: Decodable { let id: String; let kind: String; let createdAt: String; let blocking: Bool? }
    struct Background: Decodable { let kind: String; let awaited: Bool? }
    let id: String, title: String, status: String, updatedAt: String, interactionMode: String
    let lineage: Lineage
    let creationSource: String?
    let latestRunId: String?, latestRunCompletedAt: String?, activeRunId: String?
    let activityRunStatus: T3NullableString
    let pendingRuntimeRequest: Request?
    let pendingGuestMessageCount: Int?
    let hasActionableProposedPlan: Bool
    let pendingBackgroundTasks: [Background]?
    let archivedAt: String?, deletedAt: String?, snoozedUntil: String?, snoozedAt: String?
    let lastVisitedAt: T3Visit
    var backgroundHolds: Bool {
        (pendingBackgroundTasks ?? []).contains { $0.awaited == true || !["command", "monitor"].contains($0.kind) }
    }
    var actionableRequest: Request? {
        guard let request = pendingRuntimeRequest,
              ["command", "file-read", "file-change", "permission", "mcp-elicitation", "user_input", "dynamic_tool_call"].contains(request.kind),
              !request.id.isEmpty else { return nil }
        let working = !backgroundHolds && ["preparing", "starting", "running"].contains(activityRunStatus.value ?? status)
        return request.kind == "user_input" && request.blocking == false && working ? nil : request
    }
}
struct T3Shell: Decodable {
    let schemaVersion: Int
    let snapshotSequence: UInt64
    let threads: [T3ShellThread]
    static func date(_ value: String?) -> Date? {
        guard let value else { return nil }
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if let date = formatter.date(from: value) { return date }
        formatter.formatOptions = [.withInternetDateTime]
        return formatter.date(from: value)
    }
    static func decode(_ data: Data) throws -> T3Shell {
        guard data.count <= 8 * 1024 * 1024 else { throw T3Failure.oversized }
        guard let shell = try? JSONDecoder().decode(T3Shell.self, from: data), shell.schemaVersion == 2,
              shell.threads.count <= 10000, Set(shell.threads.map(\.id)).count == shell.threads.count,
              shell.threads.allSatisfy({ !$0.id.isEmpty && $0.id.utf8.count <= 512 }) else { throw T3Failure.incompatible }
        return shell
    }
    func attention(environmentID: String, now: Date, observed: inout [T3AttentionEvent: Date]) -> [T3AttentionItem] {
        var items: [T3AttentionItem] = []
        let byID = Dictionary(uniqueKeysWithValues: threads.map { ($0.id, $0) })
        var inherited: [String: [(String, T3ShellThread.Request)]] = [:]
        for child in threads {
            guard let request = child.actionableRequest else { continue }
            if child.lineage.sideChat == true, let parent = child.lineage.parentThreadId {
                inherited[parent, default: []].append((child.id, request))
            } else if child.lineage.relationshipToParent == "subagent", child.creationSource != "provider",
                      child.activityRunStatus.value != nil, request.kind != "user_input" {
                var parentID = child.lineage.parentThreadId
                var seen: Set<String> = [child.id]
                while let id = parentID, !seen.contains(id), let parent = byID[id] {
                    seen.insert(id)
                    inherited[id, default: []].append((child.id, request))
                    parentID = parent.lineage.relationshipToParent == "subagent" || parent.lineage.sideChat == true ? parent.lineage.parentThreadId : nil
                }
            }
        }
        for thread in threads {
            guard thread.archivedAt == nil, thread.deletedAt == nil,
                  thread.lineage.relationshipToParent != "subagent", thread.lineage.sideChat != true else { continue }
            let identity = T3ThreadIdentity(environmentID: environmentID, threadID: thread.id)
            var reason: T3AttentionReason
            var eventID: String
            var timestamp: Date?
            let requests = (thread.actionableRequest.map { [(thread.id, $0)] } ?? []) + (inherited[thread.id] ?? [])
            if !requests.isEmpty {
                reason = .request
                eventID = requests.map { "\($0.0.utf8.count):\($0.0)\($0.1.id.utf8.count):\($0.1.id)" }.sorted().joined(separator: "|")
                timestamp = requests.compactMap { Self.date($0.1.createdAt) }.min()
            } else if (thread.pendingGuestMessageCount ?? 0) > 0 {
                reason = .guest; eventID = "\(thread.pendingGuestMessageCount ?? 0):\(thread.updatedAt)"; timestamp = Self.date(thread.updatedAt)
            } else {
                guard !["queued", "preparing", "starting", "running", "waiting"].contains(thread.status),
                      thread.activeRunId == nil, thread.activityRunStatus.value == nil,
                      !thread.backgroundHolds,
                      let run = thread.latestRunId else { continue }
                eventID = run; timestamp = Self.date(thread.latestRunCompletedAt)
                if thread.status == "failed", thread.lastVisitedAt.date == nil || timestamp == nil || timestamp! > thread.lastVisitedAt.date! {
                    reason = .failure
                } else if thread.interactionMode == "plan", thread.hasActionableProposedPlan,
                          ["completed", "failed", "interrupted", "cancelled", "rolled_back"].contains(thread.status) {
                    reason = .plan
                } else if let completion = timestamp, let visit = thread.lastVisitedAt.date, completion > visit,
                          ["completed", "failed", "interrupted", "idle"].contains(thread.status) {
                    reason = .completion
                } else { continue }
            }
            if let snooze = thread.snoozedUntil, let wakeAt = Self.date(snooze), wakeAt > now {
                let failureEdge = Self.date(thread.latestRunCompletedAt) ?? Self.date(thread.updatedAt)
                let newFailure = thread.status == "failed" && (thread.snoozedAt == nil ||
                    (failureEdge != nil && Self.date(thread.snoozedAt) != nil && failureEdge! > Self.date(thread.snoozedAt)!))
                let raisedHand = !requests.isEmpty || thread.pendingRuntimeRequest != nil ||
                    (thread.pendingGuestMessageCount ?? 0) > 0 || newFailure
                if !raisedHand { continue }
            }
            let event = T3AttentionEvent(identity: identity, reason: reason, eventID: eventID)
            let since = timestamp ?? observed[event] ?? now
            observed[event] = since
            let title = String(thread.title.unicodeScalars.filter { !CharacterSet.controlCharacters.contains($0) }.prefix(160).map(String.init).joined())
            items.append(T3AttentionItem(event: event, title: title.isEmpty ? "未命名会话" : title, since: since, firstObserved: timestamp == nil))
        }
        let live = Set(items.map(\.event))
        observed = observed.filter { live.contains($0.key) }
        return items.sorted {
            if $0.event.reason.priority != $1.event.reason.priority { return $0.event.reason.priority < $1.event.reason.priority }
            if $0.since != $1.since { return $0.since < $1.since }
            return $0.event.identity.threadID < $1.event.identity.threadID
        }
    }
}
struct T3AttentionRound {
    private(set) var delivered: Set<T3AttentionEvent> = []
    mutating func next(_ items: [T3AttentionItem]) -> T3AttentionEvent? {
        delivered.formIntersection(Set(items.map(\.event)))
        if let next = items.first(where: { !delivered.contains($0.event) }) { return next.event }
        return items.first?.event
    }
    mutating func record(_ event: T3AttentionEvent, items: [T3AttentionItem]) {
        if items.allSatisfy({ delivered.contains($0.event) }) { delivered.removeAll() }
        delivered.insert(event)
    }
}
