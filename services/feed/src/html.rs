//! Article text (patch notes, Mojang news) from HTML into our own small block
//! format, the only form the launcher ever gets: headings, paragraphs, lists,
//! pictures and runs of text that may be bold, italic, code or a link.
//! Everything else in the HTML (scripts, styles, frames, forms, attributes) is
//! dropped here, so no HTML reaches the launcher at all.
//!
//! The same blocks go to DeepL as plain HTML (`to_html`) and come back through
//! `parse` again, so a translation is cleaned exactly like the original.

use scraper::{ElementRef, Html, Node};
use serde::{Deserialize, Serialize};

/// Lists inside lists deeper than this are flattened into their parent.
const MAX_DEPTH: usize = 6;
/// Blocks in one article; the rest is cut.
const MAX_BLOCKS: usize = 5000;
/// Characters of text in one article; the rest is cut.
const MAX_TEXT: usize = 400_000;
/// The longest link or picture address kept.
const MAX_URL: usize = 2000;
/// Elements inside elements deeper than this are read as plain text, without
/// recursion, so no page can exhaust the stack.
const MAX_NEST: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub text: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub b: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub i: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub code: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
}

fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Block {
    H { level: u8, c: Vec<Run> },
    P { c: Vec<Run> },
    Ul { items: Vec<Vec<Block>> },
    Ol { items: Vec<Vec<Block>> },
    Img { src: String, alt: String },
}

#[derive(Clone, Copy, Default)]
struct Style<'a> {
    b: bool,
    i: bool,
    code: bool,
    href: Option<&'a str>,
}

struct Walk {
    blocks: usize,
    text: usize,
    nest: usize,
    base: Option<url::Url>,
}

/// Cleans an article. `base` turns relative links and pictures into full
/// addresses (Mojang's `/v2/images/…`); without it they are dropped.
pub fn parse(html: &str, base: Option<&str>) -> Vec<Block> {
    let doc = Html::parse_fragment(html);
    let mut walk = Walk {
        blocks: 0,
        text: 0,
        nest: 0,
        base: base.and_then(|b| url::Url::parse(b).ok()),
    };
    let mut out = Vec::new();
    let mut runs = Vec::new();
    walk.children(doc.root_element(), 0, &mut out, &mut runs);
    walk.flush(&mut out, &mut runs);
    out
}

/// Elements whose content is never shown.
fn dropped(name: &str) -> bool {
    matches!(
        name,
        "script"
            | "style"
            | "iframe"
            | "object"
            | "embed"
            | "svg"
            | "math"
            | "form"
            | "input"
            | "button"
            | "select"
            | "textarea"
            | "noscript"
            | "template"
            | "head"
            | "title"
            | "video"
            | "audio"
            | "canvas"
            | "link"
            | "meta"
    )
}

fn heading(name: &str) -> Option<u8> {
    match name {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" | "h5" | "h6" => Some(4),
        _ => None,
    }
}

impl Walk {
    fn full(&self) -> bool {
        self.blocks >= MAX_BLOCKS || self.text >= MAX_TEXT
    }

