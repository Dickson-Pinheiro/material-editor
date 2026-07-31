//! How close does the text actually come to the shape?
//!
//! The wrap asks a band of the page what room a line has. The band can be the
//! whole line box, leading included, or only as far as the glyphs rise and
//! fall. The tighter band gives the text more room; the risk is that a tall
//! accent reaches into a part of the shape the band never consulted.
//!
//! This measures which. For every glyph it takes the real ink box — from the
//! outline, not from the advance — and the shortest horizontal distance from
//! that ink to the silhouette across the rows the ink spans. The wrap's own
//! clearance is what that distance should never fall below.
//!
//! ```sh
//! cargo run --release --example faixa --no-default-features --features images
//! ```
//!
//! Flip `BAND` in `layout/text.rs`, rebuild, and run again to compare.

use diagramador::display::{DisplayItem, DisplayList, GlyphRun};
use diagramador::spec::{Document, FontWeight};
use diagramador::Engine;

/// Clearance every case asks for, and the floor a violation is measured against.
const PADDING: f64 = 6.0;

const BODY: &str = "Um parágrafo de corpo com acentuação à portuguesa — ação, \
coração, ângulo, ímã, súbito — para que ascendentes e descendentes altos passem \
rente à forma e o encosto apareça. O texto segue por várias linhas.";

fn main() {
    let Some(engine) = engine() else {
        eprintln!("fontes ausentes: rode a partir da raiz do repositório");
        return;
    };

    println!("folga pedida: {PADDING} pt\n");
    println!(
        "{:<26} {:>5} {:>6} {:>9} {:>9} {:>9} {:>7}",
        "forma", "corpo", "entrel", "mínima", "média", "desvio", "viol."
    );

    let mut all: Vec<f64> = Vec::new();
    let mut violations = 0usize;
    let mut lines = 0usize;

    for (name, ring) in shapes() {
        for size in [9.0, 12.0, 18.0] {
            for leading in [1.2, 1.6] {
                let document = page(&ring, size, leading);
                let list = engine.layout(&document);
                let gaps = clearances(&engine, &list, &ring_points(&ring));

                if gaps.is_empty() {
                    continue;
                }
                let count = gaps.len();
                let min = gaps.iter().copied().fold(f64::INFINITY, f64::min);
                let mean = gaps.iter().sum::<f64>() / count as f64;
                let deviation =
                    (gaps.iter().map(|g| (g - mean).powi(2)).sum::<f64>() / count as f64).sqrt();
                let broke = gaps.iter().filter(|g| **g < PADDING - 0.01).count();

                println!(
                    "{name:<26} {size:>5.0} {leading:>6.1} {min:>9.2} {mean:>9.2} \
                     {deviation:>9.2} {broke:>7}"
                );

                all.extend(&gaps);
                violations += broke;
                lines += count;
            }
        }
    }

    if all.is_empty() {
        println!("\nnenhuma medida — as formas não encostaram no texto");
        return;
    }
    let mean = all.iter().sum::<f64>() / all.len() as f64;
    let deviation = (all.iter().map(|g| (g - mean).powi(2)).sum::<f64>() / all.len() as f64).sqrt();
    println!(
        "\nTOTAL  {lines} medidas   mínima {:.2}   média {mean:.2}   desvio {deviation:.2}   \
         violações {violations} ({:.1}%)",
        all.iter().copied().fold(f64::INFINITY, f64::min),
        100.0 * violations as f64 / lines as f64,
    );
}

// ─────────────────────────────────────────────────────────────────────────────

fn engine() -> Option<Engine> {
    let mut engine = Engine::new();
    for (file, weight) in [("DejaVuSans.ttf", 400u16), ("DejaVuSans-Bold.ttf", 700)] {
        let bytes = std::fs::read(format!("fonts/{file}")).ok()?;
        engine
            .add_font("corpo", bytes, Some(FontWeight(weight)), Some(false))
            .ok()?;
    }
    Some(engine)
}

/// The shapes, as the `points` array of a contour wrap.
fn shapes() -> Vec<(&'static str, String)> {
    let circle: Vec<String> = (0..32)
        .map(|step| {
            let angle = std::f64::consts::TAU * f64::from(step) / 32.0;
            format!(
                "[{:.4},{:.4}]",
                0.5 + 0.5 * angle.cos(),
                0.5 + 0.5 * angle.sin()
            )
        })
        .collect();

    vec![
        ("círculo", format!("[{}]", circle.join(","))),
        ("triângulo, ápice em cima", "[[0.5,0],[1,1],[0,1]]".into()),
        ("triângulo, ápice em baixo", "[[0,0],[1,0],[0.5,1]]".into()),
        (
            "côncavo em C",
            "[[0,0],[1,0],[1,0.2],[0.3,0.2],[0.3,0.8],[1,0.8],[1,1],[0,1]]".into(),
        ),
        (
            "serrilha fina",
            "[[0,0],[1,0],[1,1],[0.8,0.6],[0.6,1],[0.4,0.6],[0.2,1],[0,0.6]]".into(),
        ),
        ("retângulo (controlo)", "[[0,0],[1,0],[1,1],[0,1]]".into()),
    ]
}

