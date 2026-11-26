//! Braille plots with real axes. When an SVG plot carries its tick labels
//! as `<text>` (Giac does), the plot area is drawn in Braille inside a
//! box-drawing frame, with tick marks where the labels are and the numbers
//! as text, instead of numbers smeared into dots.

use image::DynamicImage;
use ratatui::text::{Line, Span};
use resvg::usvg;
use unicode_width::UnicodeWidthStr;

/// A text element, centred at (x, y) in canvas pixels, `h` high.
#[derive(Clone, Debug)]
struct Label {
    text: String,
    x: f32,
    y: f32,
    h: f32,
}

/// Where everything goes, worked out without rasterising (the layout pass
/// needs only the height).
#[derive(Debug)]
pub struct Plan {
    /// The plot area in canvas pixels: left, top, right, bottom.
    plot: [f32; 4],
    /// The x labels, left to right, and the y labels, top to bottom.
    xs: Vec<Label>,
    ys: Vec<Label>,
    cols: u16,
    rows: u16,
    gutter: u16,
}

impl Plan {
    /// Lines drawn: top border, the plot rows, bottom border, x labels.
    pub fn height(&self) -> usize {
        self.rows as usize + 3
    }
}

/// Plan an SVG plot drawn in a body `width` cells wide and at most
/// `max_rows` high (unless `full`). None when it has no axis labels as text:
/// then it is drawn as plain Braille.
pub fn plan(svg: &str, width: u16, max_rows: u16, full: bool) -> Option<Plan> {
    if !svg.contains("<text") {
        return None;
    }
    let opt = usvg::Options { fontdb: crate::latex::system_fonts(), ..Default::default() };
    let tree = usvg::Tree::from_str(svg, &opt).ok()?;
    let (cw, ch) = (tree.size().width(), tree.size().height());
    let (mut labels, mut plot) = (Vec::new(), None);
    walk(tree.root(), (cw, ch), &mut labels, &mut plot);
    let [pl, pt, pr, pb] = plot?;
    if pr - pl < 8.0 || pb - pt < 8.0 {
        return None;
    }
    // x labels: one line of them under the plot area
    let below: Vec<Label> =
        labels.iter().filter(|l| l.y > pb && l.x >= pl - l.h && l.x <= pr + l.h).cloned().collect();
    let mut xl = biggest_group(below, |l| l.y);
    // y labels: one column of them beside it
    let beside: Vec<Label> = labels
        .iter()
        .filter(|l| (l.x < pl || l.x > pr) && l.y >= pt - l.h && l.y <= pb + l.h)
        .cloned()
        .collect();
    let mut yl = biggest_group(beside, |l| l.x);
    if xl.len() < 2 && yl.len() < 2 {
        return None;
    }
    if xl.len() < 2 {
        xl.clear();
    }
    if yl.len() < 2 {
        yl.clear();
    }
    let gutter = yl.iter().map(|l| l.text.width()).max().unwrap_or(0) as u16;
    // the frame and the gutter take room from the Braille area
    let (cols, rows) = crate::braille::size(
        ((pr - pl).round().max(1.0) as u32, (pb - pt).round().max(1.0) as u32),
        width.saturating_sub(gutter + 2),
        max_rows.saturating_sub(3).max(2),
        full,
    );
    xl.sort_by(|a, b| a.x.total_cmp(&b.x));
    yl.sort_by(|a, b| a.y.total_cmp(&b.y));
    Some(Plan { plot: [pl, pt, pr, pb], xs: xl, ys: yl, cols, rows, gutter })
}

