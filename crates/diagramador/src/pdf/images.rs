//! Image embedding.
//!
//! Images become `ImageXObject` streams in `DeviceRGB`, Flate-compressed. When
//! the source has an alpha channel it is written as a separate soft mask, which
//! is how PDF expresses per-pixel transparency.

use std::collections::{BTreeMap, BTreeSet};

use miniz_oxide::deflate::compress_to_vec_zlib;
use pdf_writer::{Chunk, Filter, Ref};

use super::{PdfError, RefAlloc};
use crate::display::{DisplayItem, DisplayList};
use crate::images::{ColorSpace, ImageStore};

/// Zlib level 6: a good size/time trade-off for a WASM build.
const COMPRESSION_LEVEL: u8 = 6;

#[derive(Debug)]
pub struct EmbeddedImage {
    pub xobject_ref: Ref,
    pub resource_name: String,
}

#[derive(Debug, Default)]
pub struct ImageMap {
    pub images: BTreeMap<String, EmbeddedImage>,
}

impl ImageMap {
    pub fn get(&self, key: &str) -> Option<&EmbeddedImage> {
        self.images.get(key)
    }

    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }
}

/// Every image key the display list actually paints.
pub fn collect_images(list: &DisplayList) -> BTreeSet<String> {
    fn walk(items: &[DisplayItem], out: &mut BTreeSet<String>) {
        for item in items {
            match item {
                DisplayItem::Group(group) => walk(&group.items, out),
                DisplayItem::Image(image) => {
                    out.insert(image.src.clone());
                }
                _ => {}
            }
        }
    }

    let mut out = BTreeSet::new();
    for page in &list.pages {
        walk(&page.items, &mut out);
    }
    out
}

/// Write the used images into `chunk`.
///
/// An image that fails to decode is skipped rather than aborting the export —
/// a broken photo should not cost the author the rest of the document.
pub fn embed_images(
    chunk: &mut Chunk,
    store: &ImageStore,
    keys: &BTreeSet<String>,
    alloc: &mut RefAlloc,
) -> Result<ImageMap, PdfError> {
    let mut images = BTreeMap::new();

    for (index, key) in keys.iter().enumerate() {
        // A JPEG can go in untouched: PDF speaks DCT natively. Decoding it to
        // samples and re-compressing with Flate is both lossy and far larger —
        // photographs are exactly what Flate is bad at.
        if let Some(entry) = store.get(key)
            && let Some(jpeg) = inspect_jpeg(&entry.bytes)
        {
            let xobject_ref = alloc.alloc();
            let mut image = chunk.image_xobject(xobject_ref, &entry.bytes);
            image
                .width(jpeg.width as i32)
                .height(jpeg.height as i32)
                .bits_per_component(8);
            match jpeg.components {
                1 => image.color_space().device_gray(),
                _ => image.color_space().device_rgb(),
            }
            image.filter(Filter::DctDecode);

            images.insert(
                key.clone(),
                EmbeddedImage {
                    xobject_ref,
                    resource_name: format!("Im{index}"),
                },
            );
            continue;
        }

        let Ok(decoded) = store.decode(key) else {
            continue;
        };
        if decoded.width == 0 || decoded.height == 0 {
            continue;
        }

        let xobject_ref = alloc.alloc();
        let mask_ref = decoded.alpha.as_ref().map(|_| alloc.alloc());

        let compressed = compress_to_vec_zlib(&decoded.samples, COMPRESSION_LEVEL);
        {
            let mut image = chunk.image_xobject(xobject_ref, &compressed);
            image
                .width(decoded.width as i32)
                .height(decoded.height as i32)
                .bits_per_component(8);

            match decoded.color_space {
                ColorSpace::Rgb => image.color_space().device_rgb(),
                ColorSpace::Gray => image.color_space().device_gray(),
            }

            if let Some(mask) = mask_ref {
                image.s_mask(mask);
            }

            // `filter` comes from the underlying Stream, so it goes last.
            image.filter(Filter::FlateDecode);
        }

        if let (Some(mask_ref), Some(alpha)) = (mask_ref, decoded.alpha) {
            let compressed = compress_to_vec_zlib(&alpha, COMPRESSION_LEVEL);
            let mut mask = chunk.image_xobject(mask_ref, &compressed);
            mask.width(decoded.width as i32)
                .height(decoded.height as i32)
                .bits_per_component(8);
            mask.color_space().device_gray();
            mask.filter(Filter::FlateDecode);
        }

        images.insert(
            key.clone(),
            EmbeddedImage {
                xobject_ref,
                resource_name: format!("Im{index}"),
            },
        );
    }

    Ok(ImageMap { images })
}

