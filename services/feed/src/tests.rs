//! Tests of the feed service: cleaning of everything the sources send, the
//! checks on what players send, and the API on a stored state (no network).

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sha2::Digest as _;
use tower::ServiceExt;

use crate::editorial::{Editorial, link};
use crate::html::{self, Block, Run};
use crate::images::{self, reencode, sha_name, twitch_login, video_id};
use crate::mojang::{self, Version, cycle, note_id, version_kind};
use crate::twitch::{RawStream, pick};
use crate::youtube::{Wanted, channel_id, id_in_page, parse_rss, wanted};
use crate::{api, deepl, feed};

// ---------------------------------------------------------------- html ----

fn text_of(blocks: &[Block]) -> String {
    let mut out = String::new();
    for block in blocks {
        match block {
            Block::H { c, .. } | Block::P { c } => {
                for r in c {
                    out.push_str(&r.text);
                }
                out.push('\n');
            }
            Block::Ul { items } | Block::Ol { items } => {
                for item in items {
                    out.push_str(&text_of(item));
                }
            }
            Block::Img { src, alt } => {
                out.push_str(src);
                out.push_str(alt);
                out.push('\n');
            }
        }
    }
    out
}

#[test]
fn html_keeps_structure() {
    let blocks = html::parse(
        "<h1>New</h1><p>Some <b>bold</b> and <i>slanted</i> <code>/give</code> \
         <a href=\"https://www.minecraft.net/x\">link</a>.</p>\
         <ul><li>One</li><li>Two<ul><li>Inner</li></ul></li></ul>\
         <ol><li>First</li></ol><img src=\"/v2/images/a.png\" alt=\"A\">",
        Some("https://launchercontent.mojang.com/v2/"),
    );
    assert_eq!(
        blocks[0],
        Block::H {
            level: 1,
            c: vec![Run::plain("New")]
        }
    );
    let Block::P { c } = &blocks[1] else {
        panic!("{blocks:?}")
    };
    assert!(c.iter().any(|r| r.b && r.text == "bold"));
    assert!(c.iter().any(|r| r.i && r.text == "slanted"));
    assert!(c.iter().any(|r| r.code && r.text == "/give"));
    assert!(
        c.iter()
            .any(|r| r.href.as_deref() == Some("https://www.minecraft.net/x"))
    );
    assert!(matches!(&blocks[2], Block::Ul { items } if items.len() == 2));
    assert!(matches!(&blocks[3], Block::Ol { items } if items.len() == 1));
    assert_eq!(
        blocks[4],
        Block::Img {
            src: "https://launchercontent.mojang.com/v2/images/a.png".into(),
            alt: "A".into()
        }
    );
}

#[test]
fn html_drops_everything_active() {
    let junk = "<script>alert(1)</script><style>p{}</style>\
        <iframe src=\"https://evil.example\"></iframe>\
        <p onclick=\"alert(2)\" style=\"x\">Text<svg><script>alert(3)</script></svg></p>\
        <a href=\"javascript:alert(4)\">js</a> <a href=\"data:text/html,x\">data</a>\
        <a href=\"http://plain.example\">http</a> <a href=\"https://u:p@x.example\">creds</a>\
        <img src=\"javascript:alert(5)\"><img src=\"http://x.example/a.png\">\
        <form><input value=\"secret\"></form><template><p>hidden</p></template>\
        <object data=\"x\"></object><noscript>ns</noscript>";
    let blocks = html::parse(junk, None);
    let json = serde_json::to_string(&blocks).unwrap();
    for bad in [
        "alert",
        "script",
        "javascript",
        "data:",
        "http:",
        "onclick",
        "evil",
        "secret",
        "hidden",
        "u:p",
        "ns",
    ] {
        assert!(!json.contains(bad), "{bad} in {json}");
    }
    assert!(json.contains("Text"));
    // Links that are not kept stay as text without an address.
    assert!(text_of(&blocks).contains("js"));
    assert!(!blocks.iter().any(|b| matches!(b, Block::Img { .. })));
}

#[test]
fn html_relative_without_base_goes() {
    let blocks = html::parse("<img src=\"/a.png\"><p><a href=\"/x\">x</a></p>", None);
    assert_eq!(
        blocks,
        vec![Block::P {
            c: vec![Run::plain("x")]
        }]
    );
}

#[test]
fn html_strips_invisible_and_control_characters() {
    let blocks = html::parse("<p>a\u{202E}b\u{200B}c\u{0007}d\u{FEFF}</p>", None);
    let text = text_of(&blocks);
    for c in ['\u{202E}', '\u{200B}', '\u{0007}', '\u{FEFF}'] {
        assert!(!text.contains(c));
    }
}