/// Draw a planned plot: `plan.height()` lines.
pub fn draw(plan: &Plan, svg: &str) -> Option<Vec<Line<'static>>> {
    let img = crate::latex::render_svg(&strip_text(svg))?;
    let [pl, pt, pr, pb] = plan.plot;
    // just inside the plot area: its own frame line is replaced by ours
    let inset = ((pr - pl).min(pb - pt) * 0.01).max(2.0);
    let x0 = (pl + inset).max(0.0) as u32;
    let y0 = (pt + inset).max(0.0) as u32;
    let x1 = ((pr - inset) as u32).min(img.width());
    let y1 = ((pb - inset) as u32).min(img.height());
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let area = image::imageops::crop_imm(&img, x0, y0, x1 - x0, y1 - y0).to_image();
    // ticks: on the grid or axis line next to each number when there is one
    // (Giac sets its y numbers just above their lines)
    let bg = *area.get_pixel(0, 0);
    let (fw, fh) = ((x1 - x0) as f32, (y1 - y0) as f32);
    let ys: Vec<(u16, &str)> = dedup(plan.ys.iter().map(|l| {
        let y = snap(l.y, l.h, (y0, y1), |y| line_ink(&area, bg, |i| (i, y - y0), x1 - x0));
        let r = ((y - y0 as f32) / fh * plan.rows as f32).floor();
        (r.clamp(0.0, plan.rows as f32 - 1.0) as u16, l.text.as_str())
    }));
    let xs: Vec<(u16, String)> = dedup(plan.xs.iter().map(|l| {
        let x = snap(l.x, l.h, (x0, x1), |x| line_ink(&area, bg, |i| (x - x0, i), y1 - y0));
        let c = ((x - x0 as f32) / fw * plan.cols as f32).floor();
        (c.clamp(0.0, plan.cols as f32 - 1.0) as u16, l.text.clone())
    }));
    let area = crate::recolor::apply(DynamicImage::ImageRgba8(area));
    let body = crate::braille::render(&area, plan.cols, plan.rows);

    let g = plan.gutter as usize;
    let cols = plan.cols as usize;
    let mut out = Vec::with_capacity(plan.height());
    out.push(Line::raw(format!("{:g$} ┌{}┐", "", "─".repeat(cols))));
    for (r, line) in body.into_iter().enumerate() {
        let label = ys.iter().find(|(row, _)| *row as usize == r).map(|(_, t)| *t);
        let mut spans = vec![Span::raw(format!(
            "{:>g$} {}",
            label.unwrap_or(""),
            if label.is_some() { '┤' } else { '│' }
        ))];
        spans.extend(line.spans);
        spans.push(Span::raw("│"));
        out.push(Line::from(spans));
    }
    let bottom: String = (0..cols)
        .map(|c| if xs.iter().any(|(col, _)| *col as usize == c) { '┬' } else { '─' })
        .collect();
    out.push(Line::raw(format!("{:g$} └{bottom}┘", "")));
    out.push(Line::raw(x_labels(&xs, g + 2, g + 2 + cols + 1)));
    Some(out)
}

/// One tick per row or column: the first label wins.
fn dedup<T>(ticks: impl Iterator<Item = (u16, T)>) -> Vec<(u16, T)> {
    let mut out: Vec<(u16, T)> = Vec::new();
    for t in ticks {
        if !out.iter().any(|o| o.0 == t.0) {
            out.push(t);
        }
    }
    out
}

/// The pixel line within `h` of `at` (inside `range`) that is most fully
/// inked, if one is at least half inked; else `at`.
fn snap(at: f32, h: f32, range: (u32, u32), ink: impl Fn(u32) -> f32) -> f32 {
    let lo = (at - h).max(range.0 as f32) as u32;
    let hi = ((at + h) as u32).min(range.1.saturating_sub(1));
    let mut best: Option<(f32, u32)> = None;
    for p in lo..=hi {
        let f = ink(p);
        let closer = |b: u32| (p as f32 - at).abs() < (b as f32 - at).abs();
        if f >= 0.5 && best.is_none_or(|(bf, bp)| f > bf + 0.05 || (f > bf - 0.05 && closer(bp))) {
            best = Some((f, p));
        }
    }
    best.map(|(_, p)| p as f32 + 0.5).unwrap_or(at)
}

