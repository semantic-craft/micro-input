import Foundation
import SwiftUI
import MicroInputCore

struct FourAppSlot: Identifiable, Equatable, Sendable {
    var id: Int { slotIndex }
    let slotIndex: Int // 0..5
    var isPinned: Bool
    var place: KeypadNavigationPlace?
    var title: String
    var app: String
    var status: String
    var pinnedAt: Date?
    var updatedAt: Date?

    static func empty(index: Int) -> FourAppSlot {
        FourAppSlot(
            slotIndex: index,
            isPinned: false,
            place: nil,
            title: "",
            app: "",
            status: "idle",
            pinnedAt: nil,
            updatedAt: nil
        )
    }
}

@MainActor
final class FourAppSlotTable: ObservableObject {
    static let slotCount = 6
    @Published private(set) var slots: [FourAppSlot]

    init() {
        self.slots = (0..<Self.slotCount).map { FourAppSlot.empty(index: $0) }
    }

    func getSlot(_ index: Int) -> FourAppSlot? {
        guard slots.indices.contains(index) else { return nil }
        return slots[index]
    }

    func isPinned(_ index: Int) -> Bool {
        guard slots.indices.contains(index) else { return false }
        return slots[index].isPinned
    }

    func pinSlot(
        index: Int,
        place: KeypadNavigationPlace,
        title: String,
        app: String,
        status: String = "idle",
        timestamp: Date = Date()
    ) {
        guard slots.indices.contains(index) else { return }
        slots[index] = FourAppSlot(
            slotIndex: index,
            isPinned: true,
            place: place,
            title: title,
            app: app,
            status: status,
            pinnedAt: timestamp,
            updatedAt: timestamp
        )
    }

    func unpinSlot(index: Int) {
        guard slots.indices.contains(index) else { return }
        slots[index].isPinned = false
        slots[index].pinnedAt = nil
    }

    /// Fills unpinned slots with the most recent candidates from active adapters.
    /// Preserves pinned slots unchanged.
    func syncRecent(from store: Store) {
        var candidates: [(place: KeypadNavigationPlace, title: String, app: String, status: String, updatedAt: Date)] = []

        // 1. Antigravity catalog
        for row in store.antigravityStatus.rows {
            let place = KeypadNavigationPlace.antigravity(conversationId: row.conversationId, workspacePath: row.workspacePath)
            candidates.append((
                place: place,
                title: row.title,
                app: "antigravity",
                status: row.phase.rawValue,
                updatedAt: Date(timeIntervalSince1970: Double(row.transitionCount))
            ))
        }

        // 2. Cindy catalog
        for row in store.cindySessions.rows {
            let place = KeypadNavigationPlace.cindy(workspacePath: row.address.workspacePath, sessionId: row.address.sessionId)
            candidates.append((
                place: place,
                title: row.title,
                app: "cindy",
                status: row.phase,
                updatedAt: Date(timeIntervalSince1970: Double(row.eventWatermark ?? 0) / 1000.0)
            ))
        }

        // 3. ZCode catalog
        for row in store.zcodeSessions.rows {
            let place = KeypadNavigationPlace.zcode(
                sourceServer: row.address.sourceServerId ?? "",
                remoteSession: row.address.remoteSessionId ?? "",
                workspace: row.address.workspacePath,
                taskId: row.address.taskId
            )
            candidates.append((
                place: place,
                title: row.title,
                app: "zcode",
                status: row.liveStatus,
                updatedAt: Date()
            ))
        }

        // 4. DSH catalog
        for row in store.dshSessions.rows {
            let place = KeypadNavigationPlace.dsh(sessionId: row.sessionId, workspaceId: row.workspaceId, hostId: "dsh-desktop")
            candidates.append((
                place: place,
                title: row.title,
                app: "dsh",
                status: row.running ? "running" : "idle",
                updatedAt: Date()
            ))
        }

        // Sort candidates by updatedAt descending
        candidates.sort { $0.updatedAt > $1.updatedAt }

        let pinnedTargets = Set(slots.filter(\.isPinned).compactMap(\.place))
        var available = candidates.filter { !pinnedTargets.contains($0.place) }.makeIterator()

        for i in 0..<Self.slotCount {
            if slots[i].isPinned { continue }
            if let next = available.next() {
                slots[i] = FourAppSlot(
                    slotIndex: i,
                    isPinned: false,
                    place: next.place,
                    title: next.title,
                    app: next.app,
                    status: next.status,
                    pinnedAt: nil,
                    updatedAt: next.updatedAt
                )
            } else {
                slots[i] = FourAppSlot.empty(index: i)
            }
        }
    }