#[test]
fn html_is_bounded() {
    // Deep nesting: no stack overflow, lists flattened past the limit.
    let deep = "<ul><li>".repeat(2000) + "x" + &"</li></ul>".repeat(2000);
    assert!(text_of(&html::parse(&deep, None)).contains('x'));
    let deep_divs = "<div><span>".repeat(2000) + "y<script>alert(1)</script>";
    let blocks = html::parse(&deep_divs, None);
    assert!(text_of(&blocks).contains('y'));
    assert!(!text_of(&blocks).contains("alert"));
    // Too many blocks and too much text are cut.
    let many = "<p>a</p>".repeat(20_000);
    assert!(html::parse(&many, None).len() <= 5000);
    let long = format!("<p>{}</p>", "word ".repeat(200_000));
    let total: usize = text_of(&html::parse(&long, None)).chars().count();
    assert!(total <= 400_001);
    // A huge address is dropped.
    let url = format!("https://x.example/{}", "a".repeat(5000));
    assert!(html::parse(&format!("<img src=\"{url}\">"), None).is_empty());
}

#[test]
fn html_round_trip_is_stable() {
    let blocks = html::parse(
        "<h2>T &amp; &lt;x&gt;</h2><p>A <b>\"b\"</b> <a href=\"https://a.example/?q=1&amp;r=2\">l</a></p>\
         <ul><li>i</li></ul>",
        None,
    );
    let again = html::parse(&html::to_html(&blocks), None);
    assert_eq!(blocks, again);
    // Text never becomes markup.
    let html_text = html::to_html(&[Block::P {
        c: vec![Run::plain("<script>alert(1)</script>")],
    }]);
    assert!(!html_text.contains("<script>"));
    assert_eq!(
        html::escaped("<a href=\"x\">&"),
        "&lt;a href=&quot;x&quot;&gt;&amp;"
    );
}

#[test]
fn html_chunks_keep_every_block() {
    let blocks: Vec<Block> = (0..100)
        .map(|i| Block::P {
            c: vec![Run::plain(&format!("paragraph {i} {}", "x".repeat(50)))],
        })
        .collect();
    let pieces = html::chunks(&blocks, 1000);
    assert!(pieces.len() > 1);
    assert_eq!(pieces.iter().map(Vec::len).sum::<usize>(), 100);
    for piece in &pieces {
        assert!(html::to_html(piece).len() <= 1000);
    }
    assert!(html::chunks(&[], 10).is_empty());
}

#[test]
fn html_summary_and_shorten() {
    let blocks = html::parse("<h1>Head</h1><p>The first   paragraph here.</p>", None);
    assert_eq!(html::summary(&blocks, 100), "The first paragraph here.");
    let short = html::shorten("one two three four five six", 12);
    assert!(short.ends_with('…'));
    assert!(short.chars().count() <= 12);
    assert_eq!(html::shorten("  short ", 10), "short");
    assert_eq!(html::clean("a \n\t b", 100), "a b");
    assert_eq!(html::clean("abcdef", 3), "abc");
}

#[test]
fn html_pictures_are_replaced_or_dropped() {
    let blocks = vec![
        Block::Img {
            src: "https://a/1".into(),
            alt: String::new(),
        },
        Block::Ul {
            items: vec![vec![Block::Img {
                src: "https://a/2".into(),
                alt: String::new(),
            }]],
        },
    ];
    let mut found = Vec::new();
    html::images(&blocks, &mut found);
    assert_eq!(found, vec!["https://a/1", "https://a/2"]);
    let replaced = html::replace_images(blocks, &|src| {
        (src == "https://a/1").then(|| "img/x.jpg".to_owned())
    });
    let mut found = Vec::new();
    html::images(&replaced, &mut found);
    assert_eq!(found, vec!["img/x.jpg"]);
}

// ------------------------------------------------------------- youtube ----

const RSS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns:yt="http://www.youtube.com/xml/schemas/2015" xmlns:media="http://search.yahoo.com/mrss/" xmlns="http://www.w3.org/2005/Atom">
 <title>Some &amp; Channel</title>
 <entry>
  <yt:videoId>abcdefghijk</yt:videoId>
  <title>A video &lt;b&gt;</title>
  <link rel="alternate" href="https://www.youtube.com/watch?v=abcdefghijk"/>
  <published>2026-10-18T12:00:00+00:00</published>
 </entry>
 <entry>
  <yt:videoId>shortshort1</yt:videoId>
  <title>A short</title>
  <link rel="alternate" href="https://www.youtube.com/shorts/shortshort1"/>
  <published>2026-10-17T12:00:00+02:00</published>
 </entry>
 <entry>
  <yt:videoId>bad id!</yt:videoId>
  <title>Bad</title>
  <published>2026-10-17T12:00:00+00:00</published>
 </entry>
 <entry>
  <yt:videoId>nodate00000</yt:videoId>
  <title>No date</title>
  <published>yesterday</published>
 </entry>
</feed>"#;

