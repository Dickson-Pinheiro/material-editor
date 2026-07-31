//! Font subsetting and embedding.
//!
//! Each face used anywhere in the document is embedded once, as a `Type0` font
//! with `Identity-H` encoding and a `Type2` CID descendant. Only the glyphs
//! actually used survive subsetting.
//!
//! `/DW 0` makes the PDF cursor stay put after each glyph, so every advance in
//! the file comes from the layout engine rather than from the font's own
//! metrics. That is what keeps the PDF identical to the canvas: there is no
//! second opinion about how wide a glyph is.

use std::collections::BTreeMap;

use pdf_writer::types::{CidFontType, FontFlags, SystemInfo, UnicodeCmap};
use pdf_writer::{Chunk, Name, Rect, Ref, Str};
use subsetter::GlyphRemapper;

use super::{PdfError, RefAlloc};
use crate::display::{DisplayItem, DisplayList};
use crate::fonts::{FontId, FontRegistry};

/// Glyphs used from one face, mapped to the text they render.
///
/// The text comes from the display list's own runs, so a ligature maps to all
/// the characters it stands for and copy-paste out of the PDF returns the
/// original wording.
pub type GlyphUsage = BTreeMap<u16, String>;

#[derive(Debug)]
pub struct EmbeddedFont {
    pub type0_ref: Ref,
    pub resource_name: String,
    pub remapper: GlyphRemapper,
}

#[derive(Debug, Default)]
pub struct FontMap {
    pub fonts: BTreeMap<u32, EmbeddedFont>,
}

impl FontMap {
    pub fn get(&self, font: u32) -> Option<&EmbeddedFont> {
        self.fonts.get(&font)
    }

    pub fn is_empty(&self) -> bool {
        self.fonts.is_empty()
    }
}

/// Walk the display list and record which glyphs each face needs.
pub fn collect_glyphs(list: &DisplayList) -> BTreeMap<u32, GlyphUsage> {
    fn walk(items: &[DisplayItem], out: &mut BTreeMap<u32, GlyphUsage>) {
        for item in items {
            match item {
                DisplayItem::Group(group) => walk(&group.items, out),
                DisplayItem::Glyphs(run) => {
                    let usage = out.entry(run.font).or_default();
                    for (index, glyph) in run.glyphs.iter().enumerate() {
                        // A cluster spans up to the next glyph's cluster, which
                        // is how one ligature glyph maps to several characters.
                        let start = glyph.cluster as usize;
                        let end = run
                            .glyphs
                            .get(index + 1)
                            .map_or(run.text.len(), |next| next.cluster as usize);

                        if start <= end && end <= run.text.len() && run.text.is_char_boundary(start)
                        {
                            let text = &run.text[start..end.max(start)];
                            if !text.is_empty() {
                                usage.entry(glyph.id).or_insert_with(|| text.to_string());
                            }
                        }
                        usage.entry(glyph.id).or_default();
                    }
                }
                _ => {}
            }
        }
    }

    let mut out = BTreeMap::new();
    for page in &list.pages {
        walk(&page.items, &mut out);
    }
    out
}