/// The share of `n` pixels (`at(i)` gives each one's position in `img`)
/// that differ from the background `bg`, faint anti-aliased ones included
/// (a thin grid line can be too faint to show as dots, but still marks
/// where its tick goes).
fn line_ink(
    img: &image::RgbaImage,
    bg: image::Rgba<u8>,
    at: impl Fn(u32) -> (u32, u32),
    n: u32,
) -> f32 {
    if n == 0 {
        return 0.0;
    }
    let inked = (0..n)
        .filter(|&i| {
            let (x, y) = at(i);
            let Some(p) = img.get_pixel_checked(x, y) else { return false };
            let d: i32 = (0..3).map(|k| (p[k] as i32 - bg[k] as i32).abs()).sum();
            d > 24
        })
        .count();
    inked as f32 / n as f32
}

/// The x labels centred under their ticks (`offset`: the Braille area's
/// first column), skipping any that would touch the one before.
fn x_labels(xs: &[(u16, String)], offset: usize, width: usize) -> String {
    let mut row: Vec<char> = vec![' '; width + 8];
    let mut free = 0;
    for (col, text) in xs {
        let w = text.width();
        let start = (offset + *col as usize).saturating_sub(w / 2);
        if start < free || start + w > row.len() {
            continue;
        }
        for (i, c) in text.chars().enumerate() {
            row[start + i] = c;
        }
        free = start + w + 1;
    }
    row.into_iter().collect::<String>().trim_end().to_string()
}

/// Text elements, and the union of the drawing's other shapes (without a
/// background that fills the whole canvas): the plot area.
fn walk(g: &usvg::Group, canvas: (f32, f32), labels: &mut Vec<Label>, plot: &mut Option<[f32; 4]>) {
    for node in g.children() {
        match node {
            usvg::Node::Group(g) => walk(g, canvas, labels, plot),
            usvg::Node::Text(t) => {
                let text: String = t.chunks().iter().map(|c| c.text()).collect();
                let text = text.trim();
                let b = t.abs_bounding_box();
                if !text.is_empty() && b.height() > 0.0 {
                    labels.push(Label {
                        text: text.to_string(),
                        x: b.x() + b.width() / 2.0,
                        y: b.y() + b.height() / 2.0,
                        h: b.height(),
                    });
                }
            }
            usvg::Node::Path(p) => {
                let b = p.abs_bounding_box();
                if b.width() >= 0.95 * canvas.0 && b.height() >= 0.95 * canvas.1 {
                    continue; // the background
                }
                let r = [b.left(), b.top(), b.right(), b.bottom()];
                *plot = Some(match *plot {
                    None => r,
                    Some(p) => [p[0].min(r[0]), p[1].min(r[1]), p[2].max(r[2]), p[3].max(r[3])],
                });
            }
            usvg::Node::Image(_) => {}
        }
    }
}

/// The largest set of labels lined up on `key` (within a label's height).
fn biggest_group(mut labels: Vec<Label>, key: impl Fn(&Label) -> f32) -> Vec<Label> {
    labels.sort_by(|a, b| key(a).total_cmp(&key(b)));
    let mut best: Vec<Label> = Vec::new();
    let mut i = 0;
    while i < labels.len() {
        let tol = labels[i].h.max(1.0);
        let k0 = key(&labels[i]);
        let group: Vec<Label> =
            labels[i..].iter().take_while(|l| key(l) - k0 <= tol).cloned().collect();
        let n = group.len();
        if n > best.len() {
            best = group;
        }
        i += n.max(1);
    }
    best
}

