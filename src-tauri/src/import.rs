//! 电脑里已有的图片：拖进来、粘贴、选文件。按文件名排序，认得出的转成 PNG 存进 shots/。

use crate::draft::MAX_SHOTS;
use image::imageops::{self, FilterType};
use image::RgbaImage;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

/// 导入的图长边最多这么大，再大就缩（存进去太占地方，AI 那边也会再缩）
pub const IMPORT_MAX_SIDE: u32 = 8000;
/// 文件超过这么大就不读（拖进来的视频、压缩包不往内存里读）
pub const MAX_IMPORT_BYTES: u64 = 200 * 1024 * 1024;
/// 解码时最多占这么多内存（image 自带的是 512 MiB，一万多像素的大图会被当成读不出来）
pub const IMPORT_MAX_ALLOC: u64 = 1536 * 1024 * 1024;

const IMAGE_EXTS: [&str; 5] = ["png", "jpg", "jpeg", "bmp", "webp"];

pub fn is_image_path(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| IMAGE_EXTS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// 自然顺序：数字按数值比（"2" 在 "10" 前面），字母不分大小写；同一位置数字排在字母前面
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().copied().filter(|c| c.is_ascii_digit()) {
                        s.push(c);
                        it.next();
                    }
                    s
                };
                let (na, nb) = (take(&mut a), take(&mut b));
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb)).then_with(|| na.len().cmp(&nb.len()));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = match (x.is_ascii_digit(), y.is_ascii_digit()) {
                    (true, false) => Ordering::Less,
                    (false, true) => Ordering::Greater,
                    _ => x.to_lowercase().cmp(y.to_lowercase()),
                };
                if ord != Ordering::Equal {
                    return ord;
                }
                a.next();
                b.next();
            }
        }
    }
}

/// 按文件名的自然顺序排（存下来的截图文件名一般带时间，这样排就是先后顺序）
pub fn sort_paths(paths: &mut [PathBuf]) {
    let name = |p: &PathBuf| p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    paths.sort_by(|a, b| natural_cmp(&name(a), &name(b)));
}

/// 值得读进来：扩展名是图片，大小也正常
pub fn worth_reading(path: &Path, len: u64) -> bool {
    is_image_path(path) && len <= MAX_IMPORT_BYTES
}

/// 认不出、0 字节都是 None；按 EXIF 方向转正（手机竖拍的照片）；长边超过上限就缩
pub fn decode(bytes: &[u8]) -> Option<RgbaImage> {
    decode_with_limit(bytes, IMPORT_MAX_ALLOC)
}

pub fn decode_with_limit(bytes: &[u8], max_alloc: u64) -> Option<RgbaImage> {
    use image::ImageDecoder;
    let reader = || image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok();
    // 方向只读文件头；解码走 decode()，它才会按内存上限检查
    let orientation = reader()?
        .into_decoder()
        .ok()
        .and_then(|mut d| d.orientation().ok())
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut r = reader()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(max_alloc);
    r.limits(limits);
    let mut img = r.decode().ok()?;
    img.apply_orientation(orientation);
    Some(fit_within(img.to_rgba8(), IMPORT_MAX_SIDE))
}

pub fn fit_within(img: RgbaImage, max: u32) -> RgbaImage {
    let (w, h) = img.dimensions();
    let long = w.max(h);
    if long <= max {
        return img;
    }
    let ratio = max as f64 / long as f64;
    let nw = ((w as f64 * ratio).round() as u32).max(1);
    let nh = ((h as f64 * ratio).round() as u32).max(1);
    imageops::resize(&img, nw, nh, FilterType::Triangle)
}

