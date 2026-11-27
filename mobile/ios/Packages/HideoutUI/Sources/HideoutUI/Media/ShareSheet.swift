import Photos
import SwiftUI
import UIKit

/// The system share sheet (ATT-04/05/12): AirDrop, Messages, Save to Files, and anything else installed.
struct ShareSheet: UIViewControllerRepresentable {
    let items: [Any]

    func makeUIViewController(context: Context) -> UIActivityViewController { UIActivityViewController(activityItems: items, applicationActivities: nil) }
    func updateUIViewController(_ uiViewController: UIActivityViewController, context: Context) {}
}

/// Saving a picture or video to Photos failed, or the person declined the permission prompt.
public struct PhotoSaveError: Error, Equatable {
    public let message: String
    public init(_ message: String) { self.message = message }
}

/// Saves `fileURL` to the photo library (ATT-05): requests add-only access (`NSPhotoLibraryAddUsageDescription`)
/// first, so a person who only ever saves attachments is never asked for read access to their whole library.
@MainActor
public func saveToPhotoLibrary(fileURL: URL, isVideo: Bool) async throws {
    let status = await PHPhotoLibrary.requestAuthorization(for: .addOnly)
    guard status == .authorized || status == .limited else {
        throw PhotoSaveError("Allow photo access in Settings to save this.")
    }
    do {
        try await PHPhotoLibrary.shared().performChanges {
            if isVideo { PHAssetChangeRequest.creationRequestForAssetFromVideo(atFileURL: fileURL) }
            else { PHAssetChangeRequest.creationRequestForAssetFromImage(atFileURL: fileURL) }
        }
    } catch {
        throw PhotoSaveError("Couldn't save that.")
    }
}
