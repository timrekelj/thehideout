import HideoutCore
import ImageIO
import PhotosUI
import SwiftUI
import UniformTypeIdentifiers

/// `name`, stripped to characters safe for a local file name (shared with the download side,
/// `safeAttachmentFileName`).
private func safePickedFileName(_ name: String) -> String {
    let allowed = name.filter { $0.isLetter || $0.isNumber || ".-_ ".contains($0) }
    let meaningful = allowed.trimmingCharacters(in: CharacterSet(charactersIn: "."))
    return meaningful.isEmpty ? "file" : allowed
}

/// A fresh file in the app's own tmp directory (`Attachments/`, distinct from the download cache), for
/// one picked file's bytes — uploads always stream from a file on disk, never from memory, and a copy
/// here also survives the original picker item's security scope closing.
private func freshAttachmentTempFile(named name: String) -> URL {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent("thehideout-outbound", isDirectory: true)
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    return dir.appendingPathComponent("\(UUID().uuidString)-\(safePickedFileName(name))")
}

/// MIME types `ImageIO` can rewrite in place to strip GPS EXIF without re-encoding.
private func supportsExifRewrite(_ mime: String) -> Bool { mime == "image/jpeg" || mime == "image/png" || mime == "image/heic" || mime == "image/heif" }

/// Every EXIF/TIFF GPS tag, cleared so a picked photo cannot leak where it was taken (the system picker
/// does not strip this itself). EXIF *orientation* is left alone: the server honours it when it makes a
/// thumbnail.
private func stripGPS(at url: URL) {
    guard let data = try? Data(contentsOf: url),
          let source = CGImageSourceCreateWithData(data as CFData, nil),
          let type = CGImageSourceGetType(source) else { return }
    let hasGPS = (0..<CGImageSourceGetCount(source)).contains { i in
        ((CGImageSourceCopyPropertiesAtIndex(source, i, nil) as? [CFString: Any])?[kCGImagePropertyGPSDictionary]) != nil
    }
    guard hasGPS, let destination = CGImageDestinationCreateWithURL(url as CFURL, type, CGImageSourceGetCount(source), nil) else { return }
    for i in 0..<CGImageSourceGetCount(source) {
        var props = (CGImageSourceCopyPropertiesAtIndex(source, i, nil) as? [CFString: Any]) ?? [:]
        props[kCGImagePropertyGPSDictionary] = nil
        CGImageDestinationAddImageFromSource(destination, source, i, props as CFDictionary)
    }
    _ = CGImageDestinationFinalize(destination)
}

/// One file the `PhotosPicker`, document picker or camera handed the composer, resolved to a
/// `PickedFile` the store can upload: copied into the app's own tmp directory first (streamed, not
/// read whole into memory for a document/camera file; `PhotosPickerItem.loadTransferable` itself reads
/// the picker's own bytes into `Data`, the one point `PhotosUI` gives no streaming alternative for), a
/// JPEG/PNG/HEIC's GPS EXIF stripped before anything is uploaded.
func pickedFile(name: String, contentType: String, data: Data) -> PickedFile {
    let url = freshAttachmentTempFile(named: name)
    try? data.write(to: url)
    if supportsExifRewrite(contentType) { stripGPS(at: url) }
    let size = (try? FileManager.default.attributesOfItem(atPath: url.path)[.size] as? UInt64) ?? UInt64(data.count)
    let preview = contentType.hasPrefix("image/") ? url.absoluteString : nil
    return PickedFile(name: name, contentType: contentType, size: size, preview: preview, fileURL: url)
}

/// Streams `source` (a document-picker or camera URL) into a fresh tmp file rather than reading it
/// whole into memory, stripping GPS EXIF after if it's a rewritable image format.
func pickedFile(copying source: URL, suggestedName: String? = nil) throws -> PickedFile {
    let name = suggestedName ?? source.lastPathComponent
    let contentType = UTType(filenameExtension: (name as NSString).pathExtension)?.preferredMIMEType ?? "application/octet-stream"
    let url = freshAttachmentTempFile(named: name)
    let accessing = source.startAccessingSecurityScopedResource()
    defer { if accessing { source.stopAccessingSecurityScopedResource() } }
    try FileManager.default.copyItem(at: source, to: url)
    if supportsExifRewrite(contentType) { stripGPS(at: url) }
    let size = (try? FileManager.default.attributesOfItem(atPath: url.path)[.size] as? UInt64) ?? 0
    let preview = contentType.hasPrefix("image/") ? url.absoluteString : nil
    return PickedFile(name: name, contentType: contentType, size: size, preview: preview, fileURL: url)
}

