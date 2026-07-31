//! Raster image registration.
//!
//! Images are supplied by the host as raw file bytes under a key. Layout needs
//! only their pixel dimensions (to honour `fit` and to size inline images that
//! declare just one axis); the PDF emitter needs the decoded samples.

use std::collections::BTreeMap;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ImageError {
    #[error("image `{key}` is not registered")]
    Missing { key: String },
    #[error("could not decode image `{key}`: {reason}")]
    Decode { key: String, reason: String },
}

/// Pixel format of a decoded image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSpace {
    Gray,
    Rgb,
}

/// One registered image.
#[derive(Debug, Clone)]
pub struct ImageEntry {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl ImageEntry {
    /// Width divided by height, or 1.0 when the size is unknown.
    pub fn aspect(&self) -> f64 {
        if self.height == 0 {
            1.0
        } else {
            self.width as f64 / self.height as f64
        }
    }
}

/// Decoded samples ready for embedding.
#[derive(Debug, Clone)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub color_space: ColorSpace,
    /// 8 bits per channel, row-major.
    pub samples: Vec<u8>,
    /// 8-bit alpha channel, when the source had one.
    pub alpha: Option<Vec<u8>>,
}

#[derive(Debug, Default)]
pub struct ImageStore {
    entries: BTreeMap<String, ImageEntry>,
}

impl ImageStore {
    pub fn new() -> Self {
        ImageStore::default()
    }

    /// Register image bytes under `key`, probing their dimensions.
    ///
    /// Unrecognised bytes are still stored — with a zero size — so a document
    /// referencing them reports a diagnostic instead of failing outright.
    pub fn add(&mut self, key: &str, bytes: Vec<u8>) {
        let (width, height) = probe_size(&bytes).unwrap_or((0, 0));
        self.entries.insert(
            key.to_string(),
            ImageEntry {
                bytes,
                width,
                height,
            },
        );
    }

    pub fn get(&self, key: &str) -> Option<&ImageEntry> {
        self.entries.get(key)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Decode an image into samples for PDF embedding.
    #[cfg(feature = "images")]
    pub fn decode(&self, key: &str) -> Result<DecodedImage, ImageError> {
        let entry = self.entries.get(key).ok_or_else(|| ImageError::Missing {
            key: key.to_string(),
        })?;

        let decoded = image::load_from_memory(&entry.bytes).map_err(|e| ImageError::Decode {
            key: key.to_string(),
            reason: e.to_string(),
        })?;

        let rgba = decoded.to_rgba8();
        let (width, height) = rgba.dimensions();

        let mut samples = Vec::with_capacity((width * height * 3) as usize);
        let mut alpha = Vec::with_capacity((width * height) as usize);
        let mut has_transparency = false;

        for px in rgba.pixels() {
            samples.extend_from_slice(&px.0[..3]);
            alpha.push(px.0[3]);
            has_transparency |= px.0[3] != 255;
        }

        Ok(DecodedImage {
            width,
            height,
            color_space: ColorSpace::Rgb,
            samples,
            alpha: has_transparency.then_some(alpha),
        })
    }

    #[cfg(not(feature = "images"))]
    pub fn decode(&self, key: &str) -> Result<DecodedImage, ImageError> {
        Err(ImageError::Decode {
            key: key.to_string(),
            reason: "the `images` feature is disabled".into(),
        })
    }
}

/// Read the pixel dimensions from a PNG or JPEG header without decoding.
///
/// Kept header-only so layout stays cheap even with large photos — the editor
/// re-lays-out on every keystroke.
pub fn probe_size(bytes: &[u8]) -> Option<(u32, u32)> {
    probe_png(bytes).or_else(|| probe_jpeg(bytes))
}

fn probe_png(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.len() < 24 || !bytes.starts_with(SIGNATURE) {
        return None;
    }
    // IHDR is always the first chunk: length(4) type(4) width(4) height(4).
    if &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    Some((width, height))
}

fn probe_jpeg(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }

    let mut i = 2usize;
    while i + 9 < bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = bytes[i + 1];
        // Standalone markers carry no length.
        if matches!(marker, 0xD8 | 0xD9 | 0x01) || (0xD0..=0xD7).contains(&marker) {
            i += 2;
            continue;
        }
        let length = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        // SOF0..SOF15, skipping the DHT/JPG/DAC markers interleaved in that range.
        let is_sof = (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if is_sof {
            let height = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
            let width = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
            return Some((width, height));
        }
        i += 2 + length;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smallest possible PNG: 1×1, fully transparent.
    const TINY_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    #[cfg(feature = "images")]
    fn sample_png() -> Vec<u8> {
        std::fs::read("../prova-pdf/img/logo.jpg")
            .ok()
            .unwrap_or_else(|| TINY_PNG.to_vec())
    }

    #[test]
    fn probes_png_dimensions_from_the_header() {
        assert_eq!(probe_png(TINY_PNG), Some((1, 1)));
        assert_eq!(probe_size(TINY_PNG), Some((1, 1)));
    }

    #[test]
    fn rejects_non_images() {
        assert_eq!(probe_size(b"definitely not an image"), None);
        assert_eq!(probe_size(&[]), None);
    }

    #[test]
    fn store_registers_and_probes() {
        let mut store = ImageStore::new();
        store.add("logo", TINY_PNG.to_vec());
        let entry = store.get("logo").unwrap();
        assert_eq!((entry.width, entry.height), (1, 1));
        assert_eq!(entry.aspect(), 1.0);
        assert!(store.contains("logo"));
        assert!(!store.contains("outra"));
    }

    #[test]
    fn unknown_bytes_are_stored_with_zero_size() {
        let mut store = ImageStore::new();
        store.add("quebrada", b"nope".to_vec());
        let entry = store.get("quebrada").unwrap();
        assert_eq!((entry.width, entry.height), (0, 0));
        // Aspect must stay finite so layout never divides by zero.
        assert_eq!(entry.aspect(), 1.0);
    }

    #[test]
    fn clear_empties_the_store() {
        let mut store = ImageStore::new();
        store.add("a", TINY_PNG.to_vec());
        store.clear();
        assert!(store.is_empty());
    }

    #[cfg(feature = "images")]
    #[test]
    fn decodes_to_rgb_plus_alpha() {
        let mut store = ImageStore::new();
        store.add("t", TINY_PNG.to_vec());
        let decoded = store.decode("t").unwrap();
        assert_eq!((decoded.width, decoded.height), (1, 1));
        assert_eq!(decoded.color_space, ColorSpace::Rgb);
        assert_eq!(decoded.samples.len(), 3);
        // The pixel is fully transparent, so an alpha channel must be present.
        assert_eq!(decoded.alpha.as_deref(), Some(&[0u8][..]));
    }

    #[cfg(feature = "images")]
    #[test]
    fn probes_a_real_jpeg_when_one_is_available() {
        let bytes = sample_png();
        if bytes == TINY_PNG {
            return;
        }
        let (w, h) = probe_size(&bytes).expect("real image should probe");
        assert!(w > 0 && h > 0);

        let mut store = ImageStore::new();
        store.add("real", bytes);
        let decoded = store.decode("real").unwrap();
        assert_eq!((decoded.width, decoded.height), (w, h));
    }

    #[test]
    fn decoding_a_missing_key_reports_it() {
        let store = ImageStore::new();
        let err = store.decode("ausente").unwrap_err();
        assert!(matches!(err, ImageError::Missing { .. } | ImageError::Decode { .. }));
    }
}
