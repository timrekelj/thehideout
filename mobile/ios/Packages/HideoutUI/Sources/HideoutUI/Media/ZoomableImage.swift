import SwiftUI
import UIKit

/// Pinch and double-tap zoom with pan on a picture, via a native `UIScrollView`: at zoom scale 1 its
/// content size equals its bounds, so it has nothing to scroll and a horizontal swipe passes straight
/// through to the pager above it; once zoomed in, the scroll view claims the pan itself. `scale` is
/// reported back so the lightbox knows when a vertical swipe-to-dismiss is safe (only at scale 1 — ATT-05).
struct ZoomableImage: UIViewRepresentable {
    let image: UIImage?
    @Binding var scale: CGFloat

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeUIView(context: Context) -> ZoomingScrollView {
        let scrollView = ZoomingScrollView()
        scrollView.delegate = context.coordinator
        scrollView.minimumZoomScale = 1
        scrollView.maximumZoomScale = 5
        scrollView.showsVerticalScrollIndicator = false
        scrollView.showsHorizontalScrollIndicator = false
        scrollView.bouncesZoom = true
        scrollView.contentInsetAdjustmentBehavior = .never

        let imageView = UIImageView(image: image)
        imageView.contentMode = .scaleAspectFit
        imageView.isUserInteractionEnabled = true
        scrollView.addSubview(imageView)
        context.coordinator.imageView = imageView

        let doubleTap = UITapGestureRecognizer(target: context.coordinator, action: #selector(Coordinator.doubleTapped))
        doubleTap.numberOfTapsRequired = 2
        scrollView.addGestureRecognizer(doubleTap)
        return scrollView
    }

    func updateUIView(_ scrollView: ZoomingScrollView, context: Context) {
        // Only the image changes here; layout (sizing/centering the image view) is `ZoomingScrollView`'s
        // own `layoutSubviews`, which skips doing that while zoomed so it never fights an active pan.
        if context.coordinator.imageView?.image !== image {
            context.coordinator.imageView?.image = image
        }
    }

    final class Coordinator: NSObject, UIScrollViewDelegate {
        var parent: ZoomableImage
        weak var imageView: UIImageView?

        init(_ parent: ZoomableImage) { self.parent = parent }

        func viewForZooming(in scrollView: UIScrollView) -> UIView? { imageView }

        func scrollViewDidZoom(_ scrollView: UIScrollView) {
            parent.scale = scrollView.zoomScale
            center(scrollView)
        }

        private func center(_ scrollView: UIScrollView) {
            guard let imageView else { return }
            let boundsSize = scrollView.bounds.size
            var frame = imageView.frame
            frame.origin.x = frame.size.width < boundsSize.width ? (boundsSize.width - frame.size.width) / 2 : 0
            frame.origin.y = frame.size.height < boundsSize.height ? (boundsSize.height - frame.size.height) / 2 : 0
            imageView.frame = frame
        }

        @objc func doubleTapped(_ gesture: UITapGestureRecognizer) {
            guard let scrollView = gesture.view as? UIScrollView else { return }
            if scrollView.zoomScale > scrollView.minimumZoomScale {
                scrollView.setZoomScale(scrollView.minimumZoomScale, animated: true)
            } else {
                let point = gesture.location(in: imageView)
                let target = CGRect(x: point.x - 60, y: point.y - 60, width: 120, height: 120)
                scrollView.zoom(to: target, animated: true)
            }
        }
    }
}

/// Sizes and positions its image view itself, in `layoutSubviews` (so rotation/size changes relay out
/// automatically), but only at zoom scale 1: resetting `imageView.frame`/`contentSize` while zoomed in
/// (mid-pan, no gesture in progress) is undefined and would fight the pinch/pan the person is doing.
final class ZoomingScrollView: UIScrollView {
    override func layoutSubviews() {
        super.layoutSubviews()
        guard zoomScale == 1, bounds.width > 0, bounds.height > 0, let imageView = subviews.first as? UIImageView else { return }
        imageView.frame = bounds
        contentSize = bounds.size
    }
}
