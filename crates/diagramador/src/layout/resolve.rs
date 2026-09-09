//! Sugar that becomes core before layout: component instances.
//!
//! An instance names a component and fills its slots. Here it turns into a
//! group holding a copy of the component's frames, positioned and stretched
//! for the instance's rect, with the slot content in place. From then on the
//! engine sees only a group; nothing downstream knows components exist.

use std::collections::BTreeMap;

use crate::display::Diagnostic;
use crate::spec::{Component, Document, Frame, FrameContent, GroupFrame, SlotValue};

/// Instances nested deeper than this are a cycle, or as good as one.
const MAX_DEPTH: u32 = 8;

/// Replace every instance in pages and masters by the group it stands for.
pub fn instances(doc: &mut Document, diagnostics: &mut Vec<Diagnostic>) {
    let components = doc.resources.components.clone();
    if components.is_empty() {
        strip_instances(doc, diagnostics);
        return;
    }

    for (index, page) in doc.pages.iter_mut().enumerate() {
        let page_no = index as u32;
        resolve_frames(&mut page.frames, &components, page_no, 0, diagnostics);
    }
    for master in doc.resources.masters.values_mut() {
        resolve_frames(&mut master.frames, &components, 0, 0, diagnostics);
    }
}

/// Without components declared, every instance is unknown.
fn strip_instances(doc: &mut Document, diagnostics: &mut Vec<Diagnostic>) {
    let empty = BTreeMap::new();
    for (index, page) in doc.pages.iter_mut().enumerate() {
        resolve_frames(&mut page.frames, &empty, index as u32, 0, diagnostics);
    }
    for master in doc.resources.masters.values_mut() {
        resolve_frames(&mut master.frames, &empty, 0, 0, diagnostics);
    }
}

fn resolve_frames(
    frames: &mut [Frame],
    components: &BTreeMap<String, Component>,
    page: u32,
    depth: u32,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for frame in frames.iter_mut() {
        match &mut frame.content {
            FrameContent::Instance(_) => {
                resolve_one(frame, components, page, depth, diagnostics);
            }
            FrameContent::Group(group) => {
                resolve_frames(&mut group.children, components, page, depth, diagnostics);
            }
            _ => {}
        }
    }
}

fn resolve_one(
    frame: &mut Frame,
    components: &BTreeMap<String, Component>,
    page: u32,
    depth: u32,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let FrameContent::Instance(instance) = &frame.content else {
        return;
    };
    let id = frame.id.clone().unwrap_or_default();

    if depth >= MAX_DEPTH {
        diagnostics.push(
            Diagnostic::warning(
                "componentCycle",
                format!("componente `{}` instancia a si mesmo", instance.component),
            )
            .on(page, id),
        );
        frame.content = FrameContent::Group(GroupFrame::default());
        return;
    }

    let Some(component) = components.get(&instance.component) else {
        diagnostics.push(
            Diagnostic::warning(
                "unknownComponent",
                format!("componente `{}` não existe em resources.components", instance.component),
            )
            .on(page, id),
        );
        frame.content = FrameContent::Group(GroupFrame::default());
        return;
    };

    let dw = frame.rect.w - component.size[0];
    let dh = frame.rect.h - component.size[1];
    let mut slots = instance.slots.clone();
    let name = instance.component.clone();

    let mut children: Vec<Frame> = component
        .frames
        .iter()
        .enumerate()
        .map(|(index, template)| {
            let mut child = template.clone();
            // Ids from the component would collide between two instances of
            // it. Each copy is named after the instance, then the slot, the
            // component's own id, or its position.
            let suffix = child
                .slot
                .clone()
                .or_else(|| child.id.clone().filter(|id| !id.is_empty()))
                .unwrap_or_else(|| format!("f{index}"));
            child.id = Some(format!("{id}.{suffix}"));
            if child.follow.x {
                child.rect.x += dw;
            }
            if child.follow.y {
                child.rect.y += dh;
            }
            if child.follow.w {
                child.rect.w += dw;
            }
            if child.follow.h {
                child.rect.h += dh;
            }
            if let Some(slot) = child.slot.take() {
                match slots.remove(&slot) {
                    Some(value) => fill(&mut child, value),
                    None => {}
                }
            }
            child
        })
        .collect();

    for slot in slots.keys() {
        diagnostics.push(
            Diagnostic::warning(
                "unknownSlot",
                format!("componente `{name}` não tem o slot `{slot}`"),
            )
            .on(page, id.clone()),
        );
    }

    // A component may itself place instances of others.
    resolve_frames(&mut children, components, page, depth + 1, diagnostics);

    frame.content = FrameContent::Group(GroupFrame {
        children,
        instance: Some(name),
    });
}

/// Put a slot's value into the frame that declared the slot.
fn fill(frame: &mut Frame, value: SlotValue) {
    match (&mut frame.content, value) {
        (FrameContent::Text(text), SlotValue::Text(raw)) => {
            text.blocks = vec![crate::spec::Block::text(raw)];
            text.story = None;
        }
        (FrameContent::Text(text), SlotValue::Blocks(blocks)) => {
            text.blocks = blocks;
            text.story = None;
        }
        (FrameContent::Image(image), SlotValue::Image { src }) => {
            image.src = src;
        }
        // A text value on an image frame, or an image on a text frame: the
        // slot keeps what the component designed. Silently, because the
        // author of the instance may not have written the component.
        _ => {}
    }
}
