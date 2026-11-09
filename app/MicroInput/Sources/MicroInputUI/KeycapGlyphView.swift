import SwiftUI

public enum CodexKeycapId: String, CaseIterable, Identifiable {
    case blank = "blank"
    case fast = "fast"
    case approve = "approve"
    case decline = "decline"
    case split = "split"
    case mic = "mic"
    case codex = "codex"
    case bug = "bug"
    case openai = "openai"
    case term = "term"
    case download = "download"
    case delete = "delete"
    case new = "new"
    case nav = "nav"
    case magic = "magic"
    case diff = "diff"
    case play = "play"
    case git = "git"
    case branchAlt = "branch-alt"
    case branch = "branch"
    case merge = "merge"
    case pr = "pr"
    case paint = "paint"
    case lab = "lab"
    case party = "party"
    case time = "time"
    case mindPlus = "mind-plus"
    case mindMinus = "mind-minus"
    case setup = "setup"
    case folder = "folder"
    case upload = "upload"
    case apps = "apps"
    case yolo = "yolo"
    case yeet = "yeet"

    public var id: String { rawValue }

    public init(rustVariant: String) {
        switch rustVariant {
        case "Blank": self = .blank
        case "Fast": self = .fast
        case "Approve": self = .approve
        case "Decline": self = .decline
        case "Split": self = .split
        case "Mic": self = .mic
        case "Codex": self = .codex
        case "Bug": self = .bug
        case "OpenAi": self = .openai
        case "Term": self = .term
        case "Download": self = .download
        case "Delete": self = .delete
        case "New": self = .new
        case "Nav": self = .nav
        case "Magic": self = .magic
        case "Diff": self = .diff
        case "Play": self = .play
        case "Git": self = .git
        case "BranchAlt": self = .branchAlt
        case "Branch": self = .branch
        case "Merge": self = .merge
        case "Pr": self = .pr
        case "Paint": self = .paint
        case "Lab": self = .lab
        case "Party": self = .party
        case "Time": self = .time
        case "MindPlus": self = .mindPlus
        case "MindMinus": self = .mindMinus
        case "Setup": self = .setup
        case "Folder": self = .folder
        case "Upload": self = .upload
        case "Apps": self = .apps
        case "Yolo": self = .yolo
        case "Yeet": self = .yeet
        default: self = .blank
        }
    }

    public var rustVariant: String {
        switch self {
        case .blank: return "Blank"
        case .fast: return "Fast"
        case .approve: return "Approve"
        case .decline: return "Decline"
        case .split: return "Split"
        case .mic: return "Mic"
        case .codex: return "Codex"
        case .bug: return "Bug"
        case .openai: return "OpenAi"
        case .term: return "Term"
        case .download: return "Download"
        case .delete: return "Delete"
        case .new: return "New"
        case .nav: return "Nav"
        case .magic: return "Magic"
        case .diff: return "Diff"
        case .play: return "Play"
        case .git: return "Git"
        case .branchAlt: return "BranchAlt"
        case .branch: return "Branch"
        case .merge: return "Merge"
        case .pr: return "Pr"
        case .paint: return "Paint"
        case .lab: return "Lab"
        case .party: return "Party"
        case .time: return "Time"
        case .mindPlus: return "MindPlus"
        case .mindMinus: return "MindMinus"
        case .setup: return "Setup"
        case .folder: return "Folder"
        case .upload: return "Upload"
        case .apps: return "Apps"
        case .yolo: return "Yolo"
        case .yeet: return "Yeet"
        }
    }
}

public struct CodexKeycapGlyphView: View, Equatable {
    public let keycap: CodexKeycapId
    public var size: CGFloat = 22
    public var color: Color = .white

    public init(keycap: CodexKeycapId, size: CGFloat = 22, color: Color = .white) {
        self.keycap = keycap
        self.size = size
        self.color = color
    }

