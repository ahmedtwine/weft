use std::collections::HashMap;
use std::fmt::Write;

use weft_parse::{Diagram, Verdict};

const FONT: f64 = 14.0;
const LINE: f64 = 18.0;
const MARGIN: f64 = 16.0;

const INK: &str = "#1b2230";
const RULE: &str = "#a3a9b4";
const BOX: (&str, &str) = ("#f3f4f6", "#b9bec8");
const NOTE: (&str, &str) = ("#fff6d6", "#dcc370");
const INIT: (&str, &str) = ("#e8f0fc", "#c7d7f2");
const FAIL: (&str, &str) = ("#fde4e1", "#e59a90");
const HOLD: (&str, &str) = ("#e2f4e6", "#8fcb9b");
const RED: &str = "#d93025";

const HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667,
    611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500,
    222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

fn units(c: char) -> u16 {
    match c {
        ' '..='~' => HELVETICA[c as usize - 32],
        '·' => 278,
        '…' => 1000,
        '✕' | '✓' => 800,
        _ => 556,
    }
}

fn pixels(s: &str) -> f64 {
    s.chars().map(|c| f64::from(units(c))).sum::<f64>() * FONT / 1000.0
}

fn chars(s: &str) -> f64 {
    s.chars().count() as f64
}

fn rows(s: &str) -> usize {
    s.split('\n').count()
}

struct Metrics {
    measure: fn(&str) -> f64,
    pad: f64,
    min_w: f64,
    gap: f64,
    label: f64,
    note: f64,
    clear: f64,
    verdict: f64,
}

const SVG: Metrics = Metrics {
    measure: pixels,
    pad: 14.0,
    min_w: 90.0,
    gap: 28.0,
    label: 48.0,
    note: 20.0,
    clear: 12.0,
    verdict: 40.0,
};
const ASCII: Metrics = Metrics {
    measure: chars,
    pad: 2.0,
    min_w: 7.0,
    gap: 3.0,
    label: 6.0,
    note: 2.0,
    clear: 1.0,
    verdict: 4.0,
};

impl Metrics {
    fn width(&self, s: &str) -> f64 {
        s.split('\n').map(self.measure).fold(0.0, f64::max)
    }

    fn note_w(&self, s: &str) -> f64 {
        self.width(s) + self.note
    }
}

struct Verdicts {
    label: String,
    failed: Option<usize>,
    lo: usize,
    hi: usize,
}

struct Layout<'a> {
    d: &'a Diagram,
    col: HashMap<&'a str, usize>,
    w: Vec<f64>,
    x: Vec<f64>,
    verdict: Option<Verdicts>,
}

impl<'a> Layout<'a> {
    fn new(d: &'a Diagram, m: &Metrics) -> Self {
        let n = d.objects.len();
        let col: HashMap<&str, usize> = d
            .objects
            .iter()
            .enumerate()
            .map(|(i, o)| (o.name.as_str(), i))
            .collect();
        let w: Vec<f64> = d
            .objects
            .iter()
            .map(|o| (m.width(&o.label) + 2.0 * m.pad).max(m.min_w))
            .collect();
        let verdict = d.check.as_ref().zip(d.verdict()).map(|(c, v)| {
            let mut span: Vec<usize> = c.objects().into_iter().map(|o| col[o]).collect();
            let failed = match v {
                Verdict::Fails(k) => {
                    span.extend([col[d.steps[k].from.as_str()], col[d.steps[k].to.as_str()]]);
                    Some(k)
                }
                Verdict::Holds => None,
            };
            let label = match failed {
                Some(k) => format!("✕ {} · fails at step {}", c.title, k + 1),
                None => format!("✓ {} · holds", c.title),
            };
            let (lo, hi) = (
                span.iter().copied().min().unwrap_or(0),
                span.iter().copied().max().unwrap_or(n - 1),
            );
            Verdicts {
                label,
                failed,
                lo,
                hi,
            }
        });

        let mut need: Vec<(usize, usize, f64)> = Vec::new();
        let note_need = |need: &mut Vec<(usize, usize, f64)>, i: usize, nw: f64| {
            if i > 0 {
                need.push((i - 1, i, nw / 2.0 + m.clear));
            }
            if i + 1 < n {
                need.push((i, i + 1, nw / 2.0 + m.clear));
            }
        };
        for s in &d.steps {
            let (a, b) = (col[s.from.as_str()], col[s.to.as_str()]);
            let tw = m.width(&s.text) + m.label;
            if a == b && a + 1 < n {
                need.push((a, a + 1, tw));
            } else if a != b {
                need.push((a.min(b), a.max(b), tw));
            }
            for set in &s.sets {
                note_need(&mut need, col[set.object.as_str()], m.note_w(&set.value));
            }
        }
        let mut init: Vec<(usize, f64)> = d
            .init
            .iter()
            .map(|a| (col[a.object.as_str()], m.note_w(&a.value)))
            .collect();
        init.sort_by_key(|p| p.0);
        for pair in init.windows(2) {
            need.push((
                pair[0].0,
                pair[1].0,
                pair[0].1 / 2.0 + pair[1].1 / 2.0 + 2.0 * m.clear,
            ));
        }
        for &(i, nw) in &init {
            note_need(&mut need, i, nw);
        }
        if let Some(v) = &verdict {
            need.push((
                v.lo,
                v.hi,
                m.width(&v.label) + m.verdict - w[v.lo] / 2.0 - w[v.hi] / 2.0,
            ));
        }
        need.retain(|p| p.0 != p.1);
        need.sort_by_key(|p| p.1 - p.0);

        let mut gap: Vec<f64> = (1..n)
            .map(|i| w[i - 1] / 2.0 + m.gap + w[i] / 2.0)
            .collect();
        for (a, b, dist) in need {
            let cur: f64 = gap[a..b].iter().sum();
            if cur < dist {
                let add = (dist - cur) / (b - a) as f64;
                gap[a..b].iter_mut().for_each(|g| *g += add);
            }
        }
        let mut x = vec![w[0] / 2.0];
        for g in &gap {
            x.push(x[x.len() - 1] + g);
        }
        Layout {
            d,
            col,
            w,
            x,
            verdict,
        }
    }

