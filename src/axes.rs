//! Braille plots with real axes. When an SVG plot carries its text as
//! `<text>` (Giac does; matplotlib, R's svglite and Octave can), the plot
//! area is drawn in Braille inside a box-drawing frame, with tick marks where
//! the numbers are and the numbers as text, instead of numbers smeared into
//! dots. Titles and axis names go above and below the frame, legends and
//! other text inside the plot are written over the Braille.

use image::{DynamicImage, Rgba, RgbaImage};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use resvg::usvg;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// A text element: centre (x, y), width and height, in canvas pixels.
#[derive(Clone, Debug)]
struct Label {
    text: String,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Label {
    /// Across the text's lines: its height, or its width when it is turned
    /// on its side (R sets y numbers and axis names that way).
    fn thickness(&self) -> f32 {
        self.w.min(self.h).max(1.0)
    }
}

/// Silent code that makes a kernel send its plots as SVG with the text as
/// `<text>`, run at kernel start when plots are Braille. Giac does already;
/// R needs the svglite package (R's own SVG draws letters as shapes).
pub fn kernel_setup(language: &str) -> Option<&'static str> {
    match language.to_ascii_lowercase().as_str() {
        "python" => Some(PYTHON_SETUP),
        "r" => Some(R_SETUP),
        "octave" => Some("%plot -f svg"),
        _ => None,
    }
}

const PYTHON_SETUP: &str = r#"try:
    _jotter_ip = get_ipython()
    _jotter_rc = dict(_jotter_ip.config.InlineBackend.get("rc", {}) or {})
    _jotter_rc["svg.fonttype"] = "none"
    _jotter_ip.run_line_magic("config", "InlineBackend.figure_formats = ['svg']")
    _jotter_ip.run_line_magic("config", f"InlineBackend.rc = {_jotter_rc!r}")
    del _jotter_ip, _jotter_rc
except Exception:
    pass"#;

const R_SETUP: &str = r#"if (requireNamespace("svglite", quietly = TRUE) && requireNamespace("repr", quietly = TRUE)) local({
  options(jupyter.plot_mimetypes = c("text/plain", "image/svg+xml"))
  registerS3method("repr_svg", "recordedplot", function(obj, width = getOption("repr.plot.width"),
                   height = getOption("repr.plot.height"), ...) {
    tf <- tempfile(fileext = ".svg")
    on.exit(unlink(tf))
    svglite::svglite(tf, width, height)
    replayPlot(obj)
    dev.off()
    readChar(tf, file.info(tf)$size, useBytes = TRUE)
  }, envir = asNamespace("repr"))
})"#;

/// Where everything goes, worked out without rasterising (the layout pass
/// needs only the height).
#[derive(Debug)]
pub struct Plan {
    /// The plot area in canvas pixels: left, top, right, bottom.
    plot: [f32; 4],
    /// The numbers under the plot, left to right, and beside it, top to
    /// bottom.
    xs: Vec<Label>,
    ys: Vec<Label>,
    /// Text inside the plot area (legends, annotations).
    inside: Vec<Label>,
    /// Lines above the frame (titles; the y axis name at the left) and
    /// below the numbers (the x axis name).
    above: Vec<Line<'static>>,
    below: Vec<Line<'static>>,
    cols: u16,
    rows: u16,
    gutter: u16,
}

impl Plan {
    pub fn height(&self) -> usize {
        self.above.len()
            + 2
            + self.rows as usize
            + usize::from(!self.xs.is_empty())
            + self.below.len()
    }
}

