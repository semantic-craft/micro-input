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
struct Snapshot: Decodable {
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