    fn at(&self, name: &str) -> f64 {
        self.x[self.col[name]]
    }
}

pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

struct Canvas {
    body: String,
    lo: f64,
    hi: f64,
}

impl Canvas {
    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64, (fill, stroke): (&str, &str)) {
        self.lo = self.lo.min(x);
        self.hi = self.hi.max(x + w);
        let _ = write!(
            self.body,
            r#"<rect x="{x:.1}" y="{y:.1}" width="{w:.1}" height="{h:.1}" rx="4" fill="{fill}" stroke="{stroke}"/>"#
        );
    }

    fn text(&mut self, x: f64, top: f64, s: &str, anchor: &str, weight: u16) {
        let w = SVG.width(s);
        let l = if anchor == "start" { x } else { x - w / 2.0 };
        self.lo = self.lo.min(l);
        self.hi = self.hi.max(l + w);
        for (k, line) in s.split('\n').enumerate() {
            let y = top + LINE * (k as f64 + 1.0) - 5.0;
            let _ = write!(
                self.body,
                r#"<text x="{x:.1}" y="{y:.1}" text-anchor="{anchor}" font-weight="{weight}" fill="{INK}">{}</text>"#,
                escape(line)
            );
        }
    }

    fn head(&mut self, x: f64, y: f64, dir: f64) {
        let b = x - dir * 9.0;
        let _ = write!(
            self.body,
            r#"<polygon points="{x:.1},{y:.1} {b:.1},{:.1} {b:.1},{:.1}" fill="{INK}"/>"#,
            y - 4.5,
            y + 4.5
        );
    }

    fn note(&mut self, x: f64, y: f64, s: &str) {
        let w = SVG.note_w(s);
        self.rect(x - w / 2.0, y, w, note_h(s), NOTE);
        self.text(x, y + 5.0, s, "middle", 400);
    }

    fn heads(&mut self, l: &Layout, top: f64, h: f64) {
        for (i, o) in l.d.objects.iter().enumerate() {
            self.rect(l.x[i] - l.w[i] / 2.0, top, l.w[i], h, BOX);
            self.text(
                l.x[i],
                top + (h - rows(&o.label) as f64 * LINE) / 2.0,
                &o.label,
                "middle",
                400,
            );
        }
    }
}

fn note_h(s: &str) -> f64 {
    rows(s) as f64 * LINE + 10.0
}