    /// Executes slot selection.
    /// If Cindy native ownership is active, slot actions for Cindy are delegated/simulated without double consumption.
    func executeSlot(
        _ index: Int,
        in store: Store,
        profileId: String,
        bindingId: String,
        admitted: (@MainActor () -> Bool)? = nil
    ) async -> HostActionReceipt {
        guard slots.indices.contains(index), let slot = getSlot(index), let place = slot.place else {
            let receipt = HostActionReceipt(
                timestampMs: UInt64(Date().timeIntervalSince1970 * 1000),
                profileId: profileId,
                bindingId: bindingId,
                actionType: "slot_open",
                target: "slot-\(index)",
                textByteLen: nil,
                outcome: "blocked",
                detail: "槽位 \(index) 为空"
            )
            AuditLogWriter.append(receipt)
            return receipt
        }

        // Cindy native ownership check: if native ownership is active, Cindy handles its own slots directly.
        // We report simulated/delegated without double-invoking.
        if slot.app == "cindy" && store.activeDesktopAgentAdapter(for: profileId) == "cindy" && !store.nativeOwnership.allowsExecution {
            let receipt = HostActionReceipt(
                timestampMs: UInt64(Date().timeIntervalSince1970 * 1000),
                profileId: profileId,
                bindingId: bindingId,
                actionType: "slot_open",
                target: "cindy",
                textByteLen: nil,
                outcome: "simulated",
                detail: "cindy_native_slot_delegated: slot=\(index)"
            )
            AuditLogWriter.append(receipt)
            return receipt
        }

        let checkAdmitted: @MainActor () -> Bool = {
            admitted?() ?? true
        }

        // Execute navigation to place
        let outcome: String
        let detail: String
        switch place {
        case .antigravity(let conv, let ws):
            let key = AntigravitySessionKey(conversationId: conv, workspacePath: ws)
            if let selection = store.antigravityStatus.selection(for: key) {
                let rep = await store.antigravityStatus.open(selection, admitted: checkAdmitted)
                outcome = rep.rawValue
                detail = "antigravity: \(conv)@\(ws)"
            } else {
                outcome = "unconfirmed"
                detail = "antigravity session not found"
            }
        case .cindy(let ws, let sess):
            let addr = CindyTaskAddress(workspacePath: ws, sessionId: sess)
            if let selection = store.cindySessions.selection(for: addr) {
                let rep = await store.cindySessions.open(selection, admitted: checkAdmitted)
                outcome = rep.rawValue
                detail = "cindy: \(sess)@\(ws)"
            } else {
                outcome = "unconfirmed"
                detail = "cindy task not found"
            }
        case .zcode(let src, let rem, let ws, let tid):
            let addr = ZCodeTaskAddress(workspacePath: ws, workspaceIdentity: nil, sourceServerId: src.isEmpty ? nil : src, remoteSessionId: rem.isEmpty ? nil : rem, taskId: tid)
            if let selection = store.zcodeSessions.selection(for: addr) {
                let rep = await store.zcodeSessions.open(selection, admitted: checkAdmitted)
                outcome = rep.rawValue
                detail = "zcode: \(tid)@\(ws)"
            } else {
                outcome = "unconfirmed"
                detail = "zcode task not found"
            }
        case .dsh(let sess, _, _):
            if let selection = store.dshSessions.selection(for: sess) {
                let rep = await store.dshSessions.open(selection, admitted: checkAdmitted)
                outcome = rep.rawValue
                detail = "dsh: \(sess)"
            } else {
                outcome = "unconfirmed"
                detail = "dsh session not found"
            }
        default:
            outcome = "blocked"
            detail = "unsupported slot target"
        }

        let receipt = HostActionReceipt(
            timestampMs: UInt64(Date().timeIntervalSince1970 * 1000),
            profileId: profileId,
            bindingId: bindingId,
            actionType: "slot_open",
            target: slot.app,
            textByteLen: nil,
            outcome: outcome,
            detail: detail
        )
        AuditLogWriter.append(receipt)
        return receipt
    }
}