/// Plan an SVG plot drawn in a body `width` cells wide and at most
/// `max_rows` high (unless `full`). None when it has no axis numbers as
/// text: then it is drawn as plain Braille.
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
    let under = |l: &Label| l.y > pb && l.x >= pl - l.thickness() && l.x <= pr + l.thickness();
    let beside = |l: &Label| {
        (l.x < pl || l.x > pr) && l.y >= pt - l.thickness() && l.y <= pb + l.thickness()
    };
    // the numbers: the longest row of text under the plot, the longest
    // column beside it
    let xs = biggest_group(labels.iter().filter(|l| under(l)).cloned().collect(), |l| l.y);
    let ys = biggest_group(labels.iter().filter(|l| beside(l)).cloned().collect(), |l| l.x);
    let (mut xs, mut ys) =
        (if xs.len() >= 2 { xs } else { Vec::new() }, if ys.len() >= 2 { ys } else { Vec::new() });
    if xs.is_empty() && ys.is_empty() {
        return None;
    }
    xs.sort_by(|a, b| a.x.total_cmp(&b.x));
    ys.sort_by(|a, b| a.y.total_cmp(&b.y));
    let taken =
        |l: &Label| xs.iter().chain(&ys).any(|t| t.text == l.text && t.x == l.x && t.y == l.y);
    let rest: Vec<Label> = labels.iter().filter(|l| !taken(l)).cloned().collect();
    let inside: Vec<Label> =
        rest.iter().filter(|l| l.x >= pl && l.x <= pr && l.y >= pt && l.y <= pb).cloned().collect();
    let titles: Vec<Label> = rest.iter().filter(|l| l.y < pt && !beside(l)).cloned().collect();
    let names_x: Vec<Label> = rest.iter().filter(|l| l.y > pb).cloned().collect();
    let names_y: Vec<Label> = rest.iter().filter(|l| beside(l)).cloned().collect();

    let gutter = ys.iter().map(|l| l.text.width()).max().unwrap_or(0) as u16;
    // the frame and the gutter take room from the Braille area
    let extra = |n: usize| n as u16;
    let fixed = 2 + u16::from(!xs.is_empty());
    let (cols, rows) = crate::braille::size(
        ((pr - pl).round().max(1.0) as u32, (pb - pt).round().max(1.0) as u32),
        width.saturating_sub(gutter + 2),
        max_rows.saturating_sub(fixed + extra(titles.len().min(2) + names_x.len().min(1))).max(2),
        full,
    );
    let (offset, span) = (gutter as usize + 2, cols as usize);
    let total = offset + span + 1;

    let mut above: Vec<Line<'static>> = text_rows(&titles)
        .into_iter()
        .map(|t| {
            Line::styled(
                centred(&t, offset, span, total),
                Style::new().add_modifier(Modifier::BOLD),
            )
        })
        .collect();
    let name_y = text_rows(&names_y).join(" ");
    if !name_y.is_empty() {
        // at the top left, over the numbers, or a line of its own
        let fits = above.first().is_some_and(|t| {
            let line: String = t.spans.iter().map(|s| s.content.as_ref()).collect();
            line.chars().take(name_y.width() + 1).all(|c| c == ' ')
        });
        if fits {
            let first: String = above[0].spans.iter().map(|s| s.content.as_ref()).collect();
            let skip = name_y.width();
            let tail: String = first.chars().skip(skip).collect();
            above[0] = Line::from(vec![
                Span::raw(name_y.clone()),
                Span::styled(tail, Style::new().add_modifier(Modifier::BOLD)),
            ]);
        } else {
            above.push(Line::raw(fit(&name_y, total)));
        }
    }
    let below: Vec<Line<'static>> = text_rows(&names_x)
        .into_iter()
        .take(1)
        .map(|t| Line::raw(centred(&t, offset, span, total)))
        .collect();
    Some(Plan { plot: [pl, pt, pr, pb], xs, ys, inside, above, below, cols, rows, gutter })
}