pub fn svg(d: &Diagram) -> String {
    let l = Layout::new(d, &SVG);
    let mut c = Canvas {
        body: String::new(),
        lo: f64::MAX,
        hi: f64::MIN,
    };
    let mut y = MARGIN;
    if !d.title.is_empty() {
        let _ = write!(
            c.body,
            r#"<text x="0" y="{:.1}" font-size="16" font-weight="700" fill="{INK}">{}</text>"#,
            y + 14.0,
            escape(&d.title)
        );
        y += 32.0;
    }
    let head_top = y;
    let head_h = d
        .objects
        .iter()
        .map(|o| rows(&o.label) as f64 * LINE + 22.0)
        .fold(44.0, f64::max);
    c.heads(&l, head_top, head_h);
    y += head_h + 18.0;

    if !d.init.is_empty() {
        let nh = d.init.iter().map(|a| note_h(&a.value)).fold(0.0, f64::max);
        let half = |a: &weft_parse::Assign| SVG.note_w(&a.value) / 2.0;
        let left = d
            .init
            .iter()
            .map(|a| l.at(&a.object) - half(a))
            .fold(f64::MAX, f64::min);
        let right = d
            .init
            .iter()
            .map(|a| l.at(&a.object) + half(a))
            .fold(f64::MIN, f64::max);
        c.rect(left - 10.0, y, right - left + 20.0, nh + 16.0, INIT);
        for a in &d.init {
            c.note(l.at(&a.object), y + 8.0, &a.value);
        }
        y += nh + 30.0;
    }

    for (k, s) in d.steps.iter().enumerate() {
        let (a, b) = (l.at(&s.from), l.at(&s.to));
        let arrow_y = y + rows(&s.text) as f64 * LINE + 6.0;
        if a == b {
            c.text(a + 14.0, y, &s.text, "start", 400);
            let _ = write!(
                c.body,
                r#"<path d="M{a:.1},{arrow_y:.1} h30 v18 h-28" fill="none" stroke="{INK}" stroke-width="1.2"/>"#
            );
            c.head(a, arrow_y + 18.0, -1.0);
            y = arrow_y + 34.0;
        } else {
            c.text((a + b) / 2.0, y, &s.text, "middle", 400);
            let _ = write!(
                c.body,
                r#"<line x1="{a:.1}" y1="{arrow_y:.1}" x2="{b:.1}" y2="{arrow_y:.1}" stroke="{INK}" stroke-width="1.2"/>"#
            );
            c.head(b, arrow_y, (b - a).signum());
            y = arrow_y + 16.0;
        }
        let color = if l.verdict.as_ref().and_then(|v| v.failed) == Some(k) {
            RED
        } else {
            INK
        };
        let _ = write!(
            c.body,
            r##"<circle cx="{a:.1}" cy="{arrow_y:.1}" r="9" fill="{color}"/><text x="{a:.1}" y="{:.1}" text-anchor="middle" font-size="11" font-weight="700" fill="#fff">{}</text>"##,
            arrow_y + 4.0,
            k + 1
        );
        for set in &s.sets {
            c.note(l.at(&set.object), y, &set.value);
            y += note_h(&set.value) + 10.0;
        }
    }

    if let Some(v) = &l.verdict {
        y += 4.0;
        let (left, right) = (l.x[v.lo] - l.w[v.lo] / 2.0, l.x[v.hi] + l.w[v.hi] / 2.0);
        let bw = (right - left).max(SVG.width(&v.label) + SVG.verdict);
        c.rect(
            (left + right - bw) / 2.0,
            y,
            bw,
            LINE + 20.0,
            if v.failed.is_some() { FAIL } else { HOLD },
        );
        c.text((left + right) / 2.0, y + 10.0, &v.label, "middle", 600);
        y += LINE + 38.0;
    }

    c.heads(&l, y, head_h);
    let lines: String = l
        .x
        .iter()
        .map(|xi| {
            format!(
                r#"<line x1="{xi:.1}" y1="{:.1}" x2="{xi:.1}" y2="{y:.1}" stroke="{RULE}" stroke-width="1"/>"#,
                head_top + head_h
            )
        })
        .collect();
    let (lo, hi) = (c.lo - MARGIN, c.hi + MARGIN);
    let (vw, vh) = (hi - lo, y + head_h + MARGIN);
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{lo:.1} 0 {vw:.1} {vh:.1}" width="{vw:.0}" height="{vh:.0}" font-family="Helvetica, Arial, sans-serif" font-size="{FONT}"><rect x="{lo:.1}" y="0" width="{vw:.1}" height="{vh:.1}" fill="#ffffff"/>{lines}{}</svg>"##,
        c.body
    )
}

struct Grid {
    rows: Vec<Vec<char>>,
    x: Vec<usize>,
}

impl Grid {
    fn row(&mut self) -> usize {
        let width = self.rows.first().map_or(0, Vec::len);
        let mut r = vec![' '; width];
        for &xi in &self.x {
            r[xi] = '│';
        }
        self.rows.push(r);
        self.rows.len() - 1
    }

    fn put(&mut self, r: usize, at: isize, s: &str) {
        for (k, ch) in s.chars().enumerate() {
            let c = at + k as isize;
            if c >= 0 && (c as usize) < self.rows[r].len() {
                self.rows[r][c as usize] = ch;
            }
        }
    }