struct FourAppWorkspaceEntry: Identifiable, Equatable, Sendable {
    var id: String { "\(app):\(workspaceId)" }
    let app: String
    let workspaceId: String
    let displayName: String
    let sessionCount: Int

    init(app: String, workspaceId: String, displayName: String, sessionCount: Int) {
        self.app = app
        self.workspaceId = workspaceId
        self.displayName = displayName
        self.sessionCount = sessionCount
    }
}

@MainActor
final class FourAppWorkspaceNavigator: ObservableObject {
    @Published private(set) var workspaces: [FourAppWorkspaceEntry] = []
    @Published private(set) var selectedIndex: Int? = nil

    init() {}

    func setWorkspaces(_ list: [FourAppWorkspaceEntry]) {
        let prev = current
        workspaces = list
        if workspaces.isEmpty {
            selectedIndex = nil
            return
        }
        if let prev, let found = workspaces.firstIndex(where: { $0.id == prev.id }) {
            selectedIndex = found
            return
        }
        selectedIndex = 0
    }

    func selectNext() -> FourAppWorkspaceEntry? {
        guard !workspaces.isEmpty else { selectedIndex = nil; return nil }
        let cur = selectedIndex ?? 0
        let next = (cur + 1) % workspaces.count
        selectedIndex = next
        return current
    }

    func selectPrevious() -> FourAppWorkspaceEntry? {
        guard !workspaces.isEmpty else { selectedIndex = nil; return nil }
        let cur = selectedIndex ?? 0
        let prev = cur == 0 ? workspaces.count - 1 : cur - 1
        selectedIndex = prev
        return current
    }

    var current: FourAppWorkspaceEntry? {
        guard let idx = selectedIndex, workspaces.indices.contains(idx) else { return nil }
        return workspaces[idx]
    }

    func syncWorkspaces(from store: Store) {
        var map: [String: (app: String, workspaceId: String, displayName: String, count: Int)] = [:]

        for row in store.antigravityStatus.rows {
            let key = "antigravity:\(row.workspacePath)"
            if let existing = map[key] {
                map[key] = (app: existing.app, workspaceId: existing.workspaceId, displayName: existing.displayName, count: existing.count + 1)
            } else {
                let name = URL(fileURLWithPath: row.workspacePath).lastPathComponent
                map[key] = (app: "antigravity", workspaceId: row.workspacePath, displayName: name.isEmpty ? row.workspacePath : name, count: 1)
            }
        }
        for row in store.cindySessions.rows {
            let key = "cindy:\(row.address.workspacePath)"
            if let existing = map[key] {
                map[key] = (app: existing.app, workspaceId: existing.workspaceId, displayName: existing.displayName, count: existing.count + 1)
            } else {
                let name = URL(fileURLWithPath: row.address.workspacePath).lastPathComponent
                map[key] = (app: "cindy", workspaceId: row.address.workspacePath, displayName: name.isEmpty ? row.address.workspacePath : name, count: 1)
            }
        }
        for row in store.zcodeSessions.rows {
            let key = "zcode:\(row.address.workspacePath)"
            if let existing = map[key] {
                map[key] = (app: existing.app, workspaceId: existing.workspaceId, displayName: existing.displayName, count: existing.count + 1)
            } else {
                let name = URL(fileURLWithPath: row.address.workspacePath).lastPathComponent
                map[key] = (app: "zcode", workspaceId: row.address.workspacePath, displayName: name.isEmpty ? row.address.workspacePath : name, count: 1)
            }
        }
        for row in store.dshSessions.rows {
            let key = "dsh:\(row.workspaceId)"
            if let existing = map[key] {
                map[key] = (app: existing.app, workspaceId: existing.workspaceId, displayName: existing.displayName, count: existing.count + 1)
            } else {
                map[key] = (app: "dsh", workspaceId: row.workspaceId, displayName: row.workspaceId, count: 1)
            }
        }

        let list = map.values.sorted { $0.workspaceId < $1.workspaceId }.map {
            FourAppWorkspaceEntry(app: $0.app, workspaceId: $0.workspaceId, displayName: $0.displayName, sessionCount: $0.count)
        }
        setWorkspaces(list)
    }