#[test]
fn rss_is_read() {
    let channel = parse_rss(RSS, "UC1sELGmy5jp5fQUugmuYlXQ").unwrap();
    assert_eq!(channel.title, "Some & Channel");
    assert_eq!(channel.videos.len(), 2);
    let first = &channel.videos[0];
    assert_eq!(first.id, "abcdefghijk");
    assert_eq!(first.title, "A video <b>");
    assert_eq!(first.date, "2026-10-18T12:00:00Z");
    assert_eq!(first.channel, "Some & Channel");
    assert!(!first.short);
    assert!(channel.videos[1].short);
    assert_eq!(channel.videos[1].date, "2026-10-17T10:00:00Z");
}

#[test]
fn rss_junk_is_refused() {
    let doctype = r#"<?xml version="1.0"?><!DOCTYPE lolz [<!ENTITY lol "lol">]><feed><title>&lol;</title></feed>"#;
    assert!(parse_rss(doctype, "x").is_err());
    let deep = "<a>".repeat(100) + &"</a>".repeat(100);
    assert!(parse_rss(&deep, "x").is_err());
    assert!(parse_rss("<feed><title>x</feed>", "x").is_err());
    let empty = parse_rss("not xml at all", "x").unwrap();
    assert!(empty.videos.is_empty());
}

#[test]
fn rss_keeps_fifteen() {
    let entry = |i: usize| {
        format!(
            "<entry><yt:videoId>video{i:06}</yt:videoId><title>t</title>\
             <published>2026-10-18T12:00:00+00:00</published></entry>"
        )
    };
    let xml = format!(
        "<feed><title>c</title>{}</feed>",
        (0..40).map(entry).collect::<String>()
    );
    assert_eq!(parse_rss(&xml, "x").unwrap().videos.len(), 15);
}

#[test]
fn channels_named_by_players() {
    let id = "UC1sELGmy5jp5fQUugmuYlXQ";
    assert_eq!(wanted(id), Some(Wanted::Channel(id.into())));
    for link in [
        format!("https://www.youtube.com/channel/{id}"),
        format!("youtube.com/channel/{id}/videos"),
        format!("http://m.youtube.com/channel/{id}"),
    ] {
        assert_eq!(wanted(&link), Some(Wanted::Channel(id.into())), "{link}");
    }
    assert_eq!(wanted("@Grian"), Some(Wanted::Handle("Grian".into())));
    assert_eq!(
        wanted("https://www.youtube.com/@xisumavoid/videos"),
        Some(Wanted::Handle("xisumavoid".into()))
    );
    assert_eq!(
        wanted("https://www.youtube.com/@%D0%9B%D0%BE%D0%B9%D0%B4"),
        Some(Wanted::Handle("Лойд".into()))
    );
    assert_eq!(wanted("@Лойд"), Some(Wanted::Handle("Лойд".into())));
    for (link, video) in [
        ("https://youtu.be/abcdefghijk", "abcdefghijk"),
        (
            "https://www.youtube.com/watch?v=abcdefghijk&t=10",
            "abcdefghijk",
        ),
        ("https://www.youtube.com/shorts/abcdefghijk", "abcdefghijk"),
        ("https://www.youtube.com/live/abcdefghijk", "abcdefghijk"),
    ] {
        assert_eq!(wanted(link), Some(Wanted::Video(video.into())), "{link}");
    }
}

#[test]
fn junk_channel_names_are_refused() {
    for junk in [
        "",
        "   ",
        "@",
        "@ab",
        "@a b",
        "@../../etc",
        "@<script>",
        "@a%2Fb",
        "https://evil.example/@Grian",
        "https://www.youtube.com.evil.example/@Grian",
        "https://youtube.com@evil.example/@Grian",
        "https://www.youtube.com:8080/@Grian",
        "ftp://www.youtube.com/@Grian",
        "javascript:alert(1)",
        "file:///etc/passwd",
        "https://www.youtube.com/watch?v=short",
        "https://www.youtube.com/watch?v=../../../../",
        "https://www.youtube.com/channel/UCnot-an-id",
        "https://www.youtube.com/user/someone",
        "https://youtu.be/",
        "https://youtu.be/%2e%2e%2fabcdefgh",
        "UC1sELGmy5jp5fQUugmuYlX\n",
        "@Grian\u{0000}",
        "https://www.youtube.com/@Gri\u{202E}an",
    ] {
        assert_eq!(wanted(junk), None, "{junk:?}");
    }
    assert_eq!(wanted(&format!("@{}", "a".repeat(31))), None);
    assert_eq!(wanted(&"a".repeat(201)), None);
}

#[test]
fn channel_id_in_a_page() {
    let id = "UC1sELGmy5jp5fQUugmuYlXQ";
    assert_eq!(
        id_in_page(&format!("x\"externalId\":\"{id}\",y")),
        Some(id.to_owned())
    );
    assert_eq!(id_in_page("\"externalId\":\"UCshort\""), None);
    assert_eq!(id_in_page("nothing"), None);
    assert!(channel_id(id));
    assert!(!channel_id("UC1sELGmy5jp5fQUugmuYlX!"));
    assert!(!channel_id("XX1sELGmy5jp5fQUugmuYlXQ"));
}