    /// A link or picture address we keep: https only, no credentials, short.
    fn address(&self, raw: &str) -> Option<String> {
        let raw = raw.trim();
        if raw.is_empty() || raw.len() > MAX_URL {
            return None;
        }
        let url = match url::Url::parse(raw) {
            Ok(url) => url,
            Err(url::ParseError::RelativeUrlWithoutBase) => self.base.as_ref()?.join(raw).ok()?,
            Err(_) => return None,
        };
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return None;
        }
        let url = url.to_string();
        (url.len() <= MAX_URL).then_some(url)
    }

    fn push(&mut self, out: &mut Vec<Block>, block: Block) {
        if !self.full() {
            self.blocks += 1;
            out.push(block);
        }
    }

    fn flush(&mut self, out: &mut Vec<Block>, runs: &mut Vec<Run>) {
        let c = tidy(std::mem::take(runs));
        if !c.is_empty() {
            self.push(out, Block::P { c });
        }
    }

    fn children(
        &mut self,
        node: ElementRef<'_>,
        depth: usize,
        out: &mut Vec<Block>,
        runs: &mut Vec<Run>,
    ) {
        for child in node.children() {
            if self.full() {
                return;
            }
            match child.value() {
                Node::Text(text) => self.text_run(runs, text, Style::default()),
                Node::Element(_) => {
                    if let Some(element) = ElementRef::wrap(child) {
                        self.block(element, depth, out, runs);
                    }
                }
                _ => {}
            }
        }
    }

    fn block(
        &mut self,
        element: ElementRef<'_>,
        depth: usize,
        out: &mut Vec<Block>,
        runs: &mut Vec<Run>,
    ) {
        if dropped(element.value().name()) {
            return;
        }
        if self.nest >= MAX_NEST {
            self.flat_text(element, runs);
            return;
        }
        self.nest += 1;
        self.block_in(element, depth, out, runs);
        self.nest -= 1;
    }

    fn block_in(
        &mut self,
        element: ElementRef<'_>,
        depth: usize,
        out: &mut Vec<Block>,
        runs: &mut Vec<Run>,
    ) {
        let name = element.value().name();
        if let Some(level) = heading(name) {
            self.flush(out, runs);
            let mut c = Vec::new();
            self.inline(element, Style::default(), &mut c);
            let c = tidy(c);
            if !c.is_empty() {
                self.push(out, Block::H { level, c });
            }
            return;
        }
        match name {
            "ul" | "ol" if depth < MAX_DEPTH => {
                self.flush(out, runs);
                let mut items = Vec::new();
                for li in element.child_elements() {
                    if self.full() {
                        break;
                    }
                    let mut item = Vec::new();
                    let mut item_runs = Vec::new();
                    if li.value().name() == "li" {
                        self.children(li, depth + 1, &mut item, &mut item_runs);
                    } else {
                        self.block(li, depth + 1, &mut item, &mut item_runs);
                    }
                    self.flush(&mut item, &mut item_runs);
                    if !item.is_empty() {
                        items.push(item);
                    }
                }
                if !items.is_empty() {
                    self.push(
                        out,
                        if name == "ul" {
                            Block::Ul { items }
                        } else {
                            Block::Ol { items }
                        },
                    );
                }
            }
            "img" => {
                self.flush(out, runs);
                let src = element.value().attr("src").and_then(|s| self.address(s));
                if let Some(src) = src {
                    let alt = clean(element.value().attr("alt").unwrap_or(""), 300);
                    self.push(out, Block::Img { src, alt });
                }
            }
            "br" => runs.push(Run::plain(" ")),
            "hr" => self.flush(out, runs),
            "p" | "div" | "section" | "article" | "blockquote" | "li" | "ul" | "ol" | "table"
            | "thead" | "tbody" | "tr" | "td" | "th" | "figure" | "figcaption" | "header"
            | "footer" | "main" | "aside" | "nav" | "dl" | "dt" | "dd" | "pre" | "details"
            | "summary" | "center" | "body" | "html" => {
                self.flush(out, runs);
                self.children(element, depth, out, runs);
                self.flush(out, runs);
            }
            _ => self.inline(element, Style::default(), runs),
        }
    }

    fn inline<'a>(&mut self, element: ElementRef<'a>, style: Style<'a>, runs: &mut Vec<Run>) {
        if dropped(element.value().name()) || self.full() {
            return;
        }
        if self.nest >= MAX_NEST {
            self.flat_text(element, runs);
            return;
        }
        self.nest += 1;
        self.inline_in(element, style, runs);
        self.nest -= 1;
    }

    /// The text of an element nested too deep, in a loop rather than by
    /// recursion; dropped elements stay dropped.
    fn flat_text(&mut self, element: ElementRef<'_>, runs: &mut Vec<Run>) {
        let mut skipped = 0usize;
        for edge in element.traverse() {
            match edge {
                ego_tree::iter::Edge::Open(node) => match node.value() {
                    Node::Element(e) if dropped(e.name()) => skipped += 1,
                    Node::Text(text) if skipped == 0 => self.text_run(runs, text, Style::default()),
                    _ => {}
                },
                ego_tree::iter::Edge::Close(node) => {
                    if matches!(node.value(), Node::Element(e) if dropped(e.name())) {
                        skipped -= 1;
                    }
                }
            }
            if self.full() {
                return;
            }
        }
    }

    fn inline_in<'a>(&mut self, element: ElementRef<'a>, style: Style<'a>, runs: &mut Vec<Run>) {
        let name = element.value().name();
        let mut style = style;
        match name {
            "b" | "strong" => style.b = true,
            "i" | "em" | "cite" => style.i = true,
            "code" | "kbd" | "tt" | "samp" | "var" => style.code = true,
            "a" => style.href = element.value().attr("href"),
            "br" => {
                runs.push(Run::plain(" "));
                return;
            }
            "img" => return,
            _ => {}
        }
        for child in element.children() {
            match child.value() {
                Node::Text(text) => self.text_run(runs, text, style),
                Node::Element(_) => {
                    if let Some(inner) = ElementRef::wrap(child) {
                        self.inline(inner, style, runs);
                    }
                }
                _ => {}
            }
        }
    }

    fn text_run(&mut self, runs: &mut Vec<Run>, text: &str, style: Style<'_>) {
        if self.full() {
            return;
        }
        let room = MAX_TEXT - self.text;
        let text = clean(text, room);
        if text.is_empty() {
            return;
        }
        self.text += text.chars().count();
        let href = style.href.and_then(|h| self.address(h));
        runs.push(Run {
            text,
            b: style.b,
            i: style.i,
            code: style.code,
            href,
        });
    }
}