    public var body: some View {
        Canvas { context, canvasSize in
            let scale = min(canvasSize.width, canvasSize.height) / 20.0
            func pt(_ x: CGFloat, _ y: CGFloat) -> CGPoint {
                CGPoint(x: x * scale, y: y * scale)
            }
            func rect(_ x: CGFloat, _ y: CGFloat, _ w: CGFloat, _ h: CGFloat) -> CGRect {
                CGRect(x: x * scale, y: y * scale, width: w * scale, height: h * scale)
            }
            let stroke = StrokeStyle(lineWidth: 1.65 * scale, lineCap: .round, lineJoin: .round)
            let thinStroke = StrokeStyle(lineWidth: 1.4 * scale, lineCap: .round, lineJoin: .round)

            switch keycap {
            case .blank:
                let r = RoundedRectangle(cornerRadius: 2.5 * scale).path(in: rect(5.5, 5.5, 9, 9))
                context.stroke(r, with: .color(color.opacity(0.35)), style: thinStroke)

            case .fast:
                var p = Path()
                p.move(to: pt(11.2, 2.8))
                p.addLine(to: pt(5.6, 10.4))
                p.addLine(to: pt(9.6, 10.4))
                p.addLine(to: pt(8.8, 17.2))
                p.addLine(to: pt(14.4, 9.6))
                p.addLine(to: pt(10.4, 9.6))
                p.closeSubpath()
                context.stroke(p, with: .color(color), style: stroke)

            case .approve:
                let c = Circle().path(in: rect(3.6, 3.6, 12.8, 12.8))
                context.stroke(c, with: .color(color), style: stroke)
                var p = Path()
                p.move(to: pt(7.3, 10.1))
                p.addLine(to: pt(9.2, 12.0))
                p.addLine(to: pt(12.9, 8.1))
                context.stroke(p, with: .color(color), style: stroke)

            case .decline:
                let c = Circle().path(in: rect(3.6, 3.6, 12.8, 12.8))
                context.stroke(c, with: .color(color), style: stroke)
                var p = Path()
                p.move(to: pt(7.8, 7.8))
                p.addLine(to: pt(12.2, 12.2))
                p.move(to: pt(12.2, 7.8))
                p.addLine(to: pt(7.8, 12.2))
                context.stroke(p, with: .color(color), style: stroke)

            case .split:
                var p = Path()
                p.move(to: pt(4.2, 10))
                p.addLine(to: pt(8.4, 10))
                p.addLine(to: pt(11.2, 6.2))
                p.addLine(to: pt(15.4, 6.2))
                p.move(to: pt(13.2, 4.2))
                p.addLine(to: pt(15.6, 6.2))
                p.addLine(to: pt(13.2, 8.2))
                p.move(to: pt(8.4, 10))
                p.addLine(to: pt(11.2, 13.8))
                p.addLine(to: pt(15.4, 13.8))
                p.move(to: pt(13.2, 11.8))
                p.addLine(to: pt(15.6, 13.8))
                p.addLine(to: pt(13.2, 15.8))
                context.stroke(p, with: .color(color), style: stroke)

            case .mic:
                let cap = RoundedRectangle(cornerRadius: 2.6 * scale).path(in: rect(7.4, 3.6, 5.2, 7.8))
                context.stroke(cap, with: .color(color), style: stroke)
                var p = Path()
                p.addArc(center: pt(10, 9.6), radius: 4.5 * scale, startAngle: .degrees(0), endAngle: .degrees(180), clockwise: false)
                p.move(to: pt(10, 14.1))
                p.addLine(to: pt(10, 16.6))
                p.move(to: pt(7.5, 16.6))
                p.addLine(to: pt(12.5, 16.6))
                context.stroke(p, with: .color(color), style: stroke)

            case .codex:
                let c = RoundedRectangle(cornerRadius: 4.6 * scale).path(in: rect(3.6, 3.8, 12.8, 12.4))
                context.stroke(c, with: .color(color), style: thinStroke)
                var p = Path()
                p.move(to: pt(7.2, 8.4))
                p.addLine(to: pt(9.2, 10.0))
                p.addLine(to: pt(7.2, 11.6))
                p.move(to: pt(10.4, 11.6))
                p.addLine(to: pt(12.8, 11.6))
                context.stroke(p, with: .color(color), style: thinStroke)

            case .bug:
                let bodyRect = RoundedRectangle(cornerRadius: 3.4 * scale).path(in: rect(6.6, 6.8, 6.8, 8.2))
                context.stroke(bodyRect, with: .color(color), style: stroke)
                var p = Path()
                p.move(to: pt(4.2, 9.2))
                p.addLine(to: pt(6.5, 9.2))
                p.move(to: pt(13.5, 9.2))
                p.addLine(to: pt(15.8, 9.2))
                p.move(to: pt(4.6, 13.0))
                p.addLine(to: pt(6.6, 12.2))
                p.move(to: pt(15.4, 13.0))
                p.addLine(to: pt(13.4, 12.2))
                context.stroke(p, with: .color(color), style: thinStroke)

            case .openai:
                let outer = Circle().path(in: rect(3.8, 3.8, 12.4, 12.4))
                let inner = Circle().path(in: rect(7.6, 7.6, 4.8, 4.8))
                context.stroke(outer, with: .color(color), style: thinStroke)
                context.stroke(inner, with: .color(color), style: thinStroke)

            case .term:
                let box = RoundedRectangle(cornerRadius: 2.4 * scale).path(in: rect(3.4, 4.4, 13.2, 11.2))
                context.stroke(box, with: .color(color), style: stroke)
                var p = Path()
                p.move(to: pt(6.6, 8.2))
                p.addLine(to: pt(8.8, 10.0))
                p.addLine(to: pt(6.6, 11.8))
                p.move(to: pt(10.2, 11.8))
                p.addLine(to: pt(13.4, 11.8))
                context.stroke(p, with: .color(color), style: stroke)

            case .download:
                var p = Path()
                p.move(to: pt(10, 4.0))
                p.addLine(to: pt(10, 12.4))
                p.move(to: pt(7.0, 9.6))
                p.addLine(to: pt(10, 12.6))
                p.addLine(to: pt(13.0, 9.6))
                p.move(to: pt(4.8, 15.8))
                p.addLine(to: pt(15.2, 15.8))
                context.stroke(p, with: .color(color), style: stroke)

            case .delete:
                var p = Path()
                p.move(to: pt(5.2, 6.2))
                p.addLine(to: pt(14.8, 6.2))
                p.move(to: pt(8.0, 6.2))
                p.addLine(to: pt(8.0, 4.6))
                p.addLine(to: pt(12.0, 4.6))
                p.addLine(to: pt(12.0, 6.2))
                p.move(to: pt(6.6, 6.2))
                p.addLine(to: pt(7.2, 15.4))
                p.addLine(to: pt(12.8, 15.4))
                p.addLine(to: pt(13.4, 6.2))
                context.stroke(p, with: .color(color), style: thinStroke)

            case .new:
                var p = Path()
                p.move(to: pt(11.6, 4.4))
                p.addLine(to: pt(5.4, 4.4))
                p.addLine(to: pt(5.4, 15.4))
                p.addLine(to: pt(15.4, 15.4))
                p.addLine(to: pt(15.4, 9.2))
                p.move(to: pt(9.4, 10.6))
                p.addLine(to: pt(15.6, 4.4))
                context.stroke(p, with: .color(color), style: stroke)

            case .nav:
                var p = Path()
                p.move(to: pt(4.2, 9.4))
                p.addLine(to: pt(15.6, 4.6))
                p.addLine(to: pt(10.8, 16.0))
                p.addLine(to: pt(8.9, 11.3))
                p.closeSubpath()
                context.stroke(p, with: .color(color), style: stroke)

            case .magic:
                var p = Path()
                p.move(to: pt(10, 3.6))
                p.addLine(to: pt(11.9, 7.6))
                p.addLine(to: pt(16.2, 8.2))
                p.addLine(to: pt(13.1, 11.2))
                p.addLine(to: pt(13.8, 15.5))
                p.addLine(to: pt(10, 13.5))
                p.addLine(to: pt(6.2, 15.5))
                p.addLine(to: pt(6.9, 11.2))
                p.addLine(to: pt(3.8, 8.2))
                p.addLine(to: pt(8.1, 7.6))
                p.closeSubpath()
                context.stroke(p, with: .color(color), style: thinStroke)

            case .diff:
                let box = RoundedRectangle(cornerRadius: 2.4 * scale).path(in: rect(4.2, 4.2, 11.6, 11.6))
                context.stroke(box, with: .color(color), style: stroke)
                var p = Path()
                p.move(to: pt(10, 6.8))
                p.addLine(to: pt(10, 10.4))
                p.move(to: pt(8.2, 8.6))
                p.addLine(to: pt(11.8, 8.6))
                p.move(to: pt(8.2, 13.0))
                p.addLine(to: pt(11.8, 13.0))
                context.stroke(p, with: .color(color), style: stroke)

            case .git:
                let n1 = Circle().path(in: rect(4.8, 4.2, 3.2, 3.2))
                let n2 = Circle().path(in: rect(4.8, 12.6, 3.2, 3.2))
                let n3 = Circle().path(in: rect(12.0, 8.4, 3.2, 3.2))
                context.stroke(n1, with: .color(color), style: thinStroke)
                context.stroke(n2, with: .color(color), style: thinStroke)
                context.stroke(n3, with: .color(color), style: thinStroke)
                var p = Path()
                p.move(to: pt(6.4, 7.4))
                p.addLine(to: pt(6.4, 12.6))
                p.move(to: pt(8.0, 5.8))
                p.addLine(to: pt(12.0, 9.2))
                context.stroke(p, with: .color(color), style: thinStroke)

            case .branchAlt:
                var p = Path()
                p.move(to: pt(6.0, 15.6))
                p.addLine(to: pt(6.0, 4.8))
                p.move(to: pt(6.0, 10.6))
                p.addLine(to: pt(13.8, 6.2))
                p.move(to: pt(11.2, 5.6))
                p.addLine(to: pt(14.0, 6.0))
                p.addLine(to: pt(13.2, 8.8))
                context.stroke(p, with: .color(color), style: stroke)

            case .branch:
                let topNode = Circle().path(in: rect(12.0, 4.4, 3.2, 3.2))
                let botNode = Circle().path(in: rect(4.8, 12.4, 3.2, 3.2))
                context.stroke(topNode, with: .color(color), style: thinStroke)
                context.stroke(botNode, with: .color(color), style: thinStroke)
                var p = Path()
                p.move(to: pt(6.4, 4.6))
                p.addLine(to: pt(6.4, 12.4))
                p.move(to: pt(6.4, 10.0))
                p.addLine(to: pt(13.6, 10.0))
                p.addLine(to: pt(13.6, 7.6))
                context.stroke(p, with: .color(color), style: stroke)

            case .merge:
                var p = Path()
                p.move(to: pt(5.4, 4.8))
                p.addLine(to: pt(10.0, 10.2))
                p.move(to: pt(14.6, 4.8))
                p.addLine(to: pt(10.0, 10.2))
                p.addLine(to: pt(10.0, 15.6))
                p.move(to: pt(7.6, 13.2))
                p.addLine(to: pt(10.0, 15.6))
                p.addLine(to: pt(12.4, 13.2))
                context.stroke(p, with: .color(color), style: stroke)

            case .pr:
                let leftDot = Circle().path(in: rect(4.6, 4.4, 3.0, 3.0))
                let rightDot = Circle().path(in: rect(12.4, 12.6, 3.0, 3.0))
                context.stroke(leftDot, with: .color(color), style: thinStroke)
                context.stroke(rightDot, with: .color(color), style: thinStroke)
                var p = Path()
                p.move(to: pt(6.1, 7.4))
                p.addLine(to: pt(6.1, 15.4))
                p.move(to: pt(10.4, 5.8))
                p.addLine(to: pt(13.9, 5.8))
                p.addLine(to: pt(13.9, 12.6))
                context.stroke(p, with: .color(color), style: stroke)

            case .paint:
                let palette = Circle().path(in: rect(3.8, 3.8, 12.4, 12.4))
                context.stroke(palette, with: .color(color), style: stroke)
                for (x, y) in [(7.2, 6.8), (11.2, 6.8), (6.8, 10.8)] {
                    let dot = Circle().path(in: rect(x, y, 1.8, 1.8))
                    context.fill(dot, with: .color(color))
                }

            case .lab:
                var p = Path()
                p.move(to: pt(8.0, 4.4))
                p.addLine(to: pt(12.0, 4.4))
                p.move(to: pt(8.8, 4.4))
                p.addLine(to: pt(8.8, 8.2))
                p.addLine(to: pt(5.0, 15.2))
                p.addLine(to: pt(15.0, 15.2))
                p.addLine(to: pt(11.2, 8.2))
                p.addLine(to: pt(11.2, 4.4))
                context.stroke(p, with: .color(color), style: stroke)

            case .party:
                var p = Path()
                for (x, y1, y2) in [(4.8, 8.2, 11.8), (7.4, 5.8, 14.2), (10.0, 4.0, 16.0), (12.6, 6.4, 13.6), (15.2, 8.4, 11.6)] {
                    p.move(to: pt(x, y1))
                    p.addLine(to: pt(x, y2))
                }
                context.stroke(p, with: .color(color), style: stroke)

            case .time:
                let c = Circle().path(in: rect(3.8, 3.8, 12.4, 12.4))
                context.stroke(c, with: .color(color), style: stroke)
                var p = Path()
                p.move(to: pt(10.0, 6.4))
                p.addLine(to: pt(10.0, 10.2))
                p.addLine(to: pt(12.8, 11.8))
                context.stroke(p, with: .color(color), style: stroke)

            case .yolo:
                var p = Path()
                p.move(to: pt(5.0, 5.6))
                p.addLine(to: pt(9.8, 10.0))
                p.addLine(to: pt(5.0, 14.4))
                p.move(to: pt(10.4, 5.6))
                p.addLine(to: pt(15.2, 10.0))
                p.addLine(to: pt(10.4, 14.4))
                context.stroke(p, with: .color(color), style: stroke)

            case .yeet:
                var p = Path()
                p.move(to: pt(4.6, 15.4))
                p.addLine(to: pt(15.2, 4.8))
                p.move(to: pt(9.6, 4.8))
                p.addLine(to: pt(15.2, 4.8))
                p.addLine(to: pt(15.2, 10.4))
                context.stroke(p, with: .color(color), style: stroke)

            case .play:
                var p = Path()
                p.move(to: pt(7.0, 5.2))
                p.addLine(to: pt(7.0, 14.8))
                p.addLine(to: pt(14.8, 10.0))
                p.closeSubpath()
                context.stroke(p, with: .color(color), style: stroke)

            case .mindPlus, .mindMinus:
                let lobe = RoundedRectangle(cornerRadius: 3.2 * scale).path(in: rect(3.4, 5.0, 6.8, 10.0))
                context.stroke(lobe, with: .color(color), style: thinStroke)
                var p = Path()
                p.move(to: pt(12.2, 10.0))
                p.addLine(to: pt(16.8, 10.0))
                if keycap == .mindPlus {
                    p.move(to: pt(14.5, 7.7))
                    p.addLine(to: pt(14.5, 12.3))
                }
                context.stroke(p, with: .color(color), style: stroke)

            case .folder:
                var p = Path()
                p.move(to: pt(3.6, 6.2))
                p.addLine(to: pt(8.0, 6.2))
                p.addLine(to: pt(9.6, 7.8))
                p.addLine(to: pt(16.4, 7.8))
                p.addLine(to: pt(16.4, 15.0))
                p.addLine(to: pt(3.6, 15.0))
                p.closeSubpath()
                context.stroke(p, with: .color(color), style: stroke)

            case .upload:
                var p = Path()
                p.move(to: pt(10, 13.6))
                p.addLine(to: pt(10, 5.2))
                p.move(to: pt(7.0, 8.0))
                p.addLine(to: pt(10, 5.0))
                p.addLine(to: pt(13.0, 8.0))
                p.move(to: pt(4.8, 15.8))
                p.addLine(to: pt(15.2, 15.8))
                context.stroke(p, with: .color(color), style: stroke)

            case .apps:
                for (x, y) in [(4.2, 4.2), (11.0, 4.2), (4.2, 11.0), (11.0, 11.0)] {
                    let r = RoundedRectangle(cornerRadius: 1.4 * scale).path(in: rect(x, y, 4.8, 4.8))
                    context.stroke(r, with: .color(color), style: thinStroke)
                }

            case .setup:
                let c = Circle().path(in: rect(6.6, 6.6, 6.8, 6.8))
                context.stroke(c, with: .color(color), style: stroke)
                var p = Path()
                p.move(to: pt(10, 3.6)); p.addLine(to: pt(10, 5.4))
                p.move(to: pt(10, 14.6)); p.addLine(to: pt(10, 16.4))
                p.move(to: pt(3.6, 10)); p.addLine(to: pt(5.4, 10))
                p.move(to: pt(14.6, 10)); p.addLine(to: pt(16.4, 10))
                context.stroke(p, with: .color(color), style: stroke)
            }
        }
        .frame(width: size, height: size)
    }
}