// -------------------------------------------------------------- twitch ----

fn raw(login: &str, viewers: u64, mature: bool, thumb: &str) -> RawStream {
    RawStream {
        user_login: login.into(),
        user_name: format!("{login}\u{202E} name"),
        title: "  Playing   <b>Minecraft</b>  ".into(),
        viewer_count: viewers,
        started_at: "2026-10-19T10:00:00Z".into(),
        thumbnail_url: thumb.into(),
        is_mature: mature,
    }
}

const THUMB: &str = "https://static-cdn.jtvnw.net/previews-ttv/live_user_x-{width}x{height}.jpg";

#[test]
fn streams_are_picked() {
    let streams = pick(
        vec![
            raw("small", 10, false, THUMB),
            raw("adult", 9999, true, THUMB),
            raw("Blocked", 5000, false, THUMB),
            raw("big", 3000, false, THUMB),
            raw("../etc", 2000, false, THUMB),
            raw(
                "elsewhere",
                1000,
                false,
                "https://evil.example/{width}x{height}.jpg",
            ),
            raw(
                "plainhttp",
                1000,
                false,
                "http://static-cdn.jtvnw.net/x.jpg",
            ),
        ],
        &["blocked".to_owned()],
    );
    let logins: Vec<&str> = streams.iter().map(|s| s.login.as_str()).collect();
    assert_eq!(logins, vec!["big", "small"]);
    assert!(streams[0].preview.contains("440x248"));
    assert_eq!(streams[0].title, "Playing <b>Minecraft</b>");
    assert!(!streams[0].name.contains('\u{202E}'));
    // The preview address never reaches a player.
    assert!(
        !serde_json::to_string(&streams[0])
            .unwrap()
            .contains("jtvnw")
    );
    let many: Vec<RawStream> = (0..20)
        .map(|i| raw(&format!("user{i}"), i, false, THUMB))
        .collect();
    assert_eq!(pick(many, &[]).len(), crate::twitch::STREAMS);
}

#[test]
fn twitch_needs_both_secrets() {
    assert!(crate::twitch::Twitch::new(String::new(), "s".into()).is_none());
    assert!(crate::twitch::Twitch::new("c".into(), "  ".into()).is_none());
    assert!(crate::twitch::Twitch::new("c".into(), "s".into()).is_some());
}

// --------------------------------------------------------------- deepl ----

#[test]
fn deepl_targets() {
    assert_eq!(deepl::target("ru"), Some("RU"));
    assert_eq!(deepl::target("ar"), Some("AR"));
    assert_eq!(deepl::target("en"), None);
    assert!(deepl::DeepL::new("  ".into()).is_none());
    assert!(deepl::DeepL::new("key:fx".into()).is_some());
}

// ----------------------------------------------------------- editorial ----

#[test]
fn our_editorial_loads() {
    let editorial = Editorial::load(crate::EDITORIAL).unwrap();
    for lang in ["en", "ru", "ar"] {
        assert!(editorial.channels_for(lang).count() >= 5, "{lang}");
    }
    assert!(!editorial.tips.is_empty());
    let mut ids: Vec<&str> = editorial.channels.iter().map(|c| c.id.as_str()).collect();
    ids.sort_unstable();
    let count = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), count, "a channel is listed twice");
}

#[test]
fn editorial_mistakes_are_refused() {
    for bad in [
        "[[channel]]\nid = \"nope\"\nname = \"x\"\nlang = \"en\"",
        "[[channel]]\nid = \"UC1sELGmy5jp5fQUugmuYlXQ\"\nname = \"x\"\nlang = \"de\"",
        "[[event]]\nid = \"e\"\nstarts = \"2026-11-02T00:00:00Z\"\nends = \"2026-11-01T00:00:00Z\"\ntitle = { en = \"E\" }",
        "[[event]]\nid = \"e\"\nstarts = \"soon\"\nends = \"2026-11-01T00:00:00Z\"\ntitle = { en = \"E\" }",
        "[[event]]\nid = \"e\"\nstarts = \"2026-11-01T00:00:00Z\"\nends = \"2026-11-02T00:00:00Z\"\nurl = \"https://evil.example\"\ntitle = { en = \"E\" }",
        "[[announcement]]\nid = \"a\"\nuntil = \"2026-11-01T00:00:00Z\"\nurl = \"javascript:alert(1)\"\ntitle = { en = \"A\" }\ntext = { en = \"T\" }",
        "not toml [",
    ] {
        assert!(Editorial::load(bad).is_err(), "{bad}");
    }
    for ok in [
        "https://www.youtube.com/watch?v=x",
        "https://www.twitch.tv/x",
        "https://www.minecraft.net/x",
        "https://feed.lapi.sh/x",
    ] {
        assert!(link(ok), "{ok}");
    }
    for bad in [
        "http://www.minecraft.net/",
        "https://evil.example/",
        "https://lapi.sh.evil.example/",
        "https://user@www.minecraft.net/",
        "https://www.minecraft.net:444/",
    ] {
        assert!(!link(bad), "{bad}");
    }
}