impl Run {
    pub fn plain(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            b: false,
            i: false,
            code: false,
            href: None,
        }
    }

    fn same_style(&self, other: &Run) -> bool {
        self.b == other.b && self.i == other.i && self.code == other.code && self.href == other.href
    }
}

/// Text without control characters, whitespace collapsed to single spaces,
/// at most `max` characters.
pub fn clean(text: &str, max: usize) -> String {
    let mut out = String::new();
    let mut space = false;
    let mut count = 0;
    for c in text.chars() {
        if count >= max {
            break;
        }
        if c.is_whitespace()
            || c.is_control()
            || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
        {
            space = true;
            continue;
        }
        if space && !out.is_empty() {
            out.push(' ');
            count += 1;
        }
        space = false;
        out.push(c);
        count += 1;
    }
    if space && !out.is_empty() && count < max {
        out.push(' ');
    }
    out
}

/// Joins runs of the same style, puts single spaces between words of
/// neighbouring runs and trims the ends.
fn tidy(runs: Vec<Run>) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for run in runs {
        if run.text.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.same_style(&run) => {
                if last.text.ends_with(' ') && run.text.starts_with(' ') {
                    last.text.push_str(&run.text[1..]);
                } else {
                    last.text.push_str(&run.text);
                }
            }
            Some(last) if last.text.ends_with(' ') && run.text.starts_with(' ') => {
                let mut run = run;
                run.text.remove(0);
                if !run.text.is_empty() {
                    out.push(run);
                }
            }
            _ => out.push(run),
        }
    }
    if let Some(first) = out.first_mut() {
        let trimmed = first.text.trim_start().to_owned();
        first.text = trimmed;
    }
    if let Some(last) = out.last_mut() {
        let trimmed = last.text.trim_end().to_owned();
        last.text = trimmed;
    }
    out.retain(|r| !r.text.is_empty());
    out
}

/// The plain text of the first paragraph (or of anything, if none), at most
/// `max` characters, cut at a word with "…".
pub fn summary(blocks: &[Block], max: usize) -> String {
    fn text_of(runs: &[Run]) -> String {
        runs.iter().map(|r| r.text.as_str()).collect()
    }
    let text = blocks
        .iter()
        .find_map(|b| match b {
            Block::P { c } => Some(text_of(c)),
            _ => None,
        })
        .unwrap_or_default();
    shorten(&text, max)
}

