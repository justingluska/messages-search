// Turns JSON lines into Messages-style `attributedBody` blobs: NSArchiver
// typedstreams of an NSMutableAttributedString carrying the same attribute
// keys Messages writes. Used by make-fixture.py; macOS only.
//
// stdin, one JSON object per line:
//   {"t": "text", "a": ["at_0_GUID", ...], "links": [[utf16Start, utf16Len, "https://..."]], "plain": false}
//   "a"     attachment transfer GUIDs; the text must already hold one U+FFFC per GUID, in order.
//   "plain" true = immutable NSAttributedString with only the part attribute
//           (the shape stored in edit history, `message_summary_info`).
// stdout: one base64 blob per input line, same order.
//
//   swiftc -O scripts/typedstream-helper.swift -o <scratch>/typedstream-helper

import Foundation

// Part indexes are archived as 32-bit ("i"), the writing direction as 64-bit
// ("q"), matching real chat.db blobs byte for byte.

let partKey = NSAttributedString.Key("__kIMMessagePartAttributeName")
let transferKey = NSAttributedString.Key("__kIMFileTransferGUIDAttributeName")
let directionKey = NSAttributedString.Key("__kIMBaseWritingDirectionAttributeName")
let linkKey = NSAttributedString.Key("__kIMLinkAttributeName")

struct Request: Decodable {
    let t: String
    let a: [String]?
    let links: [[LinkField]]?
    let plain: Bool?
}

enum LinkField: Decodable {
    case int(Int)
    case str(String)
    init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if let i = try? c.decode(Int.self) { self = .int(i) } else { self = .str(try c.decode(String.self)) }
    }
}

func build(_ r: Request) -> NSAttributedString {
    let text = r.t as NSString
    if r.plain == true {
        return NSAttributedString(string: r.t, attributes: [partKey: NSNumber(value: Int32(0))])
    }
    let s = NSMutableAttributedString(string: r.t)
    var part = 0
    var cursor = 0
    // Each attachment is its own part (bubble), like Messages does.
    for guid in r.a ?? [] {
        let range = text.range(of: "\u{FFFC}", range: NSRange(location: cursor, length: text.length - cursor))
        guard range.location != NSNotFound else { fatalError("missing U+FFFC for \(guid)") }
        // One key at a time: a Swift dictionary literal iterates in a per-process
        // random order, which would make the archived bytes nondeterministic.
        s.addAttribute(partKey, value: NSNumber(value: Int32(part)), range: range)
        s.addAttribute(transferKey, value: guid as NSString, range: range)
        s.addAttribute(directionKey, value: NSNumber(value: -1), range: range)
        part += 1
        cursor = range.location + range.length
    }
    if cursor < text.length {
        s.addAttribute(partKey, value: NSNumber(value: Int32(part)), range: NSRange(location: cursor, length: text.length - cursor))
    }
    for link in r.links ?? [] {
        guard link.count == 3, case let .int(start) = link[0], case let .int(len) = link[1],
              case let .str(url) = link[2], let u = URL(string: url) else { fatalError("bad link \(link)") }
        s.addAttribute(linkKey, value: u as NSURL, range: NSRange(location: start, length: len))
    }
    return s
}

let decoder = JSONDecoder()
var out = FileHandle.standardOutput
var buffer = Data()
while let line = readLine(strippingNewline: true) {
    if line.isEmpty { continue }
    let req = try decoder.decode(Request.self, from: Data(line.utf8))
    let data = NSArchiver.archivedData(withRootObject: build(req))
    buffer.append(data.base64EncodedData())
    buffer.append(0x0A)
    if buffer.count > 1 << 20 {
        out.write(buffer)
        buffer.removeAll(keepingCapacity: true)
    }
}
out.write(buffer)