#[test]
fn editorial_shows_in_time() {
    let editorial = Editorial::load(
        "[[event]]\nid = \"live\"\nstarts = \"2026-11-01T00:00:00Z\"\nends = \"2026-11-02T00:00:00Z\"\n\
         title = { en = \"Live\", ru = \"Лайв\" }\n\
         [[announcement]]\nid = \"a\"\nuntil = \"2026-11-01T00:00:00Z\"\n\
         title = { en = \"A\" }\ntext = { en = \"T\", ar = \"ت\" }\n\
         [[tip]]\ntext = { en = \"one\" }\n[[tip]]\ntext = { en = \"two\", ru = \"два\" }",
    )
    .unwrap();
    let at = |s: &str| {
        chrono::DateTime::parse_from_rfc3339(s)
            .unwrap()
            .with_timezone(&chrono::Utc)
    };
    assert!(
        editorial
            .events("en", at("2026-10-01T00:00:00Z"))
            .is_empty()
    );
    let shown = editorial.events("ru", at("2026-10-20T00:00:00Z"));
    assert_eq!(shown[0].title, "Лайв");
    assert_eq!(
        editorial.events("ar", at("2026-11-01T12:00:00Z"))[0].title,
        "Live"
    );
    assert!(
        editorial
            .events("en", at("2026-11-03T00:00:00Z"))
            .is_empty()
    );
    assert_eq!(
        editorial.announcements("ar", at("2026-10-20T00:00:00Z"))[0].text,
        "ت"
    );
    assert!(
        editorial
            .announcements("en", at("2026-11-01T00:00:00Z"))
            .is_empty()
    );
    let day1 = editorial.tip("ru", at("2026-10-19T00:00:00Z")).unwrap();
    let day2 = editorial.tip("ru", at("2026-10-20T00:00:00Z")).unwrap();
    assert_ne!(day1, day2);
    assert!(["one", "два"].contains(&day1.as_str()));
}

// -------------------------------------------------------------- mojang ----

fn version(id: &str, kind: &str, date: &str) -> Version {
    Version {
        id: id.into(),
        kind: kind.into(),
        date: date.into(),
    }
}

#[test]
fn version_kinds() {
    assert_eq!(version_kind("26.4-snapshot-4", false), "snapshot");
    assert_eq!(version_kind("25w14a", false), "snapshot");
    assert_eq!(version_kind("26.3-pre-1", false), "pre");
    assert_eq!(version_kind("1.21.5-rc1", false), "rc");
    assert_eq!(version_kind("1.20 Pre-Release 2", false), "pre");
    assert_eq!(version_kind("26.3", true), "release");
}

#[test]
fn the_update_cycle() {
    let versions = vec![
        version("26.4-pre-1", "pre", "2026-10-15"),
        version("26.4-snapshot-3", "snapshot", "2026-10-08"),
        version("26.4-snapshot-2", "snapshot", "2026-10-01"),
        version("26.4-snapshot-1", "snapshot", "2026-09-24"),
        version("26.3", "release", "2026-09-10"),
        version("26.3-rc-1", "rc", "2026-09-05"),
    ];
    let c = cycle(&versions).unwrap();
    assert_eq!(c.target, "26.4");
    assert_eq!((c.snapshots, c.pre, c.rc), (3, 1, 0));
    assert!(!c.released);
    assert_eq!(c.stage, "pre");
    let released = cycle(&versions[4..]).unwrap();
    assert_eq!(released.target, "26.3");
    assert!(released.released);
    assert_eq!(released.rc, 1);
    assert!(cycle(&[version("25w14a", "snapshot", "2025-04-01")]).is_none());
    assert!(cycle(&[]).is_none());
}

#[test]
fn note_ids() {
    assert!(note_id("j-26-4-snapshot-4"));
    assert!(note_id("n-some-news"));
    for bad in [
        "x-1",
        "j-",
        "j-../x",
        "j-A",
        "j-a/b",
        "j-a.json",
        &"j-a".repeat(40),
    ] {
        assert!(!note_id(bad), "{bad}");
    }
}

// -------------------------------------------------------------- images ----

