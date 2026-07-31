//! The style cascade.
//!
//! Named styles live in `resources.styles` and may chain through `extends`.
//! Resolution order, weakest to strongest:
//!
//! 1. the inherited [`ResolvedStyle`] from the enclosing scope
//! 2. the named style referenced by `"use"`, with its own `extends` chain
//! 3. the `extends` target of the inline style object
//! 4. the inline style object itself

use std::collections::BTreeMap;

use crate::spec::{ResolvedStyle, Style};

/// Guard against `extends` cycles in hand-written JSON.
const MAX_EXTENDS_DEPTH: usize = 8;

/// Flatten a named style and everything it extends into a single patch.
fn named(styles: &BTreeMap<String, Style>, name: &str, depth: usize) -> Style {
    if depth >= MAX_EXTENDS_DEPTH {
        return Style::default();
    }
    let Some(style) = styles.get(name) else {
        return Style::default();
    };

    match &style.extends {
        Some(parent) => named(styles, parent, depth + 1).merge(style),
        None => style.clone(),
    }
}

/// Combine a `"use"` reference and an inline style object into one patch.
pub fn patch(
    styles: &BTreeMap<String, Style>,
    use_name: Option<&str>,
    inline: Option<&Style>,
) -> Style {
    let mut acc = match use_name {
        Some(name) => named(styles, name, 0),
        None => Style::default(),
    };

    if let Some(style) = inline {
        if let Some(parent) = &style.extends {
            acc = acc.merge(&named(styles, parent, 0));
        }
        acc = acc.merge(style);
    }

    acc
}

/// Apply a `"use"` reference plus an inline override on top of `parent`.
pub fn resolve(
    parent: &ResolvedStyle,
    styles: &BTreeMap<String, Style>,
    use_name: Option<&str>,
    inline: Option<&Style>,
) -> ResolvedStyle {
    parent.apply(&patch(styles, use_name, inline))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::spec::{FontWeight, TextAlign};
    use crate::units::Len;

    fn styles() -> BTreeMap<String, Style> {
        let mut map = BTreeMap::new();
        map.insert(
            "base".to_string(),
            Style {
                font_size: Some(Len(10.0)),
                color: Some(Color::BLACK),
                ..Default::default()
            },
        );
        map.insert(
            "h1".to_string(),
            Style {
                extends: Some("base".into()),
                font_size: Some(Len(20.0)),
                font_weight: Some(FontWeight::BOLD),
                ..Default::default()
            },
        );
        map
    }

    #[test]
    fn named_style_inherits_through_extends() {
        let resolved = resolve(&ResolvedStyle::default(), &styles(), Some("h1"), None);
        assert_eq!(resolved.font_size, 20.0);
        assert_eq!(resolved.font_weight, FontWeight::BOLD);
        // Inherited from "base" via the extends chain.
        assert_eq!(resolved.color, Color::BLACK);
    }

    #[test]
    fn inline_style_beats_the_named_one() {
        let inline = Style {
            font_size: Some(Len(30.0)),
            ..Default::default()
        };
        let resolved = resolve(&ResolvedStyle::default(), &styles(), Some("h1"), Some(&inline));
        assert_eq!(resolved.font_size, 30.0);
        // Untouched fields still come from the named style.
        assert_eq!(resolved.font_weight, FontWeight::BOLD);
    }

    #[test]
    fn unknown_names_resolve_to_the_parent() {
        let parent = ResolvedStyle {
            font_size: 13.0,
            ..Default::default()
        };
        let resolved = resolve(&parent, &styles(), Some("nao-existe"), None);
        assert_eq!(resolved.font_size, 13.0);
    }

    #[test]
    fn extends_cycles_terminate() {
        let mut map = BTreeMap::new();
        map.insert(
            "a".to_string(),
            Style {
                extends: Some("b".into()),
                font_size: Some(Len(11.0)),
                ..Default::default()
            },
        );
        map.insert(
            "b".to_string(),
            Style {
                extends: Some("a".into()),
                text_align: Some(TextAlign::Center),
                ..Default::default()
            },
        );

        let resolved = resolve(&ResolvedStyle::default(), &map, Some("a"), None);
        assert_eq!(resolved.font_size, 11.0);
        assert_eq!(resolved.text_align, TextAlign::Center);
    }

    #[test]
    fn inline_extends_is_honoured() {
        let inline = Style {
            extends: Some("h1".into()),
            color: Some(Color::WHITE),
            ..Default::default()
        };
        let resolved = resolve(&ResolvedStyle::default(), &styles(), None, Some(&inline));
        assert_eq!(resolved.font_size, 20.0);
        assert_eq!(resolved.color, Color::WHITE);
    }

    #[test]
    fn inheritance_flows_from_the_parent_when_nothing_is_set() {
        let parent = ResolvedStyle {
            font_size: 9.0,
            text_align: TextAlign::Justify,
            ..Default::default()
        };
        let resolved = resolve(&parent, &BTreeMap::new(), None, None);
        assert_eq!(resolved, parent);
    }
}
