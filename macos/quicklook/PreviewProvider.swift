// The Quick Look preview (space bar) of .scrawl documents.

import ImageIO
import QuickLookUI
import UniformTypeIdentifiers

@objc(PreviewProvider)
final class PreviewProvider: QLPreviewProvider, QLPreviewingController {
    func providePreview(for request: QLFilePreviewRequest) async throws -> QLPreviewReply {
        let png = try ScrawlPreview.png(at: request.fileURL)
        var size = CGSize(width: 800, height: 600)
        if let source = CGImageSourceCreateWithData(png as CFData, nil),
           let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
           let width = props[kCGImagePropertyPixelWidth] as? CGFloat,
           let height = props[kCGImagePropertyPixelHeight] as? CGFloat {
            size = CGSize(width: width, height: height)
        }
        return QLPreviewReply(dataOfContentType: .png, contentSize: size) { _ in png }
    }
}
