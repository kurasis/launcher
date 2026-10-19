//! Project descriptions and changelogs as a small tree the interface draws
//! with its own elements (D-066).
//!
//! Authors write Markdown, often with HTML in it. Nothing of it reaches the
//! webview as markup: the text becomes text nodes, a link is kept only when it
//! is `https:`, a picture only when the catalog serves it (other sites never
//! see a player's address), and HTML is read for its pictures, line breaks and
//! plain text, never drawn. Size and depth are bounded.

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

    fn close(&mut self) {
        if self.skipped.last() == Some(&self.stack.len()) {
            self.skipped.pop();
            return;
        }
        if self.stack.len() <= 1 {
            return;
        }
        let Some(node) = self.stack.pop() else {
            return;
        };
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

    /// HTML is never drawn: its pictures, line breaks and text are kept.
    fn html(&mut self, html: &str) {
        let mut at = 0;
        for found in TAG.captures_iter(html) {
            let Some(whole) = found.get(0) else {
                continue;
            };
            if !self.hidden {
                self.add_text(&entities(&html[at..whole.start()]));
            }
            at = whole.end();
            let closing = !found[1].is_empty();
            let name = found[2].to_ascii_lowercase();
            match name.as_str() {
                "script" | "style" => self.hidden = !closing,
                "br" if !self.hidden => self.push(Node::new("br")),
                "img" if !self.hidden => {
                    let address = SRC.captures(&found[3]).and_then(|c| {
                        c.get(1)
                            .or_else(|| c.get(2))
                            .or_else(|| c.get(3))
                            .map(|m| entities(m.as_str()))
                    });
                    if let Some(href) = address.as_deref().and_then(|a| (self.image)(a)) {
                        self.push(Node {
                            href: Some(href),
                            ..Node::new("img")
                        });
                    }
                }
                _ => {}
            }
        }
        if !self.hidden {
            self.add_text(&entities(&html[at..]));
        }
    }
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

/// The few character references authors use; any other stays as written.
fn entities(text: &str) -> String {
    text.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
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