/// Draw a planned plot: `plan.height()` lines.
pub fn draw(plan: &Plan, svg: &str) -> Option<Vec<Line<'static>>> {
    let img = crate::latex::render_svg(&strip_text(svg))?;
    let bg = *img.get_pixel(0, 0);
    let [pl, pt, pr, pb] = plan.plot;
    // just inside the plot area: its own frame line is replaced by ours
    let inset = ((pr - pl).min(pb - pt) * 0.01).max(2.0);
    let mut x0 = (pl + inset).max(0.0) as u32;
    let mut y0 = (pt + inset).max(0.0) as u32;
    let mut x1 = ((pr - inset) as u32).min(img.width());
    let mut y1 = ((pb - inset) as u32).min(img.height());
    if x1 <= x0 + 4 || y1 <= y0 + 4 {
        return None;
    }
    // and inside the plot's own axis lines near the edges (matplotlib's
    // spines sit inside its tick marks)
    let (mx, my) = (((x1 - x0) / 25).max(3), ((y1 - y0) / 25).max(3));
    let col_ink = |x: u32, y0: u32, y1: u32| line_ink(&img, bg, |i| (x, y0 + i), y1 - y0);
    let row_ink = |y: u32, x0: u32, x1: u32| line_ink(&img, bg, |i| (x0 + i, y), x1 - x0);
    if let Some(k) = (0..mx).rev().find(|&k| col_ink(x0 + k, y0, y1) > 0.7) {
        x0 += k + 1;
    }
    if let Some(k) = (0..mx).rev().find(|&k| col_ink(x1 - 1 - k, y0, y1) > 0.7) {
        x1 -= k + 1;
    }
    if let Some(k) = (0..my).rev().find(|&k| row_ink(y0 + k, x0, x1) > 0.7) {
        y0 += k + 1;
    }
    if let Some(k) = (0..my).rev().find(|&k| row_ink(y1 - 1 - k, x0, x1) > 0.7) {
        y1 -= k + 1;
    }
    let area = image::imageops::crop_imm(&img, x0, y0, x1 - x0, y1 - y0).to_image();
    let (fw, fh) = ((x1 - x0) as f32, (y1 - y0) as f32);
    let (cols, rows) = (plan.cols as usize, plan.rows as usize);
    let col_of = |x: f32| {
        ((x - x0 as f32) / fw * cols as f32).floor().clamp(0.0, cols as f32 - 1.0) as usize
    };
    let row_of = |y: f32| {
        ((y - y0 as f32) / fh * rows as f32).floor().clamp(0.0, rows as f32 - 1.0) as usize
    };

    // ticks: on the grid or axis line next to each number when there is one
    // (Giac sets its y numbers just above their lines)
    let ys: Vec<(usize, &str)> = dedup(plan.ys.iter().map(|l| {
        let y = snap(l.y, l.thickness(), (y0, y1), |y| row_ink(y, x0, x1));
        (row_of(y), l.text.as_str())
    }));
    let xs: Vec<(usize, String)> = dedup(plan.xs.iter().map(|l| {
        let x = snap(l.x, l.thickness(), (x0, x1), |x| col_ink(x, y0, y1));
        (col_of(x), l.text.clone())
    }));

    let area = crate::recolor::apply(DynamicImage::ImageRgba8(area));
    let mut body: Vec<Vec<(char, Style)>> = crate::braille::render(&area, plan.cols, plan.rows)
        .into_iter()
        .map(|line| {
            line.spans
                .iter()
                .flat_map(|s| s.content.chars().map(move |c| (c, s.style)).collect::<Vec<_>>())
                .collect()
        })
        .collect();
    // legends and other text inside: written over the dots
    for l in &plan.inside {
        let row = row_of(l.y);
        let start = col_of(l.x - l.w / 2.0);
        let cells = &mut body[row];
        let mut c = start.min(cols.saturating_sub(l.text.width()));
        for ch in l.text.chars() {
            if c >= cells.len() {
                break;
            }
            cells[c] = (ch, Style::default());
            c += ch.width().unwrap_or(1).max(1);
        }
    }

    let g = plan.gutter as usize;
    let mut out = plan.above.clone();
    out.push(Line::raw(format!("{:g$} ┌{}┐", "", "─".repeat(cols))));
    for (r, cells) in body.into_iter().enumerate() {
        let label = ys.iter().find(|(row, _)| *row == r).map(|(_, t)| *t);
        let mut spans = vec![Span::raw(format!(
            "{:>g$} {}",
            label.unwrap_or(""),
            if label.is_some() { '┤' } else { '│' }
        ))];
        spans.extend(merge(cells));
        spans.push(Span::raw("│"));
        out.push(Line::from(spans));
    }
    let bottom: String =
        (0..cols).map(|c| if xs.iter().any(|(col, _)| *col == c) { '┬' } else { '─' }).collect();
    out.push(Line::raw(format!("{:g$} └{bottom}┘", "")));
    if !plan.xs.is_empty() {
        out.push(Line::raw(x_labels(&xs, g + 2, g + 2 + cols + 1)));
    }
    out.extend(plan.below.iter().cloned());
    Some(out)
}

