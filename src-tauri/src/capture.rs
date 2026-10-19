//! 截屏：按下热键时抓下每块屏（物理像素）定格，框选后从原图裁剪。

use chrono::{DateTime, Local, NaiveDateTime};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::imageops::{self, FilterType};
use image::{DynamicImage, ExtendedColorType, ImageEncoder, RgbaImage};
use std::sync::atomic::{AtomicU32, Ordering};

pub const AI_MAX_SIDE: u32 = 1600;
pub const AI_JPEG_QUALITY: u8 = 85;

pub struct FrozenMonitor {
    pub index: usize,
    /// 物理像素坐标（虚拟桌面），副屏在左侧时为负
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub image: RgbaImage,
}

pub struct Frozen {
    /// 按下热键的时刻，条目归属哪一天按它算
    pub taken_at: DateTime<Local>,
    pub monitors: Vec<FrozenMonitor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// 抓下所有显示器的整屏画面。每块屏一个线程同时截（显示器句柄不能跨线程，各线程自己枚举）
pub fn grab_all() -> Result<Frozen, String> {
    let taken_at = Local::now();
    let count = xcap::Monitor::all().map_err(|e| format!("枚举显示器失败：{e}"))?.len();
    if count == 0 {
        return Err("没有找到显示器".into());
    }
    let results: Vec<Result<FrozenMonitor, String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..count)
            .map(|index| {
                scope.spawn(move || {
                    let monitors = xcap::Monitor::all().map_err(|e| format!("枚举显示器失败：{e}"))?;
                    let m = monitors.get(index).ok_or("显示器数量变了")?;
                    let image = m.capture_image().map_err(|e| format!("截屏失败：{e}"))?;
                    Ok(FrozenMonitor {
                        index,
                        x: m.x().unwrap_or(0),
                        y: m.y().unwrap_or(0),
                        width: image.width(),
                        height: image.height(),
                        scale: m.scale_factor().unwrap_or(1.0) as f64,
                        image,
                    })
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap_or_else(|_| Err("截屏线程崩溃".into()))).collect()
    });
    Ok(Frozen { taken_at, monitors: collect_monitors(results)? })
}

/// 只要有一块屏截到就继续；全都失败才报错
pub fn collect_monitors(results: Vec<Result<FrozenMonitor, String>>) -> Result<Vec<FrozenMonitor>, String> {
    let mut ok = Vec::new();
    let mut first_err = None;
    for r in results {
        match r {
            Ok(m) => ok.push(m),
            Err(e) => {
                log::warn!("{e}");
                first_err.get_or_insert(e);
            }
        }
    }
    if ok.is_empty() {
        return Err(first_err.unwrap_or_else(|| "没有找到显示器".into()));
    }
    Ok(ok)
}

/// 选区坐标可能是负的（鼠标拖出了屏幕左边或上边），裁到 0；裁完没东西了返回 None
pub fn clamp_rect(x: i64, y: i64, w: i64, h: i64) -> Option<PhysRect> {
    let (x1, y1) = (x.max(0), y.max(0));
    let (x2, y2) = (x + w, y + h);
    if x2 <= x1 || y2 <= y1 {
        return None;
    }
    Some(PhysRect { x: x1 as u32, y: y1 as u32, w: (x2 - x1) as u32, h: (y2 - y1) as u32 })
}

/// 超出屏幕的部分裁掉；完全在外面返回 None
pub fn crop(frozen: &Frozen, index: usize, rect: PhysRect) -> Option<RgbaImage> {
    let m = frozen.monitors.iter().find(|m| m.index == index)?;
    if rect.x >= m.width || rect.y >= m.height || rect.w == 0 || rect.h == 0 {
        return None;
    }
    let w = rect.w.min(m.width - rect.x);
    let h = rect.h.min(m.height - rect.y);
    Some(imageops::crop_imm(&m.image, rect.x, rect.y, w, h).to_image())
}

/// 32 位、从上到下的 BMP，给框选窗口当背景用（不压缩，最快）
pub fn to_bmp(img: &RgbaImage) -> Vec<u8> {
    let (w, h) = img.dimensions();
    let pixels = (w as usize) * (h as usize) * 4;
    let mut out = Vec::with_capacity(54 + pixels);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((54 + pixels) as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(-(h as i32)).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&(pixels as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 16]);
    for p in img.as_raw().chunks_exact(4) {
        out.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
    }
    out
}

/// 存档用的 PNG 原图
pub fn png_bytes(img: &RgbaImage) -> Vec<u8> {
    let mut buf = Vec::new();
    PngEncoder::new_with_quality(&mut buf, CompressionType::Fast, PngFilter::Adaptive)
        .write_image(img.as_raw(), img.width(), img.height(), ExtendedColorType::Rgba8)
        .expect("PNG 编码不会失败");
    buf
}

/// 发给 AI 的图：长边不超过 1600，JPEG 质量 85
pub fn ai_jpeg(img: &RgbaImage) -> Vec<u8> {
    let (w, h) = img.dimensions();
    let long = w.max(h);
    let rgb = if long > AI_MAX_SIDE {
        let ratio = AI_MAX_SIDE as f64 / long as f64;
        let nw = ((w as f64 * ratio).round() as u32).max(1);
        let nh = ((h as f64 * ratio).round() as u32).max(1);
        DynamicImage::ImageRgba8(imageops::resize(img, nw, nh, FilterType::Triangle)).to_rgb8()
    } else {
        DynamicImage::ImageRgba8(img.clone()).to_rgb8()
    };
    let mut buf = Vec::new();
    JpegEncoder::new_with_quality(&mut buf, AI_JPEG_QUALITY).encode_image(&rgb).expect("JPEG 编码不会失败");
    buf
}

/// 等比缩小到不超过 max_w × max_h，本来就小的不放大
pub fn thumbnail_fit(img: &RgbaImage, max_w: u32, max_h: u32) -> RgbaImage {
    let (w, h) = img.dimensions();
    let ratio = (max_w as f64 / w as f64).min(max_h as f64 / h as f64);
    if ratio >= 1.0 {
        return img.clone();
    }
    let nw = ((w as f64 * ratio).round() as u32).max(1);
    let nh = ((h as f64 * ratio).round() as u32).max(1);
    imageops::thumbnail(img, nw, nh)
}

pub fn thumb_jpeg(img: &RgbaImage) -> Vec<u8> {
    let mut buf = Vec::new();
    let rgb = DynamicImage::ImageRgba8(img.clone()).to_rgb8();
    JpegEncoder::new_with_quality(&mut buf, 88).encode_image(&rgb).expect("JPEG 编码不会失败");
    buf
}

static ID_COUNTER: AtomicU32 = AtomicU32::new(0);

/// `YYYYMMDD-HHMMSS-xxxx`，同一秒内也不重复
pub fn shot_id(t: NaiveDateTime) -> String {
    let n = ID_COUNTER.fetch_add(1, Ordering::Relaxed) as u16;
    format!("{}-{:04x}", t.format("%Y%m%d-%H%M%S"), seed_once().wrapping_add(n))
}

fn seed_once() -> u16 {
    static SEED: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
    *SEED.get_or_init(|| Local::now().timestamp_subsec_nanos() as u16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn synthetic(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| Rgba([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8, 255]))
    }

    fn frozen_with(img: RgbaImage) -> Frozen {
        let (w, h) = img.dimensions();
        Frozen {
            taken_at: chrono::Local::now(),
            monitors: vec![FrozenMonitor { index: 0, x: -2560, y: 0, width: w, height: h, scale: 1.5, image: img }],
        }
    }

    #[test]
    fn crop_exact_pixels() {
        let f = frozen_with(synthetic(100, 80));
        let out = crop(&f, 0, PhysRect { x: 10, y: 20, w: 30, h: 40 }).unwrap();
        assert_eq!(out.dimensions(), (30, 40));
        for (x, y, p) in out.enumerate_pixels() {
            assert_eq!(*p, *f.monitors[0].image.get_pixel(x + 10, y + 20));
        }
    }

    #[test]
    fn crop_clamps_to_bounds() {
        let f = frozen_with(synthetic(100, 80));
        let out = crop(&f, 0, PhysRect { x: 90, y: 70, w: 50, h: 50 }).unwrap();
        assert_eq!(out.dimensions(), (10, 10));
        assert!(crop(&f, 0, PhysRect { x: 200, y: 0, w: 5, h: 5 }).is_none());
        assert!(crop(&f, 3, PhysRect { x: 0, y: 0, w: 5, h: 5 }).is_none());
    }

    #[test]
    fn clamp_rect_negative_and_overflow() {
        // 拖出屏幕左上：负坐标裁到 0，宽高相应缩短
        assert_eq!(clamp_rect(-30, -10, 100, 50), Some(PhysRect { x: 0, y: 0, w: 70, h: 40 }));
        assert_eq!(clamp_rect(10, 20, 30, 40), Some(PhysRect { x: 10, y: 20, w: 30, h: 40 }));
        assert_eq!(clamp_rect(-100, 0, 50, 50), None);
        assert_eq!(clamp_rect(0, 0, 0, 10), None);
    }

    #[test]
    fn one_failed_monitor_does_not_abort() {
        let ok = FrozenMonitor { index: 1, x: 0, y: 0, width: 2, height: 2, scale: 1.0, image: synthetic(2, 2) };
        let got = collect_monitors(vec![Err("屏 0 截不到".into()), Ok(ok)]).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].index, 1);
        assert!(collect_monitors(vec![Err("a".into()), Err("b".into())]).is_err());
    }

    #[test]
    fn ai_jpeg_long_side_capped() {
        let bytes = ai_jpeg(&synthetic(4000, 1000));
        let img = image::load_from_memory(&bytes).unwrap();
        assert_eq!((img.width(), img.height()), (1600, 400));
        assert_eq!(&bytes[..2], &[0xFF, 0xD8]);
        let small = image::load_from_memory(&ai_jpeg(&synthetic(800, 600))).unwrap();
        assert_eq!((small.width(), small.height()), (800, 600));
    }

    #[test]
    fn bmp_is_top_down_bgra() {
        let mut img = RgbaImage::new(2, 2);
        img.put_pixel(0, 0, Rgba([10, 20, 30, 255]));
        let bmp = to_bmp(&img);
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(bmp.len(), 14 + 40 + 2 * 2 * 4);
        // 高度为负表示从上到下
        let height = i32::from_le_bytes(bmp[22..26].try_into().unwrap());
        assert_eq!(height, -2);
        assert_eq!(&bmp[54..58], &[30, 20, 10, 255]);
        let decoded = image::load_from_memory(&bmp).unwrap().to_rgba8();
        assert_eq!(*decoded.get_pixel(0, 0), Rgba([10, 20, 30, 255]));
    }

    #[test]
    fn png_roundtrip() {
        let img = synthetic(64, 48);
        let decoded = image::load_from_memory(&png_bytes(&img)).unwrap().to_rgba8();
        assert_eq!(decoded, img);
    }

    #[test]
    fn thumb_keeps_aspect_and_is_jpeg() {
        let t = thumbnail_fit(&synthetic(1200, 300), 240, 150);
        assert_eq!(t.dimensions(), (240, 60));
        let small = thumbnail_fit(&synthetic(100, 50), 240, 150);
        assert_eq!(small.dimensions(), (100, 50));
        let bytes = thumb_jpeg(&t);
        assert_eq!(image::load_from_memory(&bytes).unwrap().width(), 240);
    }

    #[test]
    fn shot_id_format() {
        let t = chrono::NaiveDate::from_ymd_opt(2026, 10, 15).unwrap().and_hms_opt(18, 30, 12).unwrap();
        let id = shot_id(t);
        assert!(id.starts_with("20261015-183012-"), "{id}");
        assert_eq!(id.len(), "20261015-183012-a1b2".len());
        assert_ne!(shot_id(t), shot_id(t));
    }
}
