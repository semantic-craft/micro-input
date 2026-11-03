import Foundation

// Preferences reference stable profile IDs; names are used only to seed defaults.
enum MenuFavorites {
    static let defaults = ["Antigravity", "DeepSeek Coder (DSH)", "Cindy", "Codex"]
    static func resolve(ids: [String], profiles: [Profile]) -> [Profile] {
        var result: [Profile] = []
        let preferred = ids.compactMap { id in profiles.first { $0.id.text == id } }
        let fallback = defaults.compactMap { name in profiles.first { $0.name == name } }
        for profile in preferred + fallback + profiles {
            if !result.contains(where: { $0.id.text == profile.id.text }) { result.append(profile) }
            if result.count == 4 { break }
        }
        return result
    }
    static func replacing(ids: [String], at index: Int, with id: String) -> [String] {
        guard (0..<4).contains(index), index <= ids.count else { return ids }
        var result = ids
        if let old = result.firstIndex(of: id) {
            guard index < result.count else { return result }
            result.swapAt(old, index)
        } else if index < result.count {
            result[index] = id
        } else {
            result.append(id)
        }
        return result
    }
}