/// The SVG without its `<text>` elements.
fn strip_text(svg: &str) -> String {
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(i) = rest.find("<text") {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let open_end = tail.find('>').map(|j| j + 1).unwrap_or(tail.len());
        rest = if tail[..open_end].ends_with("/>") {
            &tail[open_end..]
        } else {
            match tail.find("</text>") {
                Some(j) => &tail[j + "</text>".len()..],
                None => "",
            }
        };
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Giac's plot of x^2+2x+1 for x in -4..2, trimmed: grid, frame, curve
    /// and its tick labels as text.
    const GIAC: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" version="1.2" baseProfile="tiny" width="15.6cm" height="9.6cm" preserveAspectRatio="none" viewBox="-4.96 -1.9845 7.92 12.474" >
<rect x="-4.96" y="-1.9845" width="7.92" height="12.474" fill="white" />
<g transform="translate(0,8.505) scale(1,-1)">
<line x1="0" y1="-0.945" x2="0" y2="9.45" stroke="blue" stroke-width="0.011" />
<line x1="-4.3" y1="0" x2="2.3" y2="0" stroke="blue" stroke-width="0.011" />
<rect stroke="black" stroke-width="0.011" fill="none" x="-4.3" y="-0.945" width="6.6" height="10.395" />
<path stroke-width="0.0322" stroke="rgb(0,4,192)" fill="none" d="M-4,9 L-3,4 L-2,1 L-1,0 L0,1 L1,4 L2,9" />
</g>
<text fill="black" x="-10.101" y="16.152" transform="scale(0.396,0.6237)" style="font-size:0.67pt; text-anchor:middle;">-4</text>
<text fill="black" x="-5.0505" y="16.152" transform="scale(0.396,0.6237)" style="font-size:0.67pt; text-anchor:middle;">-2</text>
<text fill="black" x="0" y="16.152" transform="scale(0.396,0.6237)" style="font-size:0.67pt; text-anchor:middle;">0</text>
<text fill="black" x="5.0505" y="16.152" transform="scale(0.396,0.6237)" style="font-size:0.67pt; text-anchor:middle;">2</text>
<text fill="black" x="7.27475" y="13.803" transform="scale(0.396,0.6237)" style="font-size:0.67pt; text-anchor:end;">0</text>
<text fill="black" x="7.27475" y="7.3897" transform="scale(0.396,0.6237)" style="font-size:0.67pt; text-anchor:end;">4</text>
<text fill="black" x="7.27475" y="0.97635" transform="scale(0.396,0.6237)" style="font-size:0.67pt; text-anchor:end;">8</text>
</svg>"#;

    fn text(lines: &[Line]) -> Vec<String> {
        lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect()).collect()
    }

    #[test]
    fn giac_plot_gets_a_frame_ticks_and_numbers() {
        let plan = plan(GIAC, 60, 20, false).expect("labels found");
        assert_eq!(
            plan.xs.iter().map(|x| x.text.as_str()).collect::<Vec<_>>(),
            ["-4", "-2", "0", "2"]
        );
        assert_eq!(plan.ys.iter().map(|y| y.text.as_str()).collect::<Vec<_>>(), ["8", "4", "0"]);
        let lines = draw(&plan, GIAC).unwrap();
        assert_eq!(lines.len(), plan.height());
        let t = text(&lines);
        for l in &t {
            eprintln!("{l}");
        }
        assert!(t[0].trim_start().starts_with('┌') && t[0].ends_with('┐'));
        let last = t.len() - 1;
        assert!(t[last - 1].contains('└') && t[last - 1].contains('┬'));
        for n in ["-4", "-2", "0", "2"] {
            assert!(t[last].split_whitespace().any(|w| w == n), "{n} in {:?}", t[last]);
        }
        // y numbers sit at their ticks, highest first
        let ticked: Vec<&str> =
            t.iter().filter(|l| l.contains('┤')).map(|l| l.trim_start()).collect();
        assert_eq!(ticked.len(), 3);
        assert!(ticked[0].starts_with("8 ┤") && ticked[2].starts_with("0 ┤"), "{ticked:?}");
        // the curve is still there, in Braille
        assert!(t.iter().any(|l| l.chars().any(|c| ('\u{2801}'..='\u{28ff}').contains(&c))));
        // and every row is as wide as the frame
        let w = t[0].width();
        assert!(t[1..last].iter().all(|l| l.width() == w), "{t:?}");
    }

    #[test]
    fn without_text_labels_no_plan() {
        let svg = GIAC.replace("<text", "<desc").replace("</text>", "</desc>");
        assert!(plan(&svg, 60, 20, false).is_none());
    }

    #[test]
    fn strip_text_keeps_the_rest() {
        let s = r#"<svg><text x="1">a</text><line/><text/><g/></svg>"#;
        assert_eq!(strip_text(s), "<svg><line/><g/></svg>");
    }
}
