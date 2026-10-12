// Reads the preview picture Scrawl saves in each document. A .scrawl file is a zip archive and
// `preview.png` is stored uncompressed, so it can be sliced straight out of the file.

import Foundation

enum ScrawlPreview {
    struct Missing: Error {}

    /// The PNG bytes of the document's preview.
    static func png(at url: URL) throws -> Data {
        let zip = try Data(contentsOf: url, options: .mappedIfSafe)
        guard let png = find("preview.png", in: zip) else { throw Missing() }
        return png
    }

    private static func find(_ name: String, in zip: Data) -> Data? {
        let bytes = [UInt8](zip)
        func u16(_ at: Int) -> Int { Int(bytes[at]) | Int(bytes[at + 1]) << 8 }
        func u32(_ at: Int) -> Int { u16(at) | u16(at + 2) << 16 }
        // The end of central directory record, searched for backwards past any archive comment.
        guard bytes.count >= 22 else { return nil }
        var end = bytes.count - 22
        while end >= max(0, bytes.count - 22 - 0xFFFF), u32(end) != 0x0605_4B50 { end -= 1 }
        guard end >= 0, u32(end) == 0x0605_4B50 else { return nil }
        var entry = u32(end + 16)
        for _ in 0..<u16(end + 10) {
            guard entry + 46 <= bytes.count, u32(entry) == 0x0201_4B50 else { return nil }
            let method = u16(entry + 10), size = u32(entry + 20)
            let nameLength = u16(entry + 28), extra = u16(entry + 30), comment = u16(entry + 32)
            let local = u32(entry + 42)
            guard entry + 46 + nameLength <= bytes.count else { return nil }
            let entryName = String(decoding: bytes[(entry + 46)..<(entry + 46 + nameLength)], as: UTF8.self)
            if entryName == name {
                guard method == 0, local + 30 <= bytes.count, u32(local) == 0x0403_4B50 else { return nil }
                let start = local + 30 + u16(local + 26) + u16(local + 28)
                guard start + size <= bytes.count else { return nil }
                return zip.subdata(in: start..<(start + size))
            }
            entry += 46 + nameLength + extra + comment
        }
        return nil
    }
}