/// Cells back into spans, one per run of the same style.
fn merge(cells: Vec<(char, Style)>) -> Vec<Span<'static>> {
    let mut out: Vec<Span<'static>> = Vec::new();
    for (c, style) in cells {
        match out.last_mut() {
            Some(s) if s.style == style => s.content.to_mut().push(c),
            _ => out.push(Span::styled(c.to_string(), style)),
        }
    }
    out
}

/// Labels as lines of text: those at about the same height together, left
/// to right, top to bottom.
fn text_rows(labels: &[Label]) -> Vec<String> {
    let mut ls = labels.to_vec();
    ls.sort_by(|a, b| a.y.total_cmp(&b.y));
    let mut rows: Vec<Vec<Label>> = Vec::new();
    for l in ls {
        match rows.last_mut() {
            Some(r) if (l.y - r[0].y).abs() <= r[0].thickness() / 2.0 => r.push(l),
            _ => rows.push(vec![l]),
        }
    }
    rows.into_iter()
        .map(|mut r| {
            r.sort_by(|a, b| a.x.total_cmp(&b.x));
            r.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("  ")
        })
        .collect()
}

/// `text` centred over the `span` columns from `offset`, in `total` columns.
fn centred(text: &str, offset: usize, span: usize, total: usize) -> String {
    let text = fit(text, total);
    let w = text.width();
    let start = (offset + span / 2).saturating_sub(w / 2).min(total.saturating_sub(w));
    format!("{:start$}{text}", "")
}

/// `s` cut to `max` columns.
fn fit(s: &str, max: usize) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if out.width() + c.width().unwrap_or(0) > max {
            break;
        }
        out.push(c);
    }
    out
}

/// One tick per row or column: the first label wins.
fn dedup<T>(ticks: impl Iterator<Item = (usize, T)>) -> Vec<(usize, T)> {
    let mut out: Vec<(usize, T)> = Vec::new();
    for t in ticks {
        if !out.iter().any(|o| o.0 == t.0) {
            out.push(t);
        }
    }
    out
}