pub fn shorten(text: &str, max: usize) -> String {
    let text = clean(text, usize::MAX);
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let cut: String = text.chars().take(max.saturating_sub(1)).collect();
    let cut = match cut.rfind(' ') {
        Some(at) if at > cut.len() / 2 => &cut[..at],
        _ => cut.as_str(),
    };
    format!("{}…", cut.trim_end_matches([',', '.', ';', ':', ' ']))
}

/// Text made safe to put in HTML.
pub fn escaped(text: &str) -> String {
    let mut out = String::new();
    escape(text, &mut out);
    out
}

fn escape(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}

fn runs_html(runs: &[Run], out: &mut String) {
    for run in runs {
        let mut close = Vec::new();
        if let Some(href) = &run.href {
            out.push_str("<a href=\"");
            escape(href, out);
            out.push_str("\">");
            close.push("</a>");
        }
        for (on, open, end) in [
            (run.b, "<b>", "</b>"),
            (run.i, "<i>", "</i>"),
            (run.code, "<code>", "</code>"),
        ] {
            if on {
                out.push_str(open);
                close.push(end);
            }
        }
        escape(&run.text, out);
        for end in close.iter().rev() {
            out.push_str(end);
        }
    }
}

/// Plain HTML of blocks, for the translator.
pub fn to_html(blocks: &[Block]) -> String {
    let mut out = String::new();
    blocks_html(blocks, &mut out);
    out
}

fn blocks_html(blocks: &[Block], out: &mut String) {
    for block in blocks {
        match block {
            Block::H { level, c } => {
                out.push_str(&format!("<h{level}>"));
                runs_html(c, out);
                out.push_str(&format!("</h{level}>"));
            }
            Block::P { c } => {
                out.push_str("<p>");
                runs_html(c, out);
                out.push_str("</p>");
            }
            Block::Ul { items } | Block::Ol { items } => {
                let tag = if matches!(block, Block::Ul { .. }) {
                    "ul"
                } else {
                    "ol"
                };
                out.push_str(&format!("<{tag}>"));
                for item in items {
                    out.push_str("<li>");
                    blocks_html(item, out);
                    out.push_str("</li>");
                }
                out.push_str(&format!("</{tag}>"));
            }
            Block::Img { src, alt } => {
                out.push_str("<img src=\"");
                escape(src, out);
                out.push_str("\" alt=\"");
                escape(alt, out);
                out.push_str("\">");
            }
        }
    }
}

/// Splits blocks into pieces whose HTML stays under `max` bytes (a block
/// bigger than that is a piece of its own).
pub fn chunks(blocks: &[Block], max: usize) -> Vec<Vec<Block>> {
    let mut out: Vec<Vec<Block>> = Vec::new();
    let mut size = 0;
    for block in blocks {
        let len = to_html(std::slice::from_ref(block)).len();
        if out.is_empty() || size + len > max {
            out.push(Vec::new());
            size = 0;
        }
        size += len;
        if let Some(last) = out.last_mut() {
            last.push(block.clone());
        }
    }
    out
}

/// Every picture address in the blocks, for fetching.
pub fn images(blocks: &[Block], out: &mut Vec<String>) {
    for block in blocks {
        match block {
            Block::Img { src, .. } => out.push(src.clone()),
            Block::Ul { items } | Block::Ol { items } => {
                for item in items {
                    images(item, out);
                }
            }
            _ => {}
        }
    }
}

/// Swaps picture addresses for our stored ones; pictures without one go.
pub fn replace_images(blocks: Vec<Block>, stored: &dyn Fn(&str) -> Option<String>) -> Vec<Block> {
    blocks
        .into_iter()
        .filter_map(|block| match block {
            Block::Img { src, alt } => stored(&src).map(|src| Block::Img { src, alt }),
            Block::Ul { items } => Some(Block::Ul {
                items: items
                    .into_iter()
                    .map(|i| replace_images(i, stored))
                    .collect(),
            }),
            Block::Ol { items } => Some(Block::Ol {
                items: items
                    .into_iter()
                    .map(|i| replace_images(i, stored))
                    .collect(),
            }),
            other => Some(other),
        })
        .collect()
}