    func cycleNext(in store: Store) -> (outcome: String, detail: String) {
        syncWorkspaces(from: store)
        guard let entry = selectNext() else {
            return ("noop", "没有可用的工作区")
        }
        return ("confirmed", "\(entry.app):\(entry.workspaceId)")
    }

    func cyclePrevious(in store: Store) -> (outcome: String, detail: String) {
        syncWorkspaces(from: store)
        guard let entry = selectPrevious() else {
            return ("noop", "没有可用的工作区")
        }
        return ("confirmed", "\(entry.app):\(entry.workspaceId)")
    }
}

struct FourAppAttentionCandidate: Identifiable, Equatable, Sendable {
    var id: String { place.receiptTarget }
    let place: KeypadNavigationPlace
    let app: String
    let title: String
    let priorityRank: Int // 0: needs-interaction, 1: unread, 2: error
    let watermarkSignature: String
    let isEligible: Bool

    init(
        place: KeypadNavigationPlace,
        app: String,
        title: String,
        priorityRank: Int,
        watermarkSignature: String,
        isEligible: Bool
    ) {
        self.place = place
        self.app = app
        self.title = title
        self.priorityRank = priorityRank
        self.watermarkSignature = watermarkSignature
        self.isEligible = isEligible
    }
}

@MainActor
final class FourAppAttentionRound: ObservableObject {
    @Published private(set) var visitedSignatures: [KeypadNavigationPlace: String] = [:]
    @Published private(set) var visitedCount: Int = 0

    init() {}

    func reset() {
        visitedSignatures.removeAll()
        visitedCount = 0
    }

    func isVisited(place: KeypadNavigationPlace, signature: String) -> Bool {
        visitedSignatures[place] == signature
    }