const SHAPE: (f64, f64, f64, f64) = (200.0, 120.0, 140.0, 200.0);

fn page(ring: &str, size: f64, leading: f64) -> Document {
    let (x, y, w, h) = SHAPE;
    serde_json::from_str(&format!(
        r#"{{
            "page": {{ "size": "A4", "margins": "20mm" }},
            "style": {{ "fontFamily": "corpo", "fontSize": {size}, "lineHeight": {leading} }},
            "pages": [{{ "frames": [
                {{ "type": "image", "src": "ausente.png", "rect": [{x}, {y}, {w}, {h}],
                   "wrap": {{ "mode": {{ "kind": "contour", "points": {ring} }},
                              "padding": {PADDING} }} }},
                {{ "type": "text", "rect": [56, 100, 483, 560], "blocks": ["{BODY}"] }}
            ] }}]
        }}"#
    ))
    .expect("fixture válida")
}

/// The ring in page points, the way the engine placed it.
fn ring_points(ring: &str) -> Vec<[f64; 2]> {
    let (x, y, w, h) = SHAPE;
    let normalised: Vec<[f64; 2]> = serde_json::from_str(ring).expect("anel válido");
    normalised
        .into_iter()
        .map(|p| [x + p[0] * w, y + p[1] * h])
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────

fn runs(list: &DisplayList) -> Vec<GlyphRun> {
    fn walk(items: &[DisplayItem], out: &mut Vec<GlyphRun>) {
        for item in items {
            match item {
                DisplayItem::Glyphs(run) => out.push(run.clone()),
                DisplayItem::Group(group) => walk(&group.items, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for page in &list.pages {
        walk(&page.items, &mut out);
    }
    out
}

/// Ink box of one glyph in page points, from its outline.
///
/// Control points count, so the box is never smaller than the true ink — the
/// safe direction for a test that asks whether anything touched.
fn ink_box(engine: &Engine, run: &GlyphRun, index: usize) -> Option<(f64, f64, f64, f64)> {
    let glyph = run.glyphs.get(index)?;
    let path = engine.glyph_path(run.font, glyph.id)?;

    let mut numbers = Vec::new();
    let mut current = String::new();
    for ch in path.chars() {
        if ch.is_ascii_digit() || ch == '.' || (ch == '-' && current.is_empty()) {
            current.push(ch);
        } else {
            if let Ok(value) = current.parse::<f64>() {
                numbers.push(value);
            }
            current.clear();
        }
    }
    if let Ok(value) = current.parse::<f64>() {
        numbers.push(value);
    }
    if numbers.len() < 2 {
        return None;
    }

    let (mut min_x, mut max_x) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut min_y, mut max_y) = (f64::INFINITY, f64::NEG_INFINITY);
    for pair in numbers.chunks_exact(2) {
        min_x = min_x.min(pair[0]);
        max_x = max_x.max(pair[0]);
        min_y = min_y.min(pair[1]);
        max_y = max_y.max(pair[1]);
    }

    // Em units, y down from the baseline, scaled and placed.
    let origin_x = run.x + glyph.x;
    Some((
        origin_x + min_x * run.size,
        run.y + min_y * run.size,
        origin_x + max_x * run.size,
        run.y + max_y * run.size,
    ))
}

/// Shortest horizontal distance from each glyph's ink to the silhouette.
///
/// Only glyphs whose rows the shape actually occupies are measured: a line
/// well above or below the picture has nothing to be close to.
fn clearances(engine: &Engine, list: &DisplayList, ring: &[[f64; 2]]) -> Vec<f64> {
    let mut out = Vec::new();

    for run in runs(list) {
        for index in 0..run.glyphs.len() {
            let Some((left, top, right, bottom)) = ink_box(engine, &run, index) else {
                continue;
            };

            let mut nearest = f64::INFINITY;
            // Sample the rows this glyph's ink really covers.
            let mut row = top;
            while row <= bottom {
                for (a, b) in crossings(ring, row) {
                    // Ink to the left of the shape, or to its right.
                    if right <= a {
                        nearest = nearest.min(a - right);
                    } else if left >= b {
                        nearest = nearest.min(left - b);
                    } else {
                        // Overlapping: the ink is inside the silhouette.
                        nearest = nearest.min(-(right.min(b) - left.max(a)));
                    }
                }
                row += 0.5;
            }

            if nearest.is_finite() {
                out.push(nearest);
            }
        }
    }

    out
}

/// Pairs of x where the ring crosses the horizontal line `y`.
fn crossings(points: &[[f64; 2]], y: f64) -> Vec<(f64, f64)> {
    let mut xs: Vec<f64> = Vec::new();
    let mut a = *points.last().expect("anel não vazio");
    for b in points {
        if (a[1] <= y && y < b[1]) || (b[1] <= y && y < a[1]) {
            xs.push(a[0] + (y - a[1]) * (b[0] - a[0]) / (b[1] - a[1]));
        }
        a = *b;
    }
    xs.sort_by(f64::total_cmp);
    xs.chunks_exact(2).map(|p| (p[0], p[1])).collect()
}
