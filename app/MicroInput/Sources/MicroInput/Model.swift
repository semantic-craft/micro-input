// R-C03/R-C05/R-N03: view projections of the Rust-owned model, with raw unknown values visible.
import Foundation
indirect enum JSONValue: Codable {
    case object([String: JSONValue]), array([JSONValue]), string(String), integer(Int64), unsigned(UInt64), number(Decimal), bool(Bool), null
    init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null }
        else if let v = try? c.decode(Bool.self) { self = .bool(v) }
        else if let v = try? c.decode(Int64.self) { self = .integer(v) }
        else if let v = try? c.decode(UInt64.self) { self = .unsigned(v) }
        else if let v = try? c.decode(Decimal.self) { self = .number(v) }
        else if let v = try? c.decode(String.self) { self = .string(v) }
        else if let v = try? c.decode([JSONValue].self) { self = .array(v) }
        else { self = .object(try c.decode([String: JSONValue].self)) }
    }
    func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self { case .object(let v): try c.encode(v); case .array(let v): try c.encode(v); case .string(let v): try c.encode(v); case .integer(let v): try c.encode(v); case .unsigned(let v): try c.encode(v); case .number(let v): try c.encode(v); case .bool(let v): try c.encode(v); case .null: try c.encodeNil() }
    }
    var text: String { if case .string(let s) = self { return s }; let e = JSONEncoder(); e.outputFormatting = [.sortedKeys, .prettyPrinted]; return (try? String(decoding: e.encode(self), as: UTF8.self)) ?? "Unknown" }
    var bindingLabel: String {
        if case .string("Empty") = self { return "空" }
        guard case .object(let value) = self, let (kind, payload) = value.first else { return "Unknown · \(text)" }
        return "\(kind == "Unknown" ? "Unknown" : kind) · \(payload.text)"
    }
}
struct DeviceFileVerification: Decodable { let name: String, size: UInt64, sha1: String }
struct Snapshot: Decodable {
    let firmware: String?
    let device_files: [DeviceFileVerification]?
    let source: String, read_at: UInt64, profiles: [Profile], warnings: [String]
    let active_profile: JSONValue, raw: JSONValue
    let macros: JSONValue, multi_actions: JSONValue, smart_actions: JSONValue
}
struct Profile: Decodable { let id: JSONValue, index: Int, name: String, layers: [Layer] }
struct Layer: Decodable {
    let id: JSONValue, name: String, color: JSONValue, keys: [JSONValue], encoders: [JSONValue]
    let joystick: Joystick, lights: JSONValue
}
struct Joystick: Decodable { let kind: JSONValue, sectors: [JSONValue] }
extension Snapshot {
    func referenceDetail(_ binding: JSONValue) -> String? {
        guard case .object(let fields) = binding, let (kind, id) = fields.first else { return nil }
        let table: JSONValue
        switch kind { case "Macro": table = macros; case "Multi": table = multi_actions; case "Smart": table = smart_actions; default: return nil }
        guard case .array(let records) = table else { return "缺失引用 · \(id.text)" }
        for record in records {
            if case .object(let fields) = record, fields["id"]?.text == id.text {
                return "\(fields["name"]?.text ?? id.text)\n\(record.text)"
            }
        }
        return "缺失引用 · \(id.text)"
    }
}

// R-P03/R-E02: projection of Rust-owned offline intents, not firmware bindings.
struct DraftState: Decodable {
    var edits: [String:CatalogAction] = [:]
    var read_only = false
    var revision: UInt64 = 0
    var can_undo = false
    var can_redo = false
}
struct CatalogAction: Codable, Identifiable {
    var id: String, name: String, category: String, detail: String
    var payload: [String: JSONValue]
}

func bindingKey(profile: JSONValue, layer: JSONValue, slot: Int) -> String {
    let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
    return String(decoding: (try? encoder.encode([profile, layer, .string("logical-\(slot)")])) ?? Data(), as: UTF8.self)
}

extension CatalogAction {
    var parameterSummary: String {
        payload.keys.filter {$0 != "type"}.sorted().map { key in
            let label = ["text":"文本", "url":"网址", "key":"按键", "target":"目标", "modifiers":"修饰键", "adapter":"适配器（未验证）", "command":"命令引用", "session":"会话引用", "skill":"Skill 引用"][key] ?? key
            return "\(label)：\(payload[key]?.text ?? "")"
        }.joined(separator: "\n")
    }
}

extension Layer {
    var displayColor: String? {
        let value: UInt64
        switch color {
        case .unsigned(let n): value = n
        case .integer(let n) where n >= 0: value = UInt64(n)
        default: return nil
        }
        guard value <= 0xFFFFFF else { return nil }
        return String(format: "#%06llX", value)
    }
}
extension Snapshot {
    var smartRecords: [(String, JSONValue)] {
        guard case .object(let root) = smart_actions, case .object(let records) = root["smartActions"] else { return [] }
        func digits(_ key: String) -> String? {
            guard key.hasPrefix("SA_") else { return nil }
            let suffix = String(key.dropFirst(3))
            guard !suffix.isEmpty, suffix.utf8.allSatisfy({ $0 >= 48 && $0 <= 57 }) else { return nil }
            let trimmed = String(suffix.drop(while: { $0 == "0" }))
            return trimmed.isEmpty ? "0" : trimmed
        }
        return records.sorted { a, b in
            if let left = digits(a.key), let right = digits(b.key) {
                if left.count != right.count { return left.count < right.count }
                if left != right { return left < right }
            } else if digits(a.key) != nil { return true }
            else if digits(b.key) != nil { return false }
            return a.key < b.key
        }.map { ($0.key, $0.value) }
    }
}