    func collectCandidates(from store: Store) -> [FourAppAttentionCandidate] {
        var candidates: [FourAppAttentionCandidate] = []

        // Antigravity candidates:
        // Rank 0: pendingInteraction != nil
        // Rank 2: error / terminationReason
        // Unread is unknown/nil in Antigravity -> NEVER inferred as unread.
        for row in store.antigravityStatus.rows {
            let hasInteraction = row.pendingInteraction != nil
            let isError = row.phase == .failed || row.terminationReason != nil
            let eligible = hasInteraction || isError
            let rank = hasInteraction ? 0 : 2
            let sig = "\(row.transitionCount):\(row.stepIdx):\(row.pendingInteraction?.interactionId ?? ""):\(row.phase.rawValue)"
            let place = KeypadNavigationPlace.antigravity(conversationId: row.conversationId, workspacePath: row.workspacePath)
            candidates.append(FourAppAttentionCandidate(
                place: place,
                app: "antigravity",
                title: row.title,
                priorityRank: rank,
                watermarkSignature: sig,
                isEligible: eligible
            ))
        }

        // Cindy candidates:
        // Respects isEligibleForAttentionRound:
        // Rank 0: needs-interaction
        // Rank 1: unread == true
        // Rank 2: error
        // Completed sessions with unread == false/nil are NOT eligible.
        for row in store.cindySessions.rows {
            let eligible = row.isEligibleForAttentionRound
            let rank = row.attentionPriorityRank
            let sig = row.roundWatermarkSignature
            let place = KeypadNavigationPlace.cindy(workspacePath: row.address.workspacePath, sessionId: row.address.sessionId)
            candidates.append(FourAppAttentionCandidate(
                place: place,
                app: "cindy",
                title: row.title,
                priorityRank: rank,
                watermarkSignature: sig,
                isEligible: eligible
            ))
        }

        // ZCode candidates:
        // Rank 0: pendingInteraction != nil || liveStatus == "waiting"
        // Rank 2: liveStatus == "error"
        // Unread is unknown/nil -> NEVER inferred as unread.
        for row in store.zcodeSessions.rows {
            let hasInteraction = row.pendingInteraction != nil || row.liveStatus == "waiting"
            let isError = row.liveStatus == "error"
            let eligible = hasInteraction || isError
            let rank = hasInteraction ? 0 : 2
            let sig = "\(row.address.taskId):\(row.pendingInteraction?.interactionId ?? ""):\(row.liveStatus)"
            let place = KeypadNavigationPlace.zcode(
                sourceServer: row.address.sourceServerId ?? "",
                remoteSession: row.address.remoteSessionId ?? "",
                workspace: row.address.workspacePath,
                taskId: row.address.taskId
            )
            candidates.append(FourAppAttentionCandidate(
                place: place,
                app: "zcode",
                title: row.title,
                priorityRank: rank,
                watermarkSignature: sig,
                isEligible: eligible
            ))
        }

        // DSH candidates:
        // Rank 0: pendingInteraction != nil || pendingInput > 0
        // Unread is unknown/nil -> NEVER inferred as unread.
        for row in store.dshSessions.rows {
            let hasInteraction = row.pendingInteraction != nil || row.pendingInput > 0
            let eligible = hasInteraction
            let rank = 0
            let sig = "\(row.sessionId):\(row.pendingInput):\(row.pendingInteraction?.interactionId ?? ""):\(row.currentTurn ?? 0)"
            let place = KeypadNavigationPlace.dsh(sessionId: row.sessionId, workspaceId: row.workspaceId, hostId: "dsh-desktop")
            candidates.append(FourAppAttentionCandidate(
                place: place,
                app: "dsh",
                title: row.title,
                priorityRank: rank,
                watermarkSignature: sig,
                isEligible: eligible
            ))
        }

        return candidates
    }