fn png(width: u32, height: u32) -> Vec<u8> {
    let picture = image::RgbImage::from_pixel(width, height, image::Rgb([200, 30, 30]));
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(picture)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

#[test]
fn pictures_are_reencoded() {
    let jpeg = reencode(&png(2000, 1000), images::PREVIEW).unwrap();
    let back = image::load_from_memory(&jpeg).unwrap();
    assert_eq!((back.width(), back.height()), (480, 240));
    assert_eq!(&jpeg[..3], &[0xFF, 0xD8, 0xFF]);
    let small = reencode(&png(100, 50), images::WIDE).unwrap();
    assert_eq!(image::load_from_memory(&small).unwrap().width(), 100);
}

#[test]
fn junk_pictures_are_refused() {
    assert!(reencode(b"", 100).is_err());
    assert!(reencode(b"<svg xmlns='http://www.w3.org/2000/svg'/>", 100).is_err());
    assert!(reencode(b"GIF89a\x01", 100).is_err());
    let mut cut = png(64, 64);
    cut.truncate(cut.len() / 2);
    assert!(reencode(&cut, 100).is_err());
    // A PNG that claims to be 100000 pixels wide.
    let mut huge = png(1, 1);
    huge[16..20].copy_from_slice(&100_000u32.to_be_bytes());
    assert!(reencode(&huge, 100).is_err());
    // A BMP is a picture but not one we take.
    let mut bmp = Vec::new();
    image::DynamicImage::ImageRgb8(image::RgbImage::new(2, 2))
        .write_to(&mut std::io::Cursor::new(&mut bmp), image::ImageFormat::Bmp)
        .ok();
    if !bmp.is_empty() {
        assert!(reencode(&bmp, 100).is_err());
    }
}

#[test]
fn picture_names() {
    assert!(sha_name(&format!("{}.jpg", "a".repeat(64))));
    assert!(!sha_name(&format!("{}.jpg", "A".repeat(64))));
    assert!(!sha_name(&format!("{}.png", "a".repeat(64))));
    assert!(!sha_name("../state.json"));
    assert!(video_id("abc-_123XYZ"));
    assert!(!video_id("abc/../1234"));
    assert!(twitch_login("some_user1"));
    assert!(!twitch_login("Some_User"));
    assert!(!twitch_login("ab"));
    assert!(!twitch_login("../x"));
}

// ----------------------------------------------------------------- api ----

fn note(id: &str, image: Option<&str>) -> mojang::Note {
    mojang::Note {
        id: id.into(),
        title: format!("Title {id}"),
        version: Some("26.4-snapshot-1".into()),
        kind: Some("snapshot".into()),
        date: "2026-10-15T00:00:00Z".into(),
        image: image.map(str::to_owned),
        summary: "Summary".into(),
        url: None,
        blocks: vec![Block::P {
            c: vec![Run::plain("Body")],
        }],
    }
}

/// A feed on a stored state, as if the sources had answered.
fn app(dir: &std::path::Path) -> Arc<api::App> {
    let picture = format!("{}.jpg", "a".repeat(64));
    let editorial = Editorial::load(crate::EDITORIAL).unwrap();
    let ru_channel = editorial
        .channels_for("ru")
        .find(|c| c.lang == "ru")
        .unwrap()
        .id
        .clone();
    std::fs::create_dir_all(dir.join("notes")).unwrap();
    std::fs::create_dir_all(dir.join("img")).unwrap();
    std::fs::write(
        dir.join("img").join(&picture),
        reencode(&png(4, 4), 10).unwrap(),
    )
    .unwrap();
    let id = "j-26-4-snapshot-1";
    std::fs::write(
        dir.join("notes").join(format!("{id}.json")),
        serde_json::to_vec(&note(id, Some(&format!("img/{picture}")))).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.join("notes").join(format!("{id}.ru.json")),
        serde_json::to_vec(&feed::Translated {
            title: "Заголовок".into(),
            summary: "Кратко".into(),
            blocks: vec![Block::P {
                c: vec![Run::plain("Текст")],
            }],
        })
        .unwrap(),
    )
    .unwrap();
    let state = json!({
        "versions": [
            {"id": "26.4-snapshot-1", "kind": "snapshot", "date": "2026-10-15T00:00:00Z"},
            {"id": "26.3", "kind": "release", "date": "2026-09-10T00:00:00Z"}
        ],
        "patch_notes": [id, "j-missing"],
        "news": [],
        "channels": {
            ru_channel.clone(): {
                "title": "Канал",
                "videos": [{
                    "id": "abcdefghijk", "channelId": ru_channel, "channel": "Канал",
                    "title": "Видео", "date": "2026-10-18T00:00:00Z", "short": false
                }]
            }
        },
        "streams": {
            "ru": [{"login": "somebody", "name": "S", "title": "T", "viewers": 5,
                    "started": "2026-10-19T00:00:00Z"}]
        },
        "fetched": {"mojang": "2026-10-19T00:00:00Z"}
    });
    std::fs::write(dir.join("state.json"), serde_json::to_vec(&state).unwrap()).unwrap();
    let feed = feed::Feed::open(dir, editorial, crate::net::client().unwrap(), None, None).unwrap();
    Arc::new(api::App {
        feed,
        limiter: api::Limiter::default(),
    })
}

async fn call(
    app: &Arc<api::App>,
    request: Request<Body>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let response = api::router(Arc::clone(app)).oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    (status, headers, body)
}

async fn get(app: &Arc<api::App>, uri: &str) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    call(app, Request::get(uri).body(Body::empty()).unwrap()).await
}

