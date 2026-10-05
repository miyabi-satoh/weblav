//! Quick Look (`QLThumbnailGenerator`) で縮小画像を作る。

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;

use block2::RcBlock;
use image::DynamicImage;
use objc2::AllocAnyThread;
use objc2_core_foundation::CGSize;
use objc2_foundation::{NSError, NSString, NSURL};
use objc2_quick_look_thumbnailing::{
    QLThumbnailGenerationRequest, QLThumbnailGenerationRequestRepresentationTypes,
    QLThumbnailGenerator,
};
use objc2_uniform_type_identifiers::{UTType, UTTypePNG};

use super::{Failure, TIMEOUT};

pub(super) fn thumbnail(path: &Path, extension: &str, size: u32) -> Result<DynamicImage, Failure> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    // CGImage を画素に直すより、PNG に書かせて `image` で読むほうが扱う型が少ない。
    let out = std::env::temp_dir().join(format!(
        "weblav-thumbnail-{}-{}.png",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let url = NSURL::fileURLWithPath(&NSString::from_str(
        path.to_str().ok_or(Failure::Unavailable)?,
    ));
    let out_url = NSURL::fileURLWithPath(&NSString::from_str(
        out.to_str().ok_or(Failure::Unavailable)?,
    ));
    let side = f64::from(size);
    let generator = unsafe { QLThumbnailGenerator::sharedGenerator() };
    let request = unsafe {
        QLThumbnailGenerationRequest::initWithFileAtURL_size_scale_representationTypes(
            QLThumbnailGenerationRequest::alloc(),
            &url,
            CGSize::new(side, side),
            1.0,
            QLThumbnailGenerationRequestRepresentationTypes::Thumbnail,
        )
    };
    // file コンテンツの実体は拡張子を持たないので、種類は元のファイル名から渡す。
    if let Some(content_type) = UTType::typeWithFilenameExtension(&NSString::from_str(extension)) {
        unsafe { request.setContentType(Some(&content_type)) };
    }

    let (sender, receiver) = mpsc::channel();
    let written = out.clone();
    let completion = RcBlock::new(move |error: *mut NSError| {
        // 待つのをやめた後に書き終えたときは、受け取る側がいないので、ここで消す。
        if sender.send(error.is_null()).is_err() {
            let _ = std::fs::remove_file(&written);
        }
    });
    unsafe {
        generator.saveBestRepresentationForRequest_toFileAtURL_asContentType_completionHandler(
            &request,
            &out_url,
            UTTypePNG,
            &completion,
        );
    }
    let saved = receiver.recv_timeout(TIMEOUT);
    if saved.is_err() {
        unsafe { generator.cancelRequest(&request) };
    }
    drop(receiver);
    let image = match saved {
        Ok(true) => image::open(&out).map_err(|_| Failure::Unavailable),
        Ok(false) => Err(Failure::Unavailable),
        Err(_) => Err(Failure::TimedOut),
    };
    let _ = std::fs::remove_file(&out);
    image
}
