//! `document.toneStats`: numbers that describe how an image looks (brightness, contrast, clipped
//! shadows and highlights, saturation, colour cast), from the merged composite. An agent reads
//! them alongside a screenshot to suggest corrections grounded in the pixels, where a glance at a
//! downscaled screenshot can mislead (a dark UI around a bright photo, a small preview).

use photocraft_color::PixelFormat;
use photocraft_geom::Rect;
use serde_json::{Value, json};

use crate::commands::CommandSpec;
use crate::{EngineError, Result, Session};

const ID: &str = "document.toneStats";

/// About this many pixels are sampled, evenly, whatever the document's size.
const TARGET_SAMPLES: u64 = 250_000;

fn has_doc(s: &Session) -> std::result::Result<(), String> {
    s.active().map(|_| ()).ok_or_else(|| "no document open".into())
}

/// Photoshop's luminosity weights, on the encoded (display) values, 0..1.
fn luma(p: &[f32; 4]) -> f32 {
    0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2]
}

fn tone_stats(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.active().ok_or(EngineError::NoDocument)?;
    let doc = &d.doc;
    let (w, h) = (doc.size.width, doc.size.height);
    if w == 0 || h == 0 {
        return Err(EngineError::Other("the document is empty".into()));
    }
    let fmt = doc.pixel_format();
    let merged = photocraft_compose::flatten_to_surface(doc, PixelFormat::new(fmt.mode, fmt.sample, true), None);
    // Every `step`-th pixel of every `step`-th row.
    let area = u64::from(w).saturating_mul(u64::from(h));
    let step = ((area as f64 / TARGET_SAMPLES as f64).sqrt().ceil() as u32).max(1);
    let mut row = vec![[0f32; 4]; w as usize];
    let mut hist = [0u64; 256];
    let (mut n, mut seen) = (0u64, 0u64);
    let (mut sum, mut sum_sq, mut sat) = (0f64, 0f64, 0f64);
    let mut rgb = [0f64; 3];
    let mut y = 0u32;
    while y < h {
        let yi = i32::try_from(y).unwrap_or(i32::MAX);
        merged.read_rgba_into(Rect::new(0, yi, i32::try_from(w).unwrap_or(i32::MAX), yi.saturating_add(1)), &mut row);
        for p in row.iter().step_by(step as usize) {
            seen += 1;
            if p[3] < 0.5 {
                continue;
            }
            let c = [p[0].clamp(0.0, 1.0), p[1].clamp(0.0, 1.0), p[2].clamp(0.0, 1.0), 1.0];
            let l = luma(&c);
            hist[(l * 255.0).round().clamp(0.0, 255.0) as usize] += 1;
            sum += f64::from(l);
            sum_sq += f64::from(l) * f64::from(l);
            let (mx, mn) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
            if mx > 0.0 {
                sat += f64::from((mx - mn) / mx);
            }
            for k in 0..3 {
                rgb[k] += f64::from(c[k]);
            }
            n += 1;
        }
        y = y.saturating_add(step);
    }
    if n == 0 {
        return Ok(json!({"width": w, "height": h, "opaque": 0.0, "note": "the image is fully transparent"}));
    }
    let nf = n as f64;
    let mean = sum / nf;
    let std = (sum_sq / nf - mean * mean).max(0.0).sqrt();
    let percentile = |q: f64| -> u32 {
        let target = (q * nf).ceil() as u64;
        let mut acc = 0u64;
        for (i, c) in hist.iter().enumerate() {
            acc += c;
            if acc >= target.max(1) {
                return i as u32;
            }
        }
        255
    };
    let share = |r: std::ops::RangeInclusive<usize>| -> f64 { hist[r].iter().sum::<u64>() as f64 / nf * 100.0 };
    let round = |v: f64, k: f64| (v * k).round() / k;
    // A coarse 16-bin luminance histogram, in percent, dark to light.
    let bins: Vec<f64> = hist.chunks(16).map(|c| round(c.iter().sum::<u64>() as f64 / nf * 100.0, 10.0)).collect();
    let means: Vec<f64> = rgb.iter().map(|v| round(v / nf * 255.0, 1.0)).collect();
    Ok(json!({
        "width": w,
        "height": h,
        "sampled": n,
        "opaque": round(nf / seen.max(1) as f64 * 100.0, 10.0),
        "luminance": {
            "mean": round(mean * 255.0, 1.0),
            "p1": percentile(0.01),
            "p5": percentile(0.05),
            "median": percentile(0.5),
            "p95": percentile(0.95),
            "p99": percentile(0.99),
            "std": round(std * 255.0, 1.0),
        },
        "clippedShadowsPct": round(share(0..=3), 100.0),
        "clippedHighlightsPct": round(share(252..=255), 100.0),
        "saturationMean": round(sat / nf * 100.0, 10.0),
        "rgbMean": means,
        "histogram16": bins,
    }))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: ID,
        label: "Tone Statistics",
        menu: &[],
        shortcut: None,
        params: "{} → {width, height, sampled, opaque:%, luminance:{mean, p1, p5, median, p95, p99, std} (0..255), clippedShadowsPct, clippedHighlightsPct, saturationMean:%, rgbMean:[r,g,b] (0..255), histogram16:[% per 16 levels, dark to light]}. Measured on the merged image (about 250k evenly sampled pixels); transparent pixels are left out.",
        enabled: has_doc,
        run: tone_stats,
        journal: false,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tone_stats_describe_a_dark_image_and_need_a_document() {
        let mut s = Session::new();
        assert!(s.execute(ID, json!({})).is_err(), "no document");
        s.execute("file.new", json!({"width": 120, "height": 80, "background": "black"})).unwrap();
        let r = s.execute(ID, json!({})).unwrap();
        assert_eq!(r["width"], 120);
        assert!(r["luminance"]["mean"].as_f64().unwrap() < 1.0, "{r}");
        assert!(r["clippedShadowsPct"].as_f64().unwrap() > 99.0, "{r}");
        assert_eq!(r["histogram16"].as_array().unwrap().len(), 16);
        s.execute("image.adjustments.invert", json!({})).unwrap();
        let r = s.execute(ID, json!({"ignored": true})).unwrap();
        assert!(r["luminance"]["p1"].as_u64().unwrap() > 250, "{r}");
        assert!(r["clippedHighlightsPct"].as_f64().unwrap() > 99.0, "{r}");
    }
}
