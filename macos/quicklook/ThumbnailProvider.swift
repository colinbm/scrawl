// Finder icons and thumbnails for .scrawl documents.

import CoreGraphics
import ImageIO
import QuickLookThumbnailing

@objc(ThumbnailProvider)
final class ThumbnailProvider: QLThumbnailProvider {
    override func provideThumbnail(for request: QLFileThumbnailRequest, _ handler: @escaping (QLThumbnailReply?, Error?) -> Void) {
        do {
            let png = try ScrawlPreview.png(at: request.fileURL)
            guard let source = CGImageSourceCreateWithData(png as CFData, nil),
                  let image = CGImageSourceCreateImageAtIndex(source, 0, nil),
                  image.width > 0, image.height > 0
            else { throw ScrawlPreview.Missing() }
            let max = request.maximumSize
            let scale = min(max.width / CGFloat(image.width), max.height / CGFloat(image.height))
            let size = CGSize(width: CGFloat(image.width) * scale, height: CGFloat(image.height) * scale)
            handler(QLThumbnailReply(contextSize: size) { context in
                context.interpolationQuality = .high
                context.draw(image, in: CGRect(origin: .zero, size: size))
                return true
            }, nil)
        } catch {
            handler(nil, error)
        }
    }
}