fn json_of(body: &[u8]) -> Value {
    serde_json::from_slice(body).unwrap()
}

#[tokio::test]
async fn the_feed_in_each_language() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    let (status, headers, body) = get(&app, "/v1/feed?lang=ru").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "public, max-age=60");
    let feed = json_of(&body);
    assert_eq!(feed["format"], 1);
    assert_eq!(feed["lang"], "ru");
    assert_eq!(feed["versions"].as_array().unwrap().len(), 1);
    assert_eq!(feed["versions"][0]["title"], "Заголовок");
    assert_eq!(feed["versions"][0]["translated"], true);
    assert!(feed["versions"][0].get("blocks").is_none());
    assert_eq!(feed["videos"][0]["image"], "yt/abcdefghijk.jpg");
    assert_eq!(feed["streams"][0]["image"], "tw/somebody.jpg");
    assert_eq!(feed["latest"]["release"]["id"], "26.3");
    assert_eq!(feed["cycle"]["target"], "26.4");
    assert!(feed["tip"].is_string());

    let en = json_of(&get(&app, "/v1/feed?lang=en").await.2);
    assert_eq!(en["versions"][0]["title"], "Title j-26-4-snapshot-1");
    assert_eq!(en["versions"][0]["translated"], false);
    assert!(en["streams"].as_array().unwrap().is_empty());
    // A Russian channel is not in the English feed.
    assert!(en["videos"].as_array().unwrap().is_empty());
    // Any other language is English.
    let other = json_of(&get(&app, "/v1/feed?lang=%3Cscript%3E").await.2);
    assert_eq!(other["lang"], "en");
}

#[tokio::test]
async fn notes() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    let (status, headers, body) = get(&app, "/v1/notes/j-26-4-snapshot-1?lang=ru").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "public, max-age=600");
    let note = json_of(&body);
    assert_eq!(note["blocks"][0]["c"][0]["text"], "Текст");
    let en = json_of(&get(&app, "/v1/notes/j-26-4-snapshot-1").await.2);
    assert_eq!(en["blocks"][0]["c"][0]["text"], "Body");
    for bad in [
        "/v1/notes/j-missing",
        "/v1/notes/..%2Fstate",
        "/v1/notes/j-..%2F..%2Fstate",
        "/v1/notes/state.json",
        "/v1/notes/",
    ] {
        assert_eq!(get(&app, bad).await.0, StatusCode::NOT_FOUND, "{bad}");
    }
}

#[tokio::test]
async fn pictures() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    let name = format!("{}.jpg", "a".repeat(64));
    let (status, headers, body) = get(&app, &format!("/v1/img/{name}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["content-type"], "image/jpeg");
    assert!(
        headers["cache-control"]
            .to_str()
            .unwrap()
            .contains("immutable")
    );
    assert_eq!(&body[..2], &[0xFF, 0xD8]);
    for bad in [
        format!("/v1/img/{}.jpg", "b".repeat(64)),
        "/v1/img/..%2Fstate.json".to_owned(),
        "/v1/img/state.json".to_owned(),
        // A video no channel of ours has: never fetched.
        "/v1/yt/zzzzzzzzzzz.jpg".to_owned(),
        "/v1/yt/..%2F..%2Fx.jpg".to_owned(),
        // A stream not in the feed: never fetched.
        "/v1/tw/nobody_here.jpg".to_owned(),
        "/v1/tw/..%2Fstate.json".to_owned(),
        "/v1/other/x.jpg".to_owned(),
    ] {
        assert_eq!(get(&app, &bad).await.0, StatusCode::NOT_FOUND, "{bad}");
    }
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().fold(0u32, |n, &b| n << 8 | u32::from(b)) << (8 * (3 - chunk.len()));
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

#[tokio::test]
async fn player_pages() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    let (status, headers, body) = get(&app, "/play/yt/abcdefghijk?lang=ar").await;
    assert_eq!(status, StatusCode::OK);
    let csp = headers["content-security-policy"].to_str().unwrap();
    assert!(csp.starts_with("default-src 'none'"));
    assert_eq!(
        headers["referrer-policy"],
        "strict-origin-when-cross-origin"
    );
    let page = String::from_utf8(body).unwrap();
    assert!(page.contains("https://www.youtube-nocookie.com/embed/abcdefghijk"));
    assert!(page.contains("dir=\"rtl\""));
    // One script, the one whose hash the CSP names: it takes the spinner,
    // over the frame and hidden from screen readers, away once the frame loads.
    assert_eq!(page.matches("<script").count(), 1);
    let script = page
        .split("<script>")
        .nth(1)
        .and_then(|rest| rest.split("</script>").next())
        .unwrap();
    let digest = base64(&sha2::Sha256::digest(script.as_bytes()));
    let scripts = csp
        .split(';')
        .map(str::trim)
        .find(|directive| directive.starts_with("script-src"))
        .unwrap();
    assert_eq!(scripts, format!("script-src 'sha256-{digest}'"));
    let wait = page
        .find("<div class=\"wait\" aria-hidden=\"true\">")
        .unwrap();
    assert!(wait > page.find("<iframe").unwrap());
    let (status, _, body) = get(&app, "/play/twitch/somebody").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        String::from_utf8(body)
            .unwrap()
            .contains("parent=feed.lapi.sh")
    );
    for bad in [
        "/play/yt/abc",
        "/play/yt/%22%3E%3Cscript%3Eal",
        "/play/twitch/%22%3E%3Cscript%3E",
        "/play/twitch/Some_User",
    ] {
        assert_eq!(get(&app, bad).await.0, StatusCode::NOT_FOUND, "{bad}");
    }
}