/// Resolves every `PhotosPickerItem` in order to a `PickedFile` (a display name guessed from its
/// content type, since the Photos picker does not hand one over).
func resolvePhotosPickerItems(_ items: [PhotosPickerItem]) async -> [PickedFile] {
    var out: [PickedFile] = []
    for item in items {
        guard let data = try? await item.loadTransferable(type: Data.self) else { continue }
        let type = item.supportedContentTypes.first
        let mime = type?.preferredMIMEType ?? "application/octet-stream"
        let ext = type?.preferredFilenameExtension ?? "dat"
        out.append(pickedFile(name: "photo-\(UUID().uuidString.prefix(8)).\(ext)", contentType: mime, data: data))
    }
    return out
}

/// The attach button's menu: Photo Library (multi-select images and videos), Camera (photo or video),
/// and Files (ATT-09, COMP-05). Each path hands `onPicked` resolved `PickedFile`s.
struct AttachMenuButton: View {
    var onPicked: ([PickedFile]) -> Void
    @State private var showPhotos = false
    @State private var photosSelection: [PhotosPickerItem] = []
    @State private var showFiles = false
    @State private var cameraMode: CameraCaptureView.Mode?

    var body: some View {
        Menu {
            Button("Photo Library", systemImage: "photo.on.rectangle") { showPhotos = true }
            Button("Take Photo", systemImage: "camera") { cameraMode = .photo }
            Button("Take Video", systemImage: "video") { cameraMode = .video }
            Button("Choose File", systemImage: "folder") { showFiles = true }
        } label: {
            Image(systemName: "plus")
        }
        .photosPicker(isPresented: $showPhotos, selection: $photosSelection, matching: .any(of: [.images, .videos]))
        .onChange(of: photosSelection) { _, items in
            guard !items.isEmpty else { return }
            let picked = items
            photosSelection = []
            Task {
                let files = await resolvePhotosPickerItems(picked)
                if !files.isEmpty { onPicked(files) }
            }
        }
        .fileImporter(isPresented: $showFiles, allowedContentTypes: [.item], allowsMultipleSelection: true) { result in
            guard case .success(let urls) = result else { return }
            let files = urls.compactMap { try? pickedFile(copying: $0) }
            if !files.isEmpty { onPicked(files) }
        }
        .fullScreenCover(item: $cameraMode) { mode in
            CameraCaptureView(mode: mode) { url in
                if let url, let file = try? pickedFile(copying: url) { onPicked([file]) }
            }
        }
        .accessibilityLabel("Attach")
    }
}

/// Wraps `UIImagePickerController`'s camera source (no SwiftUI-native capture view exists yet):
/// `onCaptured` gets the captured photo/video's own temporary file, or nil if the person cancelled.
struct CameraCaptureView: UIViewControllerRepresentable {
    enum Mode: String, Identifiable { case photo, video
        var id: String { rawValue }
    }
    let mode: Mode
    let onCaptured: (URL?) -> Void
    @Environment(\.dismiss) private var dismiss

    func makeUIViewController(context: Context) -> UIImagePickerController {
        let picker = UIImagePickerController()
        picker.sourceType = .camera
        picker.mediaTypes = mode == .photo ? [UTType.image.identifier] : [UTType.movie.identifier]
        if mode == .video { picker.videoQuality = .typeMedium }
        picker.delegate = context.coordinator
        return picker
    }

    func updateUIViewController(_ uiViewController: UIImagePickerController, context: Context) {}
    func makeCoordinator() -> Coordinator { Coordinator(self) }

    final class Coordinator: NSObject, UIImagePickerControllerDelegate, UINavigationControllerDelegate {
        let parent: CameraCaptureView
        init(_ parent: CameraCaptureView) { self.parent = parent }

        func imagePickerController(_ picker: UIImagePickerController, didFinishPickingMediaWithInfo info: [UIImagePickerController.InfoKey: Any]) {
            defer { parent.dismiss() }
            if let videoURL = info[.mediaURL] as? URL {
                parent.onCaptured(videoURL)
            } else if let image = info[.originalImage] as? UIImage, let data = image.jpegData(compressionQuality: 0.9) {
                // A freshly rendered JPEG from `UIImage` carries no EXIF GPS of its own (there was never
                // a file with tags to strip), unlike a picked existing photo.
                let url = freshAttachmentTempFile(named: "photo.jpg")
                try? data.write(to: url)
                parent.onCaptured(url)
            } else {
                parent.onCaptured(nil)
            }
        }

        func imagePickerControllerDidCancel(_ picker: UIImagePickerController) {
            parent.dismiss()
            parent.onCaptured(nil)
        }
    }
}