/// The pixel line within `reach` of `at` (inside `range`) that is most
/// fully inked, if one is at least half inked; else `at`.
fn snap(at: f32, reach: f32, range: (u32, u32), ink: impl Fn(u32) -> f32) -> f32 {
    let lo = (at - reach).max(range.0 as f32) as u32;
    let hi = ((at + reach) as u32).min(range.1.saturating_sub(1));
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
fn line_ink(img: &RgbaImage, bg: Rgba<u8>, at: impl Fn(u32) -> (u32, u32), n: u32) -> f32 {
    if n == 0 {
        return 0.0;
    }
    let inked = (0..n)
        .filter(|&i| {
            let (x, y) = at(i);
            let Some(p) = img.get_pixel_checked(x, y) else { return false };
            let d: i32 = (0..4).map(|k| (p[k] as i32 - bg[k] as i32).abs()).sum();
            d > 24
        })
        .count();
    inked as f32 / n as f32
}

/// The x numbers centred under their ticks (`offset`: the Braille area's
/// first column), skipping any that would touch the one before.
fn x_labels(xs: &[(usize, String)], offset: usize, width: usize) -> String {
    let mut row: Vec<char> = vec![' '; width + 8];
    let mut free = 0;
    for (col, text) in xs {
        let w = text.width();
        let start = (offset + *col).saturating_sub(w / 2);
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
                        w: b.width(),
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

/// The largest set of labels lined up on `key` (within a label's
/// thickness).
fn biggest_group(mut labels: Vec<Label>, key: impl Fn(&Label) -> f32) -> Vec<Label> {
    labels.sort_by(|a, b| key(a).total_cmp(&key(b)));
    let mut best: Vec<Label> = Vec::new();
    let mut i = 0;
    while i < labels.len() {
        let tol = labels[i].thickness();
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

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/plots/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(path).unwrap()
    }

    fn render(name: &str) -> (Plan, Vec<String>) {
        let svg = fixture(name);
        let plan = plan(&svg, 78, 30, false).expect("axis numbers found");
        let lines = draw(&plan, &svg).unwrap();
        assert_eq!(lines.len(), plan.height(), "the layout pass's height");
        let text: Vec<String> =
            lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect()).collect();
        for l in &text {
            eprintln!("{l}");
        }
        (plan, text)
    }

    fn texts(ls: &[Label]) -> Vec<&str> {
        ls.iter().map(|l| l.text.as_str()).collect()
    }

    /// The frame, every row as wide as it, the curve in Braille, and the
    /// y numbers at their ticks in order.
    fn framed(t: &[String], ys: &[&str]) {
        let top = t.iter().position(|l| l.contains('┌')).unwrap();
        let bottom = t.iter().position(|l| l.contains('└')).unwrap();
        let w = t[top].width();
        assert!(t[top..=bottom].iter().all(|l| l.width() == w), "{t:?}");
        assert!(t[bottom].contains('┬'));
        assert!(
            t[top..bottom]
                .iter()
                .any(|l| l.chars().any(|c| ('\u{2801}'..='\u{28ff}').contains(&c)))
        );
        let ticked: Vec<&str> = t[top..bottom]
            .iter()
            .filter(|l| l.contains('┤'))
            .map(|l| l.split('┤').next().unwrap().trim())
            .collect();
        assert_eq!(ticked, ys);
    }

    fn line_with<'a>(t: &'a [String], word: &str) -> Option<&'a String> {
        t.iter().find(|l| l.split_whitespace().any(|w| w == word))
    }

    #[test]
    fn giac() {
        let (plan, t) = render("giac.svg");
        assert_eq!(texts(&plan.xs), ["-4", "-3", "-2", "-1", "0", "1", "2"]);
        framed(&t, &["8", "6", "4", "2", "0"]);
        assert!(line_with(&t, "-4").unwrap().contains("-3"));
    }

    #[test]
    fn matplotlib() {
        let (plan, t) = render("matplotlib.svg");
        assert_eq!(texts(&plan.xs), ["−4", "−3", "−2", "−1", "0", "1", "2"]);
        framed(&t, &["8", "6", "4", "2", "0"]);
        // title above, axis names, the legend inside the frame
        assert!(t[0].contains("Two curves"), "{t:?}");
        assert!(line_with(&t, "x").is_some_and(|l| !l.contains('┤')));
        assert!(line_with(&t, "y").is_some());
        for name in ["quadratic", "wave"] {
            assert!(t.iter().any(|l| l.contains('│') && l.contains(name)), "{name}");
        }
        // matplotlib's own axis lines are not drawn again inside ours
        let row = t.iter().find(|l| l.contains("4 ┤")).unwrap();
        let after = row.split('┤').nth(1).unwrap();
        assert!(!after.starts_with('⡇') && !after.starts_with('⣿'), "{row}");
    }

    #[test]
    fn r_svglite() {
        let (plan, t) = render("r-svglite.svg");
        assert_eq!(texts(&plan.xs), ["-4", "-3", "-2", "-1", "0", "1", "2"]);
        // R's y numbers and axis name are turned on their side: the name is
        // not taken for a number
        framed(&t, &["8", "6", "4", "2", "0"]);
        assert!(t[0].contains("Two curves"));
        assert!(t.iter().any(|l| l.contains("quadratic")));
    }

    #[test]
    fn octave() {
        let (plan, t) = render("octave.svg");
        assert_eq!(texts(&plan.xs), ["-4", "-3", "-2", "-1", "0", "1", "2"]);
        assert!(texts(&plan.ys).contains(&"0"));
        assert!(t.iter().take(2).any(|l| l.contains("Two curves")));
        assert!(t.iter().any(|l| l.contains("wave")));
    }

    #[test]
    fn kernels_asked_for_svg_text() {
        assert!(kernel_setup("python").unwrap().contains("svg.fonttype"));
        assert!(kernel_setup("R").unwrap().contains("svglite"));
        assert_eq!(kernel_setup("Octave"), Some("%plot -f svg"));
        // Giac sends text already; others are left alone
        assert_eq!(kernel_setup("giac"), None);
        assert_eq!(kernel_setup("julia"), None);
    }

    #[test]
    fn without_text_labels_no_plan() {
        let svg = fixture("giac.svg").replace("<text", "<desc").replace("</text>", "</desc>");
        assert!(plan(&svg, 60, 20, false).is_none());
    }

    #[test]
    fn strip_text_keeps_the_rest() {
        let s = r#"<svg><text x="1">a</text><line/><text/><g/></svg>"#;
        assert_eq!(strip_text(s), "<svg><line/><g/></svg>");
    }
}