    fn center(&mut self, r: usize, x: f64, s: &str) {
        self.put(r, (x - chars(s) / 2.0).round() as isize, s);
    }

    fn lines(&mut self, x: f64, s: &str, centered: bool) {
        for line in s.split('\n') {
            let r = self.row();
            if centered {
                self.center(r, x, line)
            } else {
                self.put(r, x as isize, line)
            }
        }
    }
}

pub fn ascii(d: &Diagram) -> String {
    let l = Layout::new(d, &ASCII);
    let shift = d
        .init
        .iter()
        .map(|a| ASCII.note_w(&a.value) / 2.0 - l.at(&a.object))
        .chain(
            d.steps
                .iter()
                .flat_map(|s| s.sets.iter())
                .map(|a| ASCII.note_w(&a.value) / 2.0 - l.at(&a.object)),
        )
        .chain(
            l.verdict
                .iter()
                .map(|v| (ASCII.width(&v.label) + 4.0) / 2.0 - (l.x[v.lo] + l.x[v.hi]) / 2.0),
        )
        .fold(2.0, f64::max);
    let x: Vec<f64> = l.x.iter().map(|xi| (xi + shift).round()).collect();
    let right = d
        .steps
        .iter()
        .map(|s| x[l.col[s.from.as_str()]] + chars(&s.text) + 6.0);
    let width = x
        .iter()
        .zip(&l.w)
        .map(|(xi, wi)| xi + wi / 2.0)
        .chain(right)
        .fold(0.0, f64::max) as usize
        + shift as usize
        + 2;
    let mut g = Grid {
        rows: vec![vec![' '; width]],
        x: Vec::new(),
    };
    g.put(0, 1, &d.title);
    g.row();
    let head_rows = d.objects.iter().map(|o| rows(&o.label)).max().unwrap_or(1);
    let top = g.rows.len();
    for _ in 0..head_rows + 2 {
        g.row();
    }
    for (i, o) in d.objects.iter().enumerate() {
        let (left, w) = ((x[i] - l.w[i] / 2.0).round() as isize, l.w[i] as usize);
        let rule = "─".repeat(w.saturating_sub(2));
        g.put(top, left, &format!("┌{rule}┐"));
        for r in 0..head_rows {
            g.put(
                top + 1 + r,
                left,
                &format!("│{}│", " ".repeat(w.saturating_sub(2))),
            );
        }
        for (r, line) in o.label.split('\n').enumerate() {
            g.center(top + 1 + r, x[i], line);
        }
        g.put(top + head_rows + 1, left, &format!("└{rule}┘"));
        g.put(top + head_rows + 1, x[i] as isize, "┬");
    }
    g.x = x.iter().map(|&xi| xi as usize).collect();
    g.row();
    if !d.init.is_empty() {
        let depth = d.init.iter().map(|a| rows(&a.value)).max().unwrap_or(1);
        let first = g.rows.len();
        for _ in 0..depth {
            g.row();
        }
        for a in &d.init {
            for (r, line) in a.value.split('\n').enumerate() {
                g.center(first + r, x[l.col[a.object.as_str()]], &format!("[{line}]"));
            }
        }
        g.row();
    }
    for (k, s) in d.steps.iter().enumerate() {
        let (a, b) = (x[l.col[s.from.as_str()]], x[l.col[s.to.as_str()]]);
        let mark = if l.verdict.as_ref().and_then(|v| v.failed) == Some(k) {
            "✕"
        } else {
            ""
        };
        let label = format!("{}{mark}. {}", k + 1, s.text);
        if a == b {
            g.lines(a + 2.0, &label, false);
            let r = g.row();
            g.put(r, a as isize, "├─┐");
            let r = g.row();
            g.put(r, a as isize, "◀─┘");
        } else {
            g.lines((a + b) / 2.0, &label, true);
            let r = g.row();
            let (lo, hi) = (a.min(b) as isize, a.max(b) as isize);
            g.put(r, lo, &"─".repeat((hi - lo + 1) as usize));
            g.put(r, a as isize, "●");
            g.put(r, b as isize, if b > a { "▶" } else { "◀" });
        }
        for set in &s.sets {
            for line in set.value.split('\n') {
                let r = g.row();
                g.center(r, x[l.col[set.object.as_str()]], &format!("[{line}]"));
            }
        }
        g.row();
    }
    if let Some(v) = &l.verdict {
        let r = g.row();
        g.center(r, (x[v.lo] + x[v.hi]) / 2.0, &format!("  {}  ", v.label));
        g.row();
    }
    let mut out = String::new();
    for r in g.rows {
        out.push_str(r.into_iter().collect::<String>().trim_end());
        out.push('\n');
    }
    out
}