    func advanceAttentionRound(
        in store: Store,
        profileId: String,
        bindingId: String,
        allowWrap: Bool = false,
        admitted: (@MainActor () -> Bool)? = nil
    ) async -> HostActionReceipt {
        let candidates = collectCandidates(from: store)
        var eligible = candidates.filter(\.isEligible)

        guard !eligible.isEmpty else {
            let receipt = HostActionReceipt(
                timestampMs: UInt64(Date().timeIntervalSince1970 * 1000),
                profileId: profileId,
                bindingId: bindingId,
                actionType: "four_app_attention",
                target: "attention",
                textByteLen: nil,
                outcome: "noop",
                detail: "round.empty"
            )
            AuditLogWriter.append(receipt)
            return receipt
        }

        // Sort by priority rank (0: interaction, 1: unread, 2: error)
        eligible.sort { lhs, rhs in
            if lhs.priorityRank != rhs.priorityRank {
                return lhs.priorityRank < rhs.priorityRank
            }
            return lhs.title < rhs.title
        }

        // Prune visited signatures for places no longer present
        let currentPlaces = Set(eligible.map(\.place))
        visitedSignatures = visitedSignatures.filter { currentPlaces.contains($0.key) }

        // Find next candidate not yet visited with current watermark signature
        var nextCandidate = eligible.first(where: { visitedSignatures[$0.place] != $0.watermarkSignature })

        if nextCandidate == nil && allowWrap {
            reset()
            nextCandidate = eligible.first
        }

        guard let target = nextCandidate else {
            let receipt = HostActionReceipt(
                timestampMs: UInt64(Date().timeIntervalSince1970 * 1000),
                profileId: profileId,
                bindingId: bindingId,
                actionType: "four_app_attention",
                target: "attention",
                textByteLen: nil,
                outcome: "noop",
                detail: "round.exhausted"
            )
            AuditLogWriter.append(receipt)
            return receipt
        }

        visitedSignatures[target.place] = target.watermarkSignature
        visitedCount += 1

        let checkAdmitted: @MainActor () -> Bool = {
            admitted?() ?? true
        }

        // Open target session
        let outcome: String
        let detail: String
        switch target.place {
        case .antigravity(let conv, let ws):
            let key = AntigravitySessionKey(conversationId: conv, workspacePath: ws)
            if let sel = store.antigravityStatus.selection(for: key) {
                let rep = await store.antigravityStatus.open(sel, admitted: checkAdmitted)
                outcome = rep.rawValue
                detail = "antigravity_attention: \(conv)@\(ws)"
            } else {
                outcome = "unconfirmed"
                detail = "antigravity session not found"
            }
        case .cindy(let ws, let sess):
            let addr = CindyTaskAddress(workspacePath: ws, sessionId: sess)
            if let sel = store.cindySessions.selection(for: addr) {
                let rep = await store.cindySessions.open(sel, admitted: checkAdmitted)
                outcome = rep.rawValue
                detail = "cindy_attention: \(sess)@\(ws)"
            } else {
                outcome = "unconfirmed"
                detail = "cindy task not found"
            }
        case .zcode(let src, let rem, let ws, let tid):
            let addr = ZCodeTaskAddress(workspacePath: ws, workspaceIdentity: nil, sourceServerId: src.isEmpty ? nil : src, remoteSessionId: rem.isEmpty ? nil : rem, taskId: tid)
            if let sel = store.zcodeSessions.selection(for: addr) {
                let rep = await store.zcodeSessions.open(sel, admitted: checkAdmitted)
                outcome = rep.rawValue
                detail = "zcode_attention: \(tid)@\(ws)"
            } else {
                outcome = "unconfirmed"
                detail = "zcode task not found"
            }
        case .dsh(let sess, _, _):
            if let sel = store.dshSessions.selection(for: sess) {
                let rep = await store.dshSessions.open(sel, admitted: checkAdmitted)
                outcome = rep.rawValue
                detail = "dsh_attention: \(sess)"
            } else {
                outcome = "unconfirmed"
                detail = "dsh session not found"
            }
        default:
            outcome = "blocked"
            detail = "unsupported attention target"
        }

        let receipt = HostActionReceipt(
            timestampMs: UInt64(Date().timeIntervalSince1970 * 1000),
            profileId: profileId,
            bindingId: bindingId,
            actionType: "four_app_attention",
            target: target.app,
            textByteLen: nil,
            outcome: outcome,
            detail: detail
        )
        AuditLogWriter.append(receipt)
        return receipt
    }
}

extension Store {
    func highestPriorityFourAppTarget() -> FourAppAttentionCandidate? {
        fourAppAttention.collectCandidates(from: self)
            .filter(\.isEligible)
            .min { lhs, rhs in
                if lhs.priorityRank != rhs.priorityRank {
                    return lhs.priorityRank < rhs.priorityRank
                }
                return lhs.title < rhs.title
            }
    }
}