/// What a JPEG needs to declare for PDF to decode it itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct JpegInfo {
    width: u16,
    height: u16,
    components: u8,
}

/// Read a JPEG's frame header, if it is one PDF can take verbatim.
///
/// Returns `None` for anything that is not a plain baseline or extended
/// sequential JPEG in grayscale or YCbCr — progressive frames, CMYK, arithmetic
/// coding and lossless variants all go down the decode path instead, because a
/// viewer is not obliged to handle them through `DCTDecode`.
fn inspect_jpeg(bytes: &[u8]) -> Option<JpegInfo> {
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
        // Padding and standalone markers carry no length field.
        if marker == 0xFF {
            i += 1;
            continue;
        }
        if matches!(marker, 0xD8 | 0xD9 | 0x01) || (0xD0..=0xD7).contains(&marker) {
            i += 2;
            continue;
        }

        let length = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;

        match marker {
            // Baseline and extended sequential: the two PDF is safe with.
            0xC0 | 0xC1 => {
                let height = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]);
                let width = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]);
                let components = bytes[i + 9];
                if width == 0 || height == 0 || !matches!(components, 1 | 3) {
                    return None;
                }
                return Some(JpegInfo {
                    width,
                    height,
                    components,
                });
            }
            // Any other frame type: let the decoder handle it.
            0xC2 | 0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => return None,
            // Reached the scan without a frame header: not a usable JPEG.
            0xDA => return None,
            _ => i += 2 + length,
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::{DisplayGroup, DisplayPage, ImageItem};

    fn list_with(items: Vec<DisplayItem>) -> DisplayList {
        let mut list = DisplayList::new();
        list.pages.push(DisplayPage {
            items,
            ..Default::default()
        });
        list
    }

    fn image(src: &str) -> DisplayItem {
        DisplayItem::Image(ImageItem {
            src: src.to_string(),
            ..Default::default()
        })
    }

    #[test]
    fn collects_used_keys_without_duplicates() {
        let list = list_with(vec![image("a.png"), image("b.png"), image("a.png")]);
        let keys = collect_images(&list);
        assert_eq!(keys.len(), 2);
        assert!(keys.contains("a.png") && keys.contains("b.png"));
    }

    #[test]
    fn finds_images_nested_in_groups() {
        let list = list_with(vec![DisplayItem::Group(DisplayGroup {
            items: vec![image("dentro.png")],
            ..DisplayGroup::new()
        })]);
        assert!(collect_images(&list).contains("dentro.png"));
    }

    /// Build a JPEG far enough to carry a frame header — all `inspect_jpeg`
    /// reads.
    fn jpeg_header(marker: u8, width: u16, height: u16, components: u8) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8];
        // A JFIF APP0 segment, so the parser has to skip something first.
        bytes.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x10]);
        bytes.extend_from_slice(b"JFIF\0");
        bytes.extend_from_slice(&[1, 1, 0, 0, 1, 0, 1, 0, 0]);

        bytes.extend_from_slice(&[0xFF, marker]);
        bytes.extend_from_slice(&(8 + 3 * components as u16).to_be_bytes());
        bytes.push(8);
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.push(components);
        for id in 0..components {
            bytes.extend_from_slice(&[id + 1, 0x11, 0]);
        }
        bytes.extend_from_slice(&[0xFF, 0xDA, 0, 8, 1, 1, 0, 0, 0x3F, 0]);
        bytes
    }

    #[test]
    fn a_baseline_jpeg_is_taken_verbatim() {
        let info = inspect_jpeg(&jpeg_header(0xC0, 640, 480, 3)).expect("baseline aceito");
        assert_eq!(info.width, 640);
        assert_eq!(info.height, 480);
        assert_eq!(info.components, 3);

        // Extended sequential is equally safe.
        assert!(inspect_jpeg(&jpeg_header(0xC1, 100, 50, 1)).is_some());
    }

    #[test]
    fn a_progressive_jpeg_goes_through_the_decoder() {
        // PDF viewers are not obliged to handle SOF2 through DCTDecode.
        assert_eq!(inspect_jpeg(&jpeg_header(0xC2, 640, 480, 3)), None);
        // Nor arithmetic-coded or lossless frames.
        assert_eq!(inspect_jpeg(&jpeg_header(0xC3, 640, 480, 3)), None);
        assert_eq!(inspect_jpeg(&jpeg_header(0xC9, 640, 480, 3)), None);
    }

    #[test]
    fn cmyk_and_degenerate_frames_are_refused() {
        assert_eq!(inspect_jpeg(&jpeg_header(0xC0, 640, 480, 4)), None);
        assert_eq!(inspect_jpeg(&jpeg_header(0xC0, 0, 480, 3)), None);
    }

    #[test]
    fn non_jpegs_are_refused() {
        const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
        assert_eq!(inspect_jpeg(PNG), None);
        assert_eq!(inspect_jpeg(b"nope"), None);
        assert_eq!(inspect_jpeg(&[]), None);
        // Truncated before the frame header.
        assert_eq!(inspect_jpeg(&[0xFF, 0xD8, 0xFF, 0xE0]), None);
    }

    #[cfg(feature = "images")]
    #[test]
    fn a_real_jpeg_is_embedded_without_re_encoding() {
        let Ok(bytes) = std::fs::read("../../examples/images/terra.jpg")
            .or_else(|_| std::fs::read("examples/images/terra.jpg"))
        else {
            return;
        };
        let original = bytes.len();

        let mut store = ImageStore::new();
        store.add("terra.jpg", bytes);

        let keys = BTreeSet::from(["terra.jpg".to_string()]);
        let mut chunk = Chunk::new();
        let mut alloc = RefAlloc::new();
        let map = embed_images(&mut chunk, &store, &keys, &mut alloc).unwrap();

        assert!(map.get("terra.jpg").is_some(), "a imagem não foi embutida");
        // One object, not two: a JPEG has no alpha, so there is no soft mask.
        assert_eq!(alloc.alloc().get(), 2);

        // The stream must be the original bytes, so the chunk stays near their
        // size instead of ballooning into raw samples.
        let written = chunk.as_bytes().len();
        assert!(
            written < original + 2048,
            "esperava ~{original} bytes, o chunk tem {written}"
        );
    }

    #[test]
    fn unregistered_images_are_skipped_not_fatal() {
        let store = ImageStore::new();
        let keys = BTreeSet::from(["ausente.png".to_string()]);
        let mut chunk = Chunk::new();
        let mut alloc = RefAlloc::new();

        let map = embed_images(&mut chunk, &store, &keys, &mut alloc).unwrap();
        assert!(map.is_empty());
    }

    #[cfg(feature = "images")]
    #[test]
    fn a_transparent_png_gets_a_soft_mask() {
        const TINY_PNG: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];

        let mut store = ImageStore::new();
        store.add("t.png", TINY_PNG.to_vec());

        let keys = BTreeSet::from(["t.png".to_string()]);
        let mut chunk = Chunk::new();
        let mut alloc = RefAlloc::new();

        let map = embed_images(&mut chunk, &store, &keys, &mut alloc).unwrap();
        let embedded = map.get("t.png").expect("embedded");
        assert_eq!(embedded.resource_name, "Im0");
        // Image object plus its soft mask.
        assert_eq!(alloc.alloc().get(), 3);
    }
}
