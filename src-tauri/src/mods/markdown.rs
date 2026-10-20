//! Project descriptions and changelogs as a small tree the interface draws
//! with its own elements (D-066).
//!
//! Authors write Markdown, often with HTML in it, and CurseForge's
//! descriptions are HTML only. Nothing of it reaches the webview as markup:
//! the text becomes text nodes, a link is kept only when it is `https:`, a
//! picture only when the catalog serves it (other sites never see a player's
//! address), and of HTML only a fixed set of elements is read into the same
//! nodes (paragraphs, headings, lists, tables, emphasis, links, pictures);
//! attributes other than a link's or picture's address are never read, and
//! anything else gives its text. Size and depth are bounded.

use std::sync::LazyLock;

use pulldown_cmark::{Event, Options, Parser, Tag};
use regex::Regex;
use serde::Serialize;

/// Nodes in one document.
const MAX_NODES: usize = 20_000;
/// Characters of text in one document.
const MAX_TEXT: usize = 256 * 1024;
/// Containers inside containers; deeper ones give their content to the
/// parent.
const MAX_DEPTH: usize = 24;
/// The longest link or picture address kept.
const MAX_URL: usize = 2048;

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("markdown pattern")
}

/// An HTML tag: whether it closes, its name and its attributes.
static TAG: LazyLock<Regex> = LazyLock::new(|| re(r"(?is)<\s*(/?)\s*([a-z][a-z0-9]*)([^>]*)>"));
/// A `src` attribute, quoted or not.
static SRC: LazyLock<Regex> =
    LazyLock::new(|| re(r#"(?is)\bsrc\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#));
/// An `href` attribute, quoted or not.
static HREF: LazyLock<Regex> =
    LazyLock::new(|| re(r#"(?is)\bhref\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#));
/// An `alt` attribute, quoted or not.
static ALT: LazyLock<Regex> =
    LazyLock::new(|| re(r#"(?is)\balt\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#));
/// An HTML comment.
static COMMENT: LazyLock<Regex> = LazyLock::new(|| re(r"(?s)<!--.*?(?:-->|$)"));
/// A character reference.
static ENTITY: LazyLock<Regex> =
    LazyLock::new(|| re(r"&(#[0-9]{1,7}|#[xX][0-9a-fA-F]{1,6}|[a-zA-Z]{2,8});"));
/// Runs of white space.
static SPACES: LazyLock<Regex> = LazyLock::new(|| re(r"\s+"));

/// One element: `p`, `h1`…`h6`, `quote`, `pre`, `ul`, `ol`, `li`, `table`,
/// `tr`, `th`, `td`, `em`, `strong`, `del`, `sup`, `sub`, `a` (with `href`),
/// `img` (`href` is the picture, `text` its description), `code`, `br`, `hr`,
/// `text` or `span` (anything else that holds content).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Node {
    pub k: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub c: Vec<Node>,
}

impl Node {
    fn new(k: &'static str) -> Self {
        Self {
            k,
            text: None,
            href: None,
            c: Vec::new(),
        }
    }

    fn text(text: String) -> Self {
        Self {
            text: Some(text),
            ..Self::new("text")
        }
    }
}

/// An address the description may link to: `https:` with a host, no
/// credentials.
pub fn link(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.len() > MAX_URL {
        return None;
    }
    let url = url::Url::parse(raw).ok()?;
    (url.scheme() == "https"
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none())
    .then(|| url.to_string())
}

struct Builder<'a> {
    /// Open containers; the first is the document.
    stack: Vec<Node>,
    /// Containers not opened because they were too deep, by level.
    skipped: Vec<usize>,
    nodes: usize,
    text: usize,
    /// Inside `<script>` or `<style>` written as HTML.
    hidden: bool,
    /// Containers opened by HTML tags: their place in the stack and the tag
    /// that closes them.
    html_open: Vec<(usize, &'static str)>,
    image: &'a dyn Fn(&str) -> Option<String>,
}

impl Builder<'_> {
    fn room(&self) -> bool {
        self.nodes < MAX_NODES && self.text < MAX_TEXT
    }

    fn push(&mut self, node: Node) {
        if !self.room() {
            return;
        }
        self.nodes += 1;
        if let Some(text) = &node.text {
            self.text += text.chars().count();
        }
        if let Some(top) = self.stack.last_mut() {
            top.c.push(node);
        }
    }

    fn add_text(&mut self, text: &str) {
        if text.is_empty() || !self.room() {
            return;
        }
        let left = MAX_TEXT - self.text;
        let text: String = text
            .chars()
            .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
            .take(left)
            .collect();
        // Next to text already there: one node.
        if let Some(top) = self.stack.last_mut()
            && let Some(last) = top.c.last_mut()
            && last.k == "text"
            && let Some(before) = &mut last.text
        {
            self.text += text.chars().count();
            before.push_str(&text);
            return;
        }
        self.push(Node::text(text));
    }

    fn open(&mut self, node: Node) {
        if self.stack.len() > MAX_DEPTH || !self.room() {
            self.skipped.push(self.stack.len());
            return;
        }
        self.nodes += 1;
        self.stack.push(node);
    }

    /// Closes what Markdown opened: first any HTML container left open
    /// inside it.
    fn close(&mut self) {
        if self.skipped.last() == Some(&self.stack.len()) {
            self.skipped.pop();
            return;
        }
        self.close_html_above(0);
        self.pop();
    }

    /// The open container goes into its parent.
    fn pop(&mut self) {
        if self.stack.len() <= 1 {
            return;
        }
        let Some(mut node) = self.stack.pop() else {
            return;
        };
        if self
            .html_open
            .last()
            .is_some_and(|(at, _)| *at == self.stack.len())
        {
            self.html_open.pop();
        }
        // Code written as HTML holds its text, as Markdown's does.
        if node.k == "code" && !node.c.is_empty() {
            node.text = Some(plain(&node.c).chars().take(4096).collect());
            node.c.clear();
        }
        // A link or picture that was not kept leaves its content.
        let keep = match node.k {
            "a" => node.href.is_some(),
            _ => true,
        };
        let Some(top) = self.stack.last_mut() else {
            return;
        };
        if keep {
            top.c.push(node);
        } else {
            top.c.extend(node.c);
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        let node = match tag {
            Tag::Paragraph => Node::new("p"),
            Tag::Heading { level, .. } => Node::new(match level as u8 {
                1 => "h1",
                2 => "h2",
                3 => "h3",
                4 => "h4",
                5 => "h5",
                _ => "h6",
            }),
            Tag::BlockQuote(_) => Node::new("quote"),
            Tag::CodeBlock(_) => Node::new("pre"),
            Tag::List(Some(_)) => Node::new("ol"),
            Tag::List(None) => Node::new("ul"),
            Tag::Item => Node::new("li"),
            Tag::Table(_) => Node::new("table"),
            Tag::TableHead | Tag::TableRow => Node::new("tr"),
            Tag::TableCell => Node::new("td"),
            Tag::Emphasis => Node::new("em"),
            Tag::Strong => Node::new("strong"),
            Tag::Strikethrough => Node::new("del"),
            Tag::Superscript => Node::new("sup"),
            Tag::Subscript => Node::new("sub"),
            Tag::Link { dest_url, .. } => Node {
                href: link(&dest_url),
                ..Node::new("a")
            },
            Tag::Image { dest_url, .. } => Node {
                href: (self.image)(&dest_url),
                ..Node::new("img")
            },
            _ => Node::new("span"),
        };
        self.open(node);
    }

    fn end_image(&mut self) {
        // A picture's content is its description: flattened to text.
        if self.skipped.last() == Some(&self.stack.len()) {
            self.skipped.pop();
            return;
        }
        self.close_html_above(0);
        if self.stack.len() <= 1 {
            return;
        }
        let Some(mut node) = self.stack.pop() else {
            return;
        };
        let alt = plain(&node.c);
        node.c.clear();
        let Some(top) = self.stack.last_mut() else {
            return;
        };
        if node.href.is_some() {
            node.text = (!alt.is_empty()).then_some(alt);
            top.c.push(node);
        } else if !alt.is_empty() {
            top.c.push(Node::text(alt));
        }
    }

    /// Closes the HTML containers opened at `from` in the stack or above it.
    fn close_html_above(&mut self, from: usize) {
        while let Some(&(at, _)) = self.html_open.last() {
            if at < from || at + 1 != self.stack.len() {
                break;
            }
            self.pop();
        }
    }

    /// Closes the HTML container a closing tag names, with what is open
    /// inside it, unless Markdown opened something there.
    fn close_tag(&mut self, tag: &str) {
        let Some(found) = self.html_open.iter().rposition(|(_, t)| *t == tag) else {
            return;
        };
        let at = self.html_open[found].0;
        let opened_by_html = self.html_open.len() - found;
        let skipped_inside = self.skipped.last().is_some_and(|s| *s > at);
        if self.stack.len() - at != opened_by_html || skipped_inside {
            return;
        }
        self.close_html_above(at);
    }

    fn open_tag(&mut self, tag: &'static str, node: Node) {
        // Blocks end an open paragraph; an item, row or cell ends the one
        // before it.
        let ends: &[&str] = match tag {
            "li" => &["p", "li"],
            "tr" => &["p", "td", "th", "tr"],
            "td" | "th" => &["p", "td", "th"],
            "dt" | "dd" => &["p", "dt", "dd"],
            "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol" | "table"
            | "pre" | "blockquote" | "hr" => &["p"],
            _ => &[],
        };
        while let Some(&(at, open)) = self.html_open.last()
            && at + 1 == self.stack.len()
            && ends.contains(&open)
        {
            self.pop();
        }
        if self.stack.len() > MAX_DEPTH || !self.room() {
            return;
        }
        self.nodes += 1;
        self.stack.push(node);
        self.html_open.push((self.stack.len() - 1, tag));
    }

    fn in_pre(&self) -> bool {
        self.stack.iter().any(|n| n.k == "pre")
    }

    /// Text between tags: white space as HTML treats it.
    fn html_text(&mut self, raw: &str) {
        if self.hidden || raw.is_empty() {
            return;
        }
        let text = entities(raw);
        if self.in_pre() {
            self.add_text(&text);
            return;
        }
        let text = SPACES.replace_all(&text, " ");
        if text.trim().is_empty() {
            // Not between the parts of a list or table, nor at the top.
            let holds_text = self.stack.last().is_some_and(|n| {
                !matches!(
                    n.k,
                    "doc" | "ul" | "ol" | "table" | "tr" | "block" | "quote"
                )
            });
            let after_text = self
                .stack
                .last()
                .and_then(|n| n.c.last())
                .is_some_and(|n| n.k != "br");
            if !holds_text || !after_text {
                return;
            }
        }
        self.add_text(&text);
    }

    /// HTML: a fixed set of elements becomes nodes; attributes are read only
    /// for a link's or picture's address; anything else gives its text.
    fn html(&mut self, html: &str) {
        let html = COMMENT.replace_all(html, "");
        let mut at = 0;
        for found in TAG.captures_iter(&html) {
            let Some(whole) = found.get(0) else {
                continue;
            };
            self.html_text(&html[at..whole.start()]);
            at = whole.end();
            let closing = !found[1].is_empty();
            let name = found[2].to_ascii_lowercase();
            let attributes = &found[3];
            if matches!(name.as_str(), "script" | "style") {
                self.hidden = !closing;
                continue;
            }
            if self.hidden {
                continue;
            }
            let Some((tag, kind)) = element(&name) else {
                continue;
            };
            match tag {
                "br" if !closing => self.push(Node::new("br")),
                "hr" if !closing => {
                    self.open_tag("hr", Node::new("hr"));
                    self.pop();
                }
                "img" if !closing => {
                    let address = attribute(&SRC, attributes);
                    if let Some(href) = address.as_deref().and_then(|a| (self.image)(a)) {
                        let alt = attribute(&ALT, attributes)
                            .map(|a| {
                                a.chars()
                                    .filter(|c| !c.is_control())
                                    .take(500)
                                    .collect::<String>()
                            })
                            .filter(|a| !a.trim().is_empty());
                        self.push(Node {
                            href: Some(href),
                            text: alt,
                            ..Node::new("img")
                        });
                    }
                }
                "br" | "hr" | "img" => {}
                _ if closing => self.close_tag(tag),
                // `<p/>` and the like open nothing.
                _ if attributes.trim_end().ends_with('/') => {}
                "a" => {
                    let href = attribute(&HREF, attributes).and_then(|h| link(&h));
                    self.open_tag(
                        "a",
                        Node {
                            href,
                            ..Node::new("a")
                        },
                    );
                }
                _ => self.open_tag(tag, Node::new(kind)),
            }
        }
        self.html_text(&html[at..]);
    }
}

/// An HTML element read into nodes: the tag that closes it and its node.
fn element(name: &str) -> Option<(&'static str, &'static str)> {
    Some(match name {
        "p" => ("p", "p"),
        "h1" => ("h1", "h1"),
        "h2" => ("h2", "h2"),
        "h3" => ("h3", "h3"),
        "h4" => ("h4", "h4"),
        "h5" => ("h5", "h5"),
        "h6" => ("h6", "h6"),
        "div" | "section" | "article" | "center" | "header" | "footer" | "main" | "aside"
        | "figure" | "details" | "summary" | "dl" | "nav" => ("div", "block"),
        "figcaption" | "dt" => ("dt", "p"),
        "dd" => ("dd", "p"),
        "blockquote" => ("blockquote", "quote"),
        "pre" => ("pre", "pre"),
        "code" | "kbd" | "samp" | "tt" => ("code", "code"),
        "ul" => ("ul", "ul"),
        "ol" => ("ol", "ol"),
        "li" => ("li", "li"),
        "table" => ("table", "table"),
        "tr" => ("tr", "tr"),
        "th" => ("th", "th"),
        "td" => ("td", "td"),
        "strong" | "b" => ("strong", "strong"),
        "em" | "i" | "cite" | "dfn" | "var" => ("em", "em"),
        "del" | "s" | "strike" => ("del", "del"),
        "sup" => ("sup", "sup"),
        "sub" => ("sub", "sub"),
        "span" | "font" | "u" | "ins" | "small" | "big" | "mark" | "abbr" | "label" => {
            ("span", "span")
        }
        "a" => ("a", "a"),
        "br" => ("br", "br"),
        "hr" => ("hr", "hr"),
        "img" => ("img", "img"),
        _ => return None,
    })
}

/// An attribute's value, its character references read.
fn attribute(pattern: &Regex, attributes: &str) -> Option<String> {
    pattern.captures(attributes).and_then(|c| {
        c.get(1)
            .or_else(|| c.get(2))
            .or_else(|| c.get(3))
            .map(|m| entities(m.as_str()))
    })
}

/// The text inside nodes, without markup.
fn plain(nodes: &[Node]) -> String {
    let mut out = String::new();
    for node in nodes {
        if let Some(text) = &node.text
            && node.k != "img"
        {
            out.push_str(text);
        }
        out.push_str(&plain(&node.c));
    }
    out.trim().to_owned()
}

/// Character references: numbers and the names authors use; any other
/// stays as written. Control and direction characters are dropped.
fn entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    ENTITY
        .replace_all(text, |c: &regex::Captures<'_>| {
            let name = &c[1];
            let found =
                if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                    u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
                } else if let Some(decimal) = name.strip_prefix('#') {
                    decimal.parse::<u32>().ok().and_then(char::from_u32)
                } else {
                    named(name)
                };
            match found {
                Some(c) if (c.is_control() && c != '\n' && c != '\t') || is_direction(c) => {
                    String::new()
                }
                Some(c) => c.to_string(),
                None => c[0].to_owned(),
            }
        })
        .into_owned()
}

fn is_direction(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

fn named(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "rsquo" => '\u{2019}',
        "lsquo" => '\u{2018}',
        "rdquo" => '\u{201D}',
        "ldquo" => '\u{201C}',
        "mdash" => '\u{2014}',
        "ndash" => '\u{2013}',
        "hellip" => '\u{2026}',
        "laquo" => '\u{AB}',
        "raquo" => '\u{BB}',
        "middot" => '\u{B7}',
        "bull" => '\u{2022}',
        "copy" => '\u{A9}',
        "reg" => '\u{AE}',
        "trade" => '\u{2122}',
        "times" => '\u{D7}',
        "deg" => '\u{B0}',
        "rarr" => '\u{2192}',
        "larr" => '\u{2190}',
        "euro" => '\u{20AC}',
        _ => return None,
    })
}

/// Markdown as a tree of safe nodes. `image` keeps a picture's address (one
/// the catalog serves) or drops it.
pub fn parse(markdown: &str, image: &dyn Fn(&str) -> Option<String>) -> Vec<Node> {
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    let mut builder = Builder {
        stack: vec![Node::new("doc")],
        skipped: Vec::new(),
        nodes: 0,
        text: 0,
        hidden: false,
        html_open: Vec::new(),
        image,
    };
    // Inside a table header row: its cells are headings.
    let mut head = false;
    for event in Parser::new_ext(markdown, options) {
        match event {
            Event::Start(Tag::TableHead) => {
                head = true;
                builder.start(Tag::TableHead);
            }
            Event::Start(Tag::TableCell) if head => builder.open(Node::new("th")),
            Event::Start(tag) => builder.start(tag),
            Event::End(pulldown_cmark::TagEnd::TableHead) => {
                head = false;
                builder.close();
            }
            Event::End(pulldown_cmark::TagEnd::Image) => builder.end_image(),
            Event::End(_) => builder.close(),
            Event::Text(text) => {
                if !builder.hidden {
                    builder.add_text(&text);
                }
            }
            Event::Code(code) => {
                builder.push(Node {
                    text: Some(code.chars().take(4096).collect()),
                    ..Node::new("code")
                });
            }
            Event::Html(html) | Event::InlineHtml(html) => builder.html(&html),
            Event::SoftBreak => builder.add_text(" "),
            Event::HardBreak => builder.push(Node::new("br")),
            Event::Rule => builder.push(Node::new("hr")),
            Event::InlineMath(text) | Event::DisplayMath(text) => builder.add_text(&text),
            Event::FootnoteReference(_) | Event::TaskListMarker(_) => {}
        }
    }
    // Whatever was left open.
    while builder.stack.len() > 1 {
        builder.close();
    }
    builder.stack.pop().map(|doc| doc.c).unwrap_or_default()
}

/// HTML (a CurseForge description) as a tree of safe nodes, as [`parse`]
/// reads the HTML in Markdown.
pub fn parse_html(html: &str, image: &dyn Fn(&str) -> Option<String>) -> Vec<Node> {
    let mut builder = Builder {
        stack: vec![Node::new("doc")],
        skipped: Vec::new(),
        nodes: 0,
        text: 0,
        hidden: false,
        html_open: Vec::new(),
        image,
    };
    builder.html(html);
    while builder.stack.len() > 1 {
        builder.pop();
    }
    builder.stack.pop().map(|doc| doc.c).unwrap_or_default()
}