fn resolve_request(body: &str, ip: &str) -> Request<Body> {
    Request::post("/v1/channels/resolve")
        .header("content-type", "application/json")
        .header("x-real-ip", ip)
        .body(Body::from(body.to_owned()))
        .unwrap()
}

#[tokio::test]
async fn channel_lookups_refuse_junk() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    for (body, code) in [
        ("{\"input\":\"https://evil.example/@x\"}", "bad_input"),
        ("{\"input\":\"\"}", "bad_input"),
        ("{\"input\":5}", "bad_request"),
        ("not json", "bad_request"),
        ("{}", "bad_request"),
    ] {
        let (status, _, answer) = call(&app, resolve_request(body, "203.0.113.1")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(json_of(&answer)["error"], code, "{body}");
    }
    let big = format!("{{\"input\":\"{}\"}}", "a".repeat(10_000));
    let (status, _, _) = call(&app, resolve_request(&big, "203.0.113.1")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let wrong_type = Request::post("/v1/channels/resolve")
        .header("content-type", "text/plain")
        .body(Body::from("{\"input\":\"@Grian\"}"))
        .unwrap();
    assert_eq!(call(&app, wrong_type).await.0, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn own_videos_refuse_junk() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    let ok = get(&app, "/v1/videos?channels=").await;
    assert_eq!(ok.0, StatusCode::OK);
    assert_eq!(json_of(&ok.2)["videos"], json!([]));
    for bad in [
        "/v1/videos?channels=nope".to_owned(),
        "/v1/videos?channels=UC1sELGmy5jp5fQUugmuYlXQ,..%2F".to_owned(),
        format!(
            "/v1/videos?channels={}",
            (0..31)
                .map(|i| format!("UC{i:022}"))
                .collect::<Vec<_>>()
                .join(",")
        ),
    ] {
        assert_eq!(get(&app, &bad).await.0, StatusCode::BAD_REQUEST, "{bad}");
    }
}

#[test]
fn limiter_counts_per_address() {
    let limiter = api::Limiter::default();
    let minute = std::time::Duration::from_secs(60);
    for _ in 0..3 {
        assert!(limiter.allow("a", "x", 3, minute));
    }
    assert!(!limiter.allow("a", "x", 3, minute));
    assert!(limiter.allow("b", "x", 3, minute));
    assert!(limiter.allow("a", "y", 3, minute));
    assert!(limiter.allow("a", "x", 3, std::time::Duration::ZERO));
}

#[tokio::test]
async fn health_and_unknown_paths() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    let (status, _, body) = get(&app, "/v1/health").await;
    assert_eq!(status, StatusCode::OK);
    let health = json_of(&body);
    assert_eq!(health["ok"], true);
    assert_eq!(health["service"], "lapi-feed");
    assert_eq!(health["counts"]["patchNotes"], 2);
    let (status, _, body) = get(&app, "/etc/passwd").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json_of(&body)["error"], "not_found");
}

#[test]
fn a_broken_state_is_started_afresh() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("state.json"), b"{not json").unwrap();
    let feed = feed::Feed::open(
        dir.path(),
        Editorial::load(crate::EDITORIAL).unwrap(),
        crate::net::client().unwrap(),
        None,
        None,
    )
    .unwrap();
    assert_eq!(feed.counts()["patchNotes"], 0);
    assert_eq!(feed.build("en")["versions"], json!([]));
}

#[test]
fn unused_pictures_are_kept_for_a_while() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    let used = app.feed.used_images();
    assert!(used.contains(&format!("img/{}.jpg", "a".repeat(64))));
    let stray = dir
        .path()
        .join("img")
        .join(format!("{}.jpg", "c".repeat(64)));
    std::fs::write(&stray, b"x").unwrap();
    // New files stay even when unused.
    assert_eq!(app.feed.images.clean(&used), 0);
    assert!(stray.exists());
}