/// Subset and write every used face into `chunk`.
pub fn embed_fonts(
    chunk: &mut Chunk,
    registry: &FontRegistry,
    usage: &BTreeMap<u32, GlyphUsage>,
    alloc: &mut RefAlloc,
) -> Result<FontMap, PdfError> {
    let adobe = SystemInfo {
        registry: Str(b"Adobe"),
        ordering: Str(b"Identity"),
        supplement: 0,
    };

    let mut fonts = BTreeMap::new();

    for (index, (font, glyphs)) in usage.iter().enumerate() {
        let Some(face) = registry.face(FontId(*font)) else {
            continue;
        };

        // ── Subset ────────────────────────────────────────────────────────────
        let mut remapper = GlyphRemapper::new();
        for gid in glyphs.keys() {
            remapper.remap(*gid);
        }

        let subset =
            subsetter::subset(face.bytes(), 0, &remapper).map_err(|e| PdfError::Subset {
                family: face.family.clone(),
                reason: e.to_string(),
            })?;

        let type0_ref = alloc.alloc();
        let cid_ref = alloc.alloc();
        let descriptor_ref = alloc.alloc();
        let file_ref = alloc.alloc();
        let cmap_ref = alloc.alloc();

        // ── Descriptor metrics, in PDF's 1/1000 em units ──────────────────────
        let ttf = face.ttf();
        let upem = face.metrics.units_per_em as f32;
        let scale = 1000.0 / upem;
        let bbox = ttf.global_bounding_box();

        let mut flags = FontFlags::SYMBOLIC;
        if face.italic {
            flags |= FontFlags::ITALIC;
        }
        if face.weight.is_bold() {
            flags |= FontFlags::FORCE_BOLD;
        }

        let name = face.post_script_name.as_bytes();

        // ── ToUnicode ─────────────────────────────────────────────────────────
        let mut cmap = UnicodeCmap::new(Name(b"Adobe-Identity-UCS"), adobe);
        for (gid, text) in glyphs {
            let Some(new_gid) = remapper.get(*gid) else {
                continue;
            };
            let mut chars = text.chars();
            match (chars.next(), chars.next()) {
                (Some(single), None) => cmap.pair(new_gid, single),
                (Some(_), Some(_)) => {
                    cmap.pair_with_multiple(new_gid, text.chars());
                }
                _ => {}
            }
        }
        chunk.cmap(cmap_ref, &cmap.finish()).system_info(adobe);

        chunk.stream(file_ref, &subset);

        chunk
            .font_descriptor(descriptor_ref)
            .name(Name(name))
            .flags(flags)
            .bbox(Rect::new(
                bbox.x_min as f32 * scale,
                bbox.y_min as f32 * scale,
                bbox.x_max as f32 * scale,
                bbox.y_max as f32 * scale,
            ))
            .italic_angle(face.metrics.italic_angle as f32)
            .ascent(face.metrics.ascender as f32 * 1000.0)
            .descent(face.metrics.descender as f32 * 1000.0)
            .cap_height(face.metrics.cap_height as f32 * 1000.0)
            .stem_v(80.0)
            .font_file2(file_ref);

        // Default width 0: the content stream supplies every advance itself.
        chunk
            .cid_font(cid_ref)
            .subtype(CidFontType::Type2)
            .base_font(Name(name))
            .system_info(adobe)
            .font_descriptor(descriptor_ref)
            .default_width(0.0);

        chunk
            .type0_font(type0_ref)
            .base_font(Name(name))
            .encoding_predefined(Name(b"Identity-H"))
            .descendant_font(cid_ref)
            .to_unicode(cmap_ref);

        fonts.insert(
            *font,
            EmbeddedFont {
                type0_ref,
                resource_name: format!("F{index}"),
                remapper,
            },
        );
    }

    Ok(FontMap { fonts })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::{DisplayPage, Glyph, GlyphRun};

    fn list_with(runs: Vec<GlyphRun>) -> DisplayList {
        let mut list = DisplayList::new();
        list.pages.push(DisplayPage {
            items: runs.into_iter().map(DisplayItem::Glyphs).collect(),
            ..Default::default()
        });
        list
    }

    fn run(font: u32, text: &str, glyphs: Vec<(u16, u32)>) -> GlyphRun {
        GlyphRun {
            font,
            text: text.to_string(),
            glyphs: glyphs
                .into_iter()
                .map(|(id, cluster)| Glyph {
                    id,
                    cluster,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn collects_glyphs_per_face() {
        let list = list_with(vec![
            run(0, "ab", vec![(10, 0), (11, 1)]),
            run(1, "c", vec![(12, 0)]),
        ]);
        let usage = collect_glyphs(&list);
        assert_eq!(usage.len(), 2);
        assert_eq!(usage[&0].len(), 2);
        assert_eq!(usage[&0][&10], "a");
        assert_eq!(usage[&0][&11], "b");
        assert_eq!(usage[&1][&12], "c");
    }

    #[test]
    fn unions_glyphs_across_runs_and_pages() {
        let mut list = list_with(vec![run(0, "ab", vec![(10, 0), (11, 1)])]);
        list.pages.push(DisplayPage {
            items: vec![DisplayItem::Glyphs(run(0, "bc", vec![(11, 0), (12, 1)]))],
            ..Default::default()
        });
        let usage = collect_glyphs(&list);
        assert_eq!(usage[&0].keys().copied().collect::<Vec<_>>(), vec![10, 11, 12]);
    }

    #[test]
    fn a_ligature_maps_to_every_character_it_stands_for() {
        // One glyph covering the two bytes of "fi".
        let list = list_with(vec![run(0, "fix", vec![(500, 0), (30, 2)])]);
        let usage = collect_glyphs(&list);
        assert_eq!(usage[&0][&500], "fi");
        assert_eq!(usage[&0][&30], "x");
    }

    #[test]
    fn glyphs_inside_groups_are_found() {
        use crate::display::DisplayGroup;
        let mut list = DisplayList::new();
        list.pages.push(DisplayPage {
            items: vec![DisplayItem::Group(DisplayGroup {
                items: vec![DisplayItem::Glyphs(run(3, "z", vec![(99, 0)]))],
                ..DisplayGroup::new()
            })],
            ..Default::default()
        });
        assert_eq!(collect_glyphs(&list)[&3][&99], "z");
    }

    #[test]
    fn an_empty_list_needs_no_fonts() {
        assert!(collect_glyphs(&DisplayList::new()).is_empty());
    }
}