/// 加图后给用户的提示：多出来的、读不出来的
pub fn outcome_message(over_limit: usize, unreadable: usize) -> Option<String> {
    let mut parts = Vec::new();
    if over_limit > 0 {
        parts.push(format!("一条最多 {MAX_SHOTS} 张，多出来的没加"));
    }
    if unreadable > 0 {
        parts.push(format!("有 {unreadable} 个文件不是图片或读不出来，没加"));
    }
    (!parts.is_empty()).then(|| parts.join("；"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use std::path::PathBuf;

    #[test]
    fn natural_sort_by_file_name() {
        let mut paths: Vec<PathBuf> =
            ["10.png", "2.png", "1.png", "b.png", "A10.png", "a2.png"].iter().map(|n| PathBuf::from(format!("C:/x/{n}"))).collect();
        sort_paths(&mut paths);
        let names: Vec<String> = paths.iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect();
        assert_eq!(names, ["1.png", "2.png", "10.png", "a2.png", "A10.png", "b.png"]);
    }

    #[test]
    fn is_image_path_by_extension() {
        for ok in ["a.PNG", "b.jpeg", "c.jpg", "d.bmp", "e.webp"] {
            assert!(is_image_path(&PathBuf::from(ok)), "{ok}");
        }
        for bad in ["d.txt", "e", "f.png.lnk"] {
            assert!(!is_image_path(&PathBuf::from(bad)), "{bad}");
        }
    }

    fn sample() -> RgbaImage {
        RgbaImage::from_fn(3, 2, |x, y| Rgba([x as u8 * 80, y as u8 * 120, 50, 255]))
    }

    #[test]
    fn decode_png_and_webp() {
        let png = crate::capture::png_bytes(&sample());
        assert_eq!(decode(&png).map(|i| i.dimensions()), Some((3, 2)));
        let mut webp = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut webp)
            .encode(sample().as_raw(), 3, 2, image::ExtendedColorType::Rgba8)
            .unwrap();
        assert_eq!(decode(&webp).map(|i| i.dimensions()), Some((3, 2)));
    }

    #[test]
    fn decode_garbage_is_none() {
        assert!(decode(b"").is_none());
        assert!(decode(b"not an image").is_none());
    }

    #[test]
    fn fit_within_caps_long_side() {
        assert_eq!(fit_within(RgbaImage::new(9000, 100), 8000).dimensions(), (8000, 89));
        assert_eq!(fit_within(RgbaImage::new(100, 50), 8000).dimensions(), (100, 50));
    }

    #[test]
    fn outcome_messages() {
        assert_eq!(outcome_message(0, 0), None);
        assert_eq!(outcome_message(2, 0).as_deref(), Some("一条最多 9 张，多出来的没加"));
        assert_eq!(outcome_message(0, 1).as_deref(), Some("有 1 个文件不是图片或读不出来，没加"));
        assert_eq!(
            outcome_message(1, 3).as_deref(),
            Some("一条最多 9 张，多出来的没加；有 3 个文件不是图片或读不出来，没加")
        );
    }

    #[test]
    fn only_image_files_of_sane_size_are_read() {
        assert!(worth_reading(&PathBuf::from("a.png"), 1024));
        assert!(!worth_reading(&PathBuf::from("movie.mp4"), 1024));
        assert!(!worth_reading(&PathBuf::from("huge.png"), MAX_IMPORT_BYTES + 1));
    }

    #[test]
    fn decode_respects_alloc_limit() {
        let png = crate::capture::png_bytes(&RgbaImage::new(100, 100));
        assert!(decode_with_limit(&png, 1000).is_none());
        assert!(decode_with_limit(&png, IMPORT_MAX_ALLOC).is_some());
        // 默认上限要比 image 自带的 512 MiB 大：12000×12000 的图也认得出
        assert!(IMPORT_MAX_ALLOC > 12000 * 12000 * 4);
    }

    /// 3×2 的 JPEG，带 EXIF 方向 6（要顺时针转 90 度）
    fn jpeg_with_orientation_6() -> Vec<u8> {
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 90)
            .encode(&[200u8; 3 * 2 * 3], 3, 2, image::ExtendedColorType::Rgb8)
            .unwrap();
        let mut tiff = b"Exif\0\0II\x2a\x00\x08\x00\x00\x00".to_vec();
        tiff.extend_from_slice(&[1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0]);
        let mut out = jpeg[..2].to_vec();
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&((tiff.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(&tiff);
        out.extend_from_slice(&jpeg[2..]);
        out
    }

    #[test]
    fn decode_follows_exif_orientation() {
        assert_eq!(decode(&jpeg_with_orientation_6()).map(|i| i.dimensions()), Some((2, 3)));
    }
}
