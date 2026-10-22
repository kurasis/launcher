#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};

use super::model::*;
use super::player::{Target, navigation_allowed};
use super::*;

const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn note_card_json(id: &str) -> Value {
    json!({
        "id": id,
        "title": "Minecraft 26.4 Snapshot 4",
        "date": "2026-10-14T15:00:00Z",
        "summary": "New mobs and fixes.",
        "translated": false,
        "version": "26.4-snapshot-4",
        "kind": "snapshot",
        "image": format!("img/{SHA}.jpg"),
        "imageWidth": 1280,
        "imageHeight": 720,
    })
}

fn sample(lang: &str) -> Value {
    json!({
        "format": 1,
        "generated": "2026-10-19T12:00:00Z",
        "lang": lang,
        "versions": [note_card_json("j-26-4-snapshot-4")],
        "news": [{
            "id": "n-a-new-mob",
            "title": "A new mob",
            "date": "2026-10-10T10:00:00+02:00",
            "summary": "Meet it.",
            "translated": true,
            "url": "https://www.minecraft.net/en-us/article/a-new-mob",
        }],
        "videos": [{
            "id": "dQw4w9WgXcQ",
            "channelId": "UCabcdefghijklmnopqrstuv",
            "channel": "Channel",
            "title": "A video",
            "date": "2026-10-18T09:00:00Z",
            "short": true,
            "image": "yt/dQw4w9WgXcQ.jpg",
            "imageTall": "ys/dQw4w9WgXcQ.jpg",
            "avatar": "ya/UCabcdefghijklmnopqrstuv.jpg",
        }],
        "streams": [{
            "login": "some_streamer",
            "name": "Some Streamer",
            "title": "Building",
            "viewers": 1234,
            "started": "2026-10-19T10:00:00Z",
            "image": "tw/some_streamer.jpg",
            "avatar": "ta/some_streamer.jpg",
        }],
        "cycle": {
            "target": "26.4", "snapshots": 4, "pre": 0, "rc": 0, "released": false,
            "stage": "snapshot", "latest": "26.4-snapshot-4", "date": "2026-10-14T15:00:00Z",
        },
        "latest": {
            "release": {"id": "26.3", "kind": "release", "date": "2026-09-01T10:00:00Z"},
            "snapshot": {"id": "26.4-snapshot-4", "kind": "snapshot", "date": "2026-10-14T15:00:00Z"},
        },
        "events": [{
            "id": "build-contest", "title": "Build contest",
            "starts": "2026-10-25T10:00:00Z", "ends": "2026-10-26T10:00:00Z",
            "url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        }],
        "announcements": [{"id": "welcome", "title": "Welcome", "text": "Hello", "url": "https://feed.lapi.sh/"}],
        "tip": "Press F3 for coordinates.",
        "fetched": {"mojang": "2026-10-19T11:59:00Z", "evil": "2026-10-19T11:59:00Z"},
    })
}

#[test]
fn a_whole_feed_is_read() {
    let feed = feed(&sample("ru"), Lang::Ru).unwrap();
    assert_eq!(feed.lang, Lang::Ru);
    assert_eq!(feed.versions.len(), 1);
    assert_eq!(feed.versions[0].kind, Some(VersionKind::Snapshot));
    assert_eq!(
        feed.versions[0].image.as_deref(),
        Some(format!("img/{SHA}.jpg").as_str())
    );
    // Any RFC 3339 date becomes UTC with a Z.
    assert_eq!(feed.news[0].date, "2026-10-10T08:00:00Z");
    assert!(feed.news[0].translated);
    assert_eq!(
        (feed.versions[0].image_width, feed.versions[0].image_height),
        (Some(1280), Some(720))
    );
    assert_eq!(feed.videos[0].image.as_deref(), Some("yt/dQw4w9WgXcQ.jpg"));
    assert_eq!(
        feed.videos[0].image_tall.as_deref(),
        Some("ys/dQw4w9WgXcQ.jpg")
    );
    assert_eq!(
        feed.videos[0].avatar.as_deref(),
        Some("ya/UCabcdefghijklmnopqrstuv.jpg")
    );
    assert!(feed.videos[0].short);
    assert_eq!(feed.streams[0].viewers, 1234);
    assert_eq!(
        feed.streams[0].avatar.as_deref(),
        Some("ta/some_streamer.jpg")
    );
    assert_eq!(feed.cycle.as_ref().unwrap().snapshots, 4);
    assert_eq!(feed.latest.release.as_ref().unwrap().id, "26.3");
    assert_eq!(feed.events.len(), 1);
    assert_eq!(
        feed.announcements[0].url.as_deref(),
        Some("https://feed.lapi.sh/")
    );
    assert_eq!(feed.tip.as_deref(), Some("Press F3 for coordinates."));
    assert_eq!(feed.fetched.keys().collect::<Vec<_>>(), ["mojang"]);
}

#[test]
fn not_a_feed_is_refused_whole() {
    for junk in [
        json!(null),
        json!([]),
        json!("feed"),
        json!({"lang": "en"}),
        json!({"format": 0, "lang": "en", "generated": "2026-10-19T12:00:00Z"}),
        json!({"format": "1", "lang": "en", "generated": "2026-10-19T12:00:00Z"}),
        json!({"format": 1, "lang": "de", "generated": "2026-10-19T12:00:00Z"}),
        json!({"format": 1, "lang": "en", "generated": "yesterday"}),
    ] {
        assert!(feed(&junk, Lang::En).is_none(), "{junk}");
    }
    // A feed in another language than asked.
    assert!(feed(&sample("ar"), Lang::En).is_none());
    // A newer format is read for what it still has.
    let mut newer = sample("en");
    newer["format"] = json!(2);
    newer["somethingNew"] = json!({"x": 1});
    assert_eq!(feed(&newer, Lang::En).unwrap().versions.len(), 1);
}

#[test]
fn bad_items_are_dropped_and_the_rest_stays() {
    let mut value = sample("en");
    let good = note_card_json("j-good");
    let mut bad_ids = Vec::new();
    for id in [
        "J-UPPER",
        "j-",
        "x-other",
        "n-news-in-versions",
        "j-../../etc/passwd",
        "j-<script>",
        "j-a b",
        &format!("j-{}", "a".repeat(79)),
    ] {
        bad_ids.push(note_card_json(id));
    }
    let mut versions = vec![good];
    versions.extend(bad_ids);
    versions.push(json!({"id": "j-no-title", "date": "2026-10-14T15:00:00Z"}));
    versions.push(json!({"id": "j-bad-date", "title": "x", "date": "14.10.2026"}));
    versions.push(json!("j-a-string"));
    versions.push(json!(null));
    value["versions"] = json!(versions);
    value["videos"] = json!([
        {"id": "short", "channelId": "UCabcdefghijklmnopqrstuv", "channel": "c", "title": "t", "date": "2026-10-18T09:00:00Z"},
        {"id": "dQw4w9WgXc!", "channelId": "UCabcdefghijklmnopqrstuv", "channel": "c", "title": "t", "date": "2026-10-18T09:00:00Z"},
        {"id": "dQw4w9WgXcQ", "channelId": "UXabcdefghijklmnopqrstuv", "channel": "c", "title": "t", "date": "2026-10-18T09:00:00Z"},
        {"id": "dQw4w9WgXcQ", "channelId": "UCabcdefghijklmnopqrstuv", "channel": "c", "title": "t",
         "date": "2026-10-18T09:00:00Z", "image": "yt/aaaaaaaaaaa.jpg"},
    ]);
    value["streams"] = json!([
        {"login": "Upper", "name": "n", "viewers": 1, "started": "2026-10-19T10:00:00Z"},
        {"login": "ab", "name": "n", "viewers": 1, "started": "2026-10-19T10:00:00Z"},
        {"login": "fine_one", "name": "n", "viewers": -1, "started": "2026-10-19T10:00:00Z"},
        {"login": "fine_two", "name": "n", "viewers": 1e12, "started": "2026-10-19T10:00:00Z"},
        {"login": "fine_three", "name": "n", "viewers": 5, "started": "2026-10-19T10:00:00Z", "image": "tw/other.jpg"},
    ]);
    value["cycle"] = json!({"target": "26.4", "snapshots": "4"});
    value["latest"] = json!({
        "release": {"id": "26.4-snapshot-4", "kind": "snapshot", "date": "2026-10-14T15:00:00Z"},
        "snapshot": {"id": "26.3", "kind": "release", "date": "2026-09-01T10:00:00Z"},
    });
    let feed = feed(&value, Lang::En).unwrap();
    assert_eq!(
        feed.versions
            .iter()
            .map(|v| v.id.as_str())
            .collect::<Vec<_>>(),
        ["j-good"]
    );
    // A preview that is not the video's own is not shown.
    assert_eq!(feed.videos.len(), 1);
    assert_eq!(feed.videos[0].image, None);
    assert_eq!(feed.streams.len(), 1);
    assert_eq!(feed.streams[0].login, "fine_three");
    assert_eq!(feed.streams[0].image, None);
    assert!(feed.cycle.is_none());
    assert_eq!(feed.latest, Latest::default());
    // The news card stays.
    assert_eq!(feed.news.len(), 1);
}

#[test]
fn picture_sizes_and_channel_pictures_are_checked() {
    let mut value = sample("en");
    let card = |change: Value| {
        let mut card = note_card_json("j-sized");
        for (key, v) in change.as_object().unwrap() {
            card[key] = v.clone();
        }
        card
    };
    value["versions"] = json!([
        card(json!({"imageWidth": 1280, "imageHeight": null})),
        card(json!({"imageWidth": 0})),
        card(json!({"imageHeight": 9000})),
        card(json!({"imageWidth": "1280"})),
        card(json!({"imageWidth": 12.5})),
        card(json!({"imageWidth": -1})),
        card(json!({"image": null})),
        card(json!({"image": "img/../../secret.jpg"})),
        card(json!({"imageWidth": 8192, "imageHeight": 1})),
    ]);
    let video = |change: Value| {
        let mut video = sample("en")["videos"][0].clone();
        for (key, v) in change.as_object().unwrap() {
            video[key] = v.clone();
        }
        video
    };
    value["videos"] = json!([
        video(json!({"short": false})),
        video(json!({"imageTall": "ys/aaaaaaaaaaa.jpg"})),
        video(json!({"imageTall": "yt/dQw4w9WgXcQ.jpg"})),
        video(json!({"avatar": "ya/UCzzzzzzzzzzzzzzzzzzzzzz.jpg"})),
        video(json!({"avatar": "ya/../../secret.jpg"})),
        video(json!({"avatar": "ta/dQw4w9WgXcQ.jpg"})),
        video(json!({"avatar": 5})),
    ]);
    value["streams"] = json!([
        {"login": "one_streamer", "name": "n", "viewers": 1, "started": "2026-10-19T10:00:00Z",
         "avatar": "ta/other_streamer.jpg"},
        {"login": "two_streamer", "name": "n", "viewers": 1, "started": "2026-10-19T10:00:00Z",
         "avatar": "tw/two_streamer.jpg"},
    ]);
    let feed = feed(&value, Lang::En).unwrap();
    let sizes: Vec<_> = feed
        .versions
        .iter()
        .map(|v| (v.image_width, v.image_height))
        .collect();
    assert_eq!(
        sizes,
        [
            (None, None),
            (None, None),
            (None, None),
            (None, None),
            (None, None),
            (None, None),
            (None, None),
            (None, None),
            (Some(8192), Some(1)),
        ]
    );
    // A video that is not a Short has no upright preview; pictures of other
    // videos or channels are not shown.
    let shown: Vec<_> = feed
        .videos
        .iter()
        .map(|v| (v.image_tall.is_some(), v.avatar.is_some()))
        .collect();
    assert_eq!(
        shown,
        [
            (false, true),
            (false, true),
            (false, true),
            (true, false),
            (true, false),
            (true, false),
            (true, false),
        ]
    );
    assert!(feed.streams.iter().all(|s| s.avatar.is_none()));
}

#[test]
fn pages_of_the_endless_list_are_read_strictly() {
    let all = crate::storage::models::FeedBlock::ALL;
    let video = json!({
        "id": "dQw4w9WgXcQ",
        "channelId": "UCabcdefghijklmnopqrstuv",
        "channel": "Channel",
        "title": "A video",
        "date": "2026-10-18T10:00:00Z",
        "short": false,
        "image": "yt/dQw4w9WgXcQ.jpg",
    });
    let stream = json!({
        "login": "somebody",
        "name": "Somebody",
        "title": "Building",
        "viewers": 12,
        "started": "2026-10-19T10:00:00Z",
        "image": "tw/somebody.jpg",
    });
    let value = json!({
        "format": 1,
        "items": [
            {"stream": stream},
            {"video": video},
            {"note": note_card_json("j-26-4-snapshot-4")},
            {"note": {"id": "../../etc", "title": "x", "date": "2026-10-01T00:00:00Z"}},
            {"video": {"id": "<script>", "title": "x"}},
            {"something": {"id": "x"}},
            "junk",
        ],
        "next": "2026-10-14T15:00:00Z~j-26-4-snapshot-4",
    });
    let page = page(&value, &all).unwrap();
    assert_eq!(page.items.len(), 3);
    assert!(matches!(&page.items[0], FeedItem::Stream { stream } if stream.login == "somebody"));
    assert!(matches!(&page.items[1], FeedItem::Video { video } if video.id == "dQw4w9WgXcQ"));
    assert!(matches!(&page.items[2], FeedItem::Note { note } if note.id == "j-26-4-snapshot-4"));
    assert_eq!(
        page.next.as_deref(),
        Some("2026-10-14T15:00:00Z~j-26-4-snapshot-4")
    );
    let shown = serde_json::to_value(&page).unwrap();
    assert_eq!(shown["items"][0]["type"], "stream");
    assert_eq!(shown["items"][2]["note"]["id"], "j-26-4-snapshot-4");

    // Only the parts asked for, whatever the service sends.
    let videos = page_of(&value, &[crate::storage::models::FeedBlock::Videos]);
    assert_eq!(videos.items.len(), 1);

    // A bad cursor or no format refuses the page; no next is the end.
    let mut bad = value.clone();
    bad["next"] = json!("../../state");
    assert!(super::model::page(&bad, &all).is_none());
    bad["next"] = json!(7);
    assert!(super::model::page(&bad, &all).is_none());
    let mut end = value.clone();
    end["next"] = Value::Null;
    assert_eq!(page_of(&end, &all).next, None);
    assert!(super::model::page(&json!({"items": []}), &all).is_none());
    assert!(super::model::page(&json!({"format": 1}), &all).is_none());

    // Never more items than a page holds.
    let many = json!({
        "format": 1,
        "items": vec![json!({"video": video}); 500],
    });
    assert_eq!(page_of(&many, &all).items.len(), 40);
}

fn page_of(value: &Value, kinds: &[crate::storage::models::FeedBlock]) -> FeedPage {
    super::model::page(value, kinds).unwrap()
}

#[test]
fn cursors_follow_the_service() {
    for good in [
        "2026-10-14T15:00:00Z~j-26-4-snapshot-4",
        "2026-10-14T15:00:00Z~n-a-new-mob",
        "2026-10-14T15:00:00Z~yt:dQw4w9WgXcQ",
        "2026-10-14T15:00:00Z~tw:somebody",
    ] {
        assert!(cursor(good), "{good}");
    }
    for bad in [
        "",
        "~",
        "2026-10-14T15:00:00Z",
        "2026-10-14T15:00:00+02:00~j-26-4",
        "2026-10-14~j-26-4",
        "2026-10-14T15:00:00Z~yt:../x",
        "2026-10-14T15:00:00Z~tw:Some Body",
        "2026-10-14T15:00:00Z~x:abc",
        "2026-10-14T15:00:00Z~j-26-4&kinds=all",
        &format!("2026-10-14T15:00:00Z~j-{}", "a".repeat(120)),
    ] {
        assert!(!cursor(bad), "{bad}");
    }
}

#[test]
fn links_and_pictures_must_be_ours_to_open() {
    let mut value = sample("en");
    let mut news = Vec::new();
    for (i, url) in [
        "javascript:alert(1)",
        "http://www.minecraft.net/en-us/article/x",
        "https://www.minecraft.net.evil.example/x",
        "https://evil.example/?https://www.minecraft.net",
        "https://user:pw@www.minecraft.net/x",
        "https://www.minecraft.net:8443/x",
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        "data:text/html,<script>alert(1)</script>",
        "file:///etc/passwd",
    ]
    .into_iter()
    .enumerate()
    {
        news.push(json!({
            "id": format!("n-item-{i}"), "title": "t", "date": "2026-10-10T10:00:00Z",
            "url": url, "image": "img/../../secret.jpg",
        }));
    }
    value["news"] = json!(news);
    value["events"] = json!([{
        "id": "e", "title": "t", "starts": "2026-10-25T10:00:00Z",
        "ends": "2026-10-26T10:00:00Z", "url": "https://evil.lapi.sh.example/",
    }]);
    let feed = feed(&value, Lang::En).unwrap();
    assert_eq!(feed.news.len(), 9);
    for card in &feed.news {
        // Only articles on www.minecraft.net; a YouTube address is no article.
        assert_eq!(card.url, None, "{}", card.id);
        assert_eq!(card.image, None);
    }
    assert_eq!(feed.events[0].url, None);
}

#[test]
fn feed_links_lead_only_to_the_feeds_sites() {
    for good in [
        "https://www.minecraft.net/en-us/article/x",
        "https://help.minecraft.net/hc/x",
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        "https://youtu.be/dQw4w9WgXcQ",
        "https://www.twitch.tv/some_streamer",
        "https://feed.lapi.sh/",
    ] {
        assert!(link(good), "{good}");
    }
    // The launcher may open these elsewhere, but never from the feed.
    for bad in [
        "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize?client_id=x",
        "https://account.microsoft.com/family",
        "https://www.xbox.com/en-US/live",
        "https://github.com/kurasis/launcher",
        "https://bugs.mojang.com/browse/MC-1",
        &format!("https://www.minecraft.net/{}", "a".repeat(2000)),
    ] {
        assert!(!link(bad), "{bad}");
    }
}

#[test]
fn texts_are_cleaned_and_bounded() {
    let mut value = sample("en");
    let mut card = note_card_json("j-long");
    card["title"] = json!(format!(
        "\u{202e}evil\u{0007}\u{2066} title {}",
        "x".repeat(5000)
    ));
    card["summary"] = json!("   ");
    value["versions"] = json!([card]);
    value["tip"] = json!("\n\t ");
    let mut videos = Vec::new();
    for i in 0..500 {
        videos.push(json!({
            "id": format!("aaaaaaa{i:04}"), "channelId": "UCabcdefghijklmnopqrstuv",
            "channel": "c", "title": "t", "date": "2026-10-18T09:00:00Z",
        }));
    }
    value["videos"] = json!(videos);
    let feed = feed(&value, Lang::En).unwrap();
    let title = &feed.versions[0].title;
    assert!(title.starts_with("evil  title xxx"), "{title}");
    assert!(title.chars().count() <= 301);
    assert!(title.ends_with('…'));
    assert!(
        !title
            .chars()
            .any(|c| c.is_control() || ('\u{202a}'..='\u{2069}').contains(&c))
    );
    assert_eq!(feed.versions[0].summary, "");
    assert_eq!(feed.tip, None);
    assert_eq!(feed.videos.len(), 60);
}

#[test]
fn ids_and_picture_names_follow_the_service() {
    assert!(note_id("j-26-4-snapshot-4") && note_id("n-x"));
    for bad in ["j-", "jj-x", "j-X", "n-a_b", "j-a/b", "", "j-ä"] {
        assert!(!note_id(bad), "{bad}");
    }
    assert!(note_id(&format!("j-{}", "a".repeat(78))));
    assert!(!note_id(&format!("j-{}", "a".repeat(79))));
    assert!(video_id("dQw4w9WgXcQ") && video_id("a-_b-_c-_d-"));
    assert!(!video_id("dQw4w9WgXc") && !video_id("dQw4w9WgXc/") && !video_id("dQw4w9WgXcQQ"));
    assert!(twitch_login("abc") && twitch_login("a_b_1"));
    assert!(!twitch_login("ab") && !twitch_login("ABC") && !twitch_login(&"a".repeat(26)));
    assert!(channel_id("UCabcdefghijklmnopqrstuv"));
    assert!(!channel_id("UCabcdefghijklmnopqrstu") && !channel_id("UCabcdefghijklmnopqrst/v"));
    assert!(picture_name(PictureKind::Img, &format!("{SHA}.jpg")));
    for bad in [
        format!("{}.jpg", SHA.to_uppercase()),
        format!("{SHA}.png"),
        format!("{SHA}.jpg.exe"),
        format!("{}.jpg", &SHA[1..]),
        "../x.jpg".to_owned(),
        ".jpg".to_owned(),
    ] {
        assert!(!picture_name(PictureKind::Img, &bad), "{bad}");
    }
    assert!(picture_name(PictureKind::Yt, "dQw4w9WgXcQ.jpg"));
    assert!(!picture_name(PictureKind::Yt, "dQw4w9WgXc.jpg"));
    assert!(picture_name(PictureKind::Tw, "some_streamer.jpg"));
    assert!(!picture_name(PictureKind::Tw, "Some.jpg"));
    assert!(picture_name(PictureKind::Ys, "dQw4w9WgXcQ.jpg"));
    assert!(!picture_name(PictureKind::Ys, "dQw4w9WgXcQ.png"));
    assert!(picture_name(
        PictureKind::Ya,
        "UCabcdefghijklmnopqrstuv.jpg"
    ));
    assert!(!picture_name(PictureKind::Ya, "dQw4w9WgXcQ.jpg"));
    assert!(!picture_name(
        PictureKind::Ya,
        "UCabcdefghijklmnopqrst/v.jpg"
    ));
    assert!(picture_name(PictureKind::Ta, "some_streamer.jpg"));
    assert!(!picture_name(PictureKind::Ta, "../x.jpg"));
    assert_eq!(
        picture_path("yt/dQw4w9WgXcQ.jpg"),
        Some((PictureKind::Yt, "dQw4w9WgXcQ.jpg"))
    );
    assert_eq!(picture_path("tw/dQw4w9WgXcQ.jpg"), None);
    assert_eq!(picture_path("img/x/y.jpg"), None);
}

#[test]
fn protocol_paths_name_only_pictures() {
    assert_eq!(
        parse_picture_request(&format!("/img/{SHA}.jpg")),
        Some((PictureKind::Img, format!("{SHA}.jpg")))
    );
    assert_eq!(
        parse_picture_request("/tw/some_streamer.jpg"),
        Some((PictureKind::Tw, "some_streamer.jpg".to_owned()))
    );
    assert_eq!(
        parse_picture_request("/ys/dQw4w9WgXcQ.jpg"),
        Some((PictureKind::Ys, "dQw4w9WgXcQ.jpg".to_owned()))
    );
    assert_eq!(
        parse_picture_request("/ya/UCabcdefghijklmnopqrstuv.jpg"),
        Some((PictureKind::Ya, "UCabcdefghijklmnopqrstuv.jpg".to_owned()))
    );
    assert_eq!(
        parse_picture_request("/ta/some_streamer.jpg"),
        Some((PictureKind::Ta, "some_streamer.jpg".to_owned()))
    );
    assert_eq!(parse_picture_request("/ya/some_streamer.jpg"), None);
    for bad in [
        "",
        "/",
        "img/x.jpg",
        "/img/",
        "/img/../../settings.json",
        "/img/%2e%2e%2fsettings.json",
        "/yt/dQw4w9WgXcQ.jpg/x",
        "/yt//dQw4w9WgXcQ.jpg",
        "/other/dQw4w9WgXcQ.jpg",
        "/yt/dQw4w9WgXcQ.png",
        "/yt/dQw4w9WgXcQ.jpg?x=1",
        "//evil.example/yt/dQw4w9WgXcQ.jpg",
        "/tw/Some.jpg",
    ] {
        assert_eq!(parse_picture_request(bad), None, "{bad}");
    }
}

#[test]
fn only_whole_jpegs_are_served() {
    let mut good = vec![0xff, 0xd8, 0xff, 0xe0];
    good.resize(200, 0);
    good.extend([0xff, 0xd9]);
    assert!(jpeg(&good));
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.resize(300, 0);
    assert!(!jpeg(&png));
    let mut html = b"<html><script>".to_vec();
    html.resize(300, b' ');
    assert!(!jpeg(&html));
    assert!(!jpeg(&good[..good.len() - 1]));
    assert!(!jpeg(&[0xff, 0xd8, 0xff, 0xd9]));
}

fn note_json(blocks: Value) -> Value {
    let mut note = note_card_json("j-26-4-snapshot-4");
    note["blocks"] = blocks;
    note
}

#[test]
fn notes_keep_only_known_blocks() {
    let note = note(
        &note_json(json!([
            {"t": "h", "level": 2, "c": [{"text": "New"}]},
            {"t": "h", "level": 9, "c": [{"text": "Too deep a heading"}]},
            {"t": "p", "c": [
                {"text": "<img src=x onerror=alert(1)>"},
                {"text": "bold", "b": true},
                {"text": "a link", "href": "https://www.minecraft.net/en-us"},
                {"text": "a bad link", "href": "javascript:alert(1)"},
                {"text": "elsewhere", "href": "https://bugs.example/MC-1"},
                {"text": "\u{202e}flip"},
                {"x": "no text"},
            ]},
            {"t": "script", "c": [{"text": "alert(1)"}]},
            {"t": "html", "html": "<b>x</b>"},
            {"t": "img", "src": format!("img/{SHA}.jpg"), "alt": "A picture"},
            {"t": "img", "src": "https://evil.example/x.jpg", "alt": "remote"},
            {"t": "ul", "items": [[{"t": "p", "c": [{"text": "one"}]}], [], "junk"]},
            {"t": "ol", "items": []},
            {"t": "p", "c": []},
        ])),
        "j-26-4-snapshot-4",
        Lang::En,
    )
    .unwrap();
    let json = serde_json::to_value(&note.blocks).unwrap();
    assert_eq!(
        json,
        json!([
            {"t": "h", "level": 2, "c": [{"text": "New"}]},
            {"t": "p", "c": [
                {"text": "<img src=x onerror=alert(1)>"},
                {"text": "bold", "b": true},
                {"text": "a link", "href": "https://www.minecraft.net/en-us"},
                {"text": "a bad link"},
                {"text": "elsewhere"},
                {"text": "flip"},
            ]},
            {"t": "img", "src": format!("img/{SHA}.jpg"), "alt": "A picture"},
            {"t": "ul", "items": [[{"t": "p", "c": [{"text": "one"}]}]]},
        ])
    );
    // The card the note carries is checked like any other.
    assert_eq!(note.card.kind, Some(VersionKind::Snapshot));
    let serialized = serde_json::to_value(&note).unwrap();
    assert_eq!(serialized["id"], "j-26-4-snapshot-4");
    assert_eq!(serialized["lang"], "en");
}

#[test]
fn notes_are_bounded() {
    // Lists inside lists, far deeper than allowed.
    let mut deep = json!({"t": "p", "c": [{"text": "bottom"}]});
    for _ in 0..100 {
        deep = json!({"t": "ul", "items": [[deep]]});
    }
    let read = note(&note_json(json!([deep])), "j-26-4-snapshot-4", Lang::En).unwrap();
    let mut depth = 0;
    let mut block = read.blocks.first();
    while let Some(Block::Ul { items }) = block {
        depth += 1;
        block = items.first().and_then(|i| i.first());
    }
    assert!(depth <= 8, "{depth}");

    // Many blocks and much text.
    let many: Vec<Value> = (0..20_000)
        .map(|_| json!({"t": "p", "c": [{"text": "x".repeat(100)}]}))
        .collect();
    let read = note(&note_json(json!(many)), "j-26-4-snapshot-4", Lang::En).unwrap();
    assert!(read.blocks.len() <= 6000);
    let huge = json!([{"t": "p", "c": [{"text": "y".repeat(100_000)}]}]);
    let read = note(&note_json(huge), "j-26-4-snapshot-4", Lang::En).unwrap();
    let Block::P { c } = &read.blocks[0] else {
        panic!("a paragraph")
    };
    assert_eq!(c[0].text.chars().count(), 20_000);

    // Another note's answer is not this note.
    assert!(note(&note_json(json!([])), "j-other", Lang::En).is_none());
}

#[test]
fn own_channels_and_videos_are_read_strictly() {
    assert_eq!(
        own_channel(&json!({"channelId": "UCabcdefghijklmnopqrstuv", "title": " Mine "})),
        Some(OwnChannel {
            channel_id: "UCabcdefghijklmnopqrstuv".into(),
            title: "Mine".into()
        })
    );
    for junk in [
        json!({"channelId": "UC123", "title": "x"}),
        json!({"channelId": "UCabcdefghijklmnopqrstuv"}),
        json!({"error": "not_found"}),
        json!([]),
    ] {
        assert_eq!(own_channel(&junk), None, "{junk}");
    }
    assert_eq!(videos(&json!({"videos": []})), Some(Vec::new()));
    assert_eq!(videos(&json!({"error": "too_many"})), None);
    assert!(channel_input("@handle").is_some());
    assert_eq!(
        channel_input("  https://www.youtube.com/@x  "),
        Some("https://www.youtube.com/@x")
    );
    for bad in ["", "   ", "a\nb", "a\u{0}b"] {
        assert_eq!(channel_input(bad), None, "{bad:?}");
    }
    assert_eq!(channel_input(&"a".repeat(201)), None);
    assert!(channel_input(&"a".repeat(200)).is_some());
}

fn root() -> (tempfile::TempDir, DataRoot) {
    let dir = tempfile::tempdir().unwrap();
    let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
    (dir, root)
}

#[test]
fn own_channel_list_is_bounded_and_checked_on_disk() {
    let (_dir, root) = root();
    assert!(own_channels(&root).is_empty());
    let mut stored = Vec::new();
    for i in 0..40 {
        stored.push(
            json!({"channelId": format!("UCabcdefghijklmnopqrs{i:03}"), "title": format!("C{i}")}),
        );
    }
    stored.insert(
        1,
        json!({"channelId": "UCabcdefghijklmnopqrs000", "title": "duplicate"}),
    );
    stored.insert(2, json!({"channelId": "../../x", "title": "bad"}));
    stored.insert(3, json!("UCabcdefghijklmnopqrs999"));
    std::fs::write(
        root.feed_channels(),
        serde_json::to_vec(&json!({ "channels": stored })).unwrap(),
    )
    .unwrap();
    let channels = own_channels(&root);
    assert_eq!(channels.len(), MAX_OWN_CHANNELS);
    assert_eq!(channels[0].title, "C0");
    assert_eq!(channels[1].title, "C1");
    let after = remove_own_channel(&root, "UCabcdefghijklmnopqrs000").unwrap();
    assert_eq!(after.len(), MAX_OWN_CHANNELS - 1);
    assert_eq!(own_channels(&root), after);
    // Damaged or oversized files read as no channels.
    std::fs::write(root.feed_channels(), b"{not json").unwrap();
    assert!(own_channels(&root).is_empty());
    std::fs::write(root.feed_channels(), vec![b' '; 200_000]).unwrap();
    assert!(own_channels(&root).is_empty());
}

#[test]
fn own_twitch_channels_are_read_strictly_and_kept_beside_youtube() {
    assert_eq!(
        own_streamer(&json!({"login": "ph1lza", "name": " Ph1LzA "})),
        Some(OwnStreamer {
            login: "ph1lza".into(),
            name: "Ph1LzA".into()
        })
    );
    for junk in [
        json!({"login": "Ph1LzA", "name": "x"}),
        json!({"login": "../x", "name": "x"}),
        json!({"login": "ab", "name": "x"}),
        json!({"login": "ph1lza"}),
        json!({"error": "not_found"}),
    ] {
        assert_eq!(own_streamer(&junk), None, "{junk}");
    }
    let live = json!({"streams": [
        {"login": "ph1lza", "name": "Ph1LzA", "title": "Hardcore", "viewers": 10,
         "started": "2026-10-20T10:00:00Z", "image": "tw/ph1lza.jpg"},
        {"login": "bad login", "name": "x", "viewers": 1, "started": "2026-10-20T10:00:00Z"},
        {"login": "docm77", "name": "docm77", "viewers": 5,
         "started": "2026-10-20T10:00:00Z", "image": "tw/someone.jpg"},
    ]});
    let read = streams(&live).unwrap();
    assert_eq!(read.len(), 2);
    assert_eq!(read[0].image.as_deref(), Some("tw/ph1lza.jpg"));
    // A picture of another channel is not taken.
    assert_eq!(read[1].image, None);
    assert_eq!(streams(&json!({"error": "too_many"})), None);

    let (_dir, root) = root();
    std::fs::write(
        root.feed_channels(),
        serde_json::to_vec(&json!({
            "channels": [{"channelId": "UCabcdefghijklmnopqrstuv", "title": "Mine"}],
            "twitch": [
                {"login": "ph1lza", "name": "Ph1LzA"},
                {"login": "ph1lza", "name": "again"},
                {"login": "<script>", "name": "x"},
                {"login": "docm77", "name": "docm77"},
            ],
        }))
        .unwrap(),
    )
    .unwrap();
    let kept: Vec<String> = own_streamers(&root).into_iter().map(|s| s.login).collect();
    assert_eq!(kept, ["ph1lza", "docm77"]);
    // Removing a Twitch channel keeps the YouTube ones, and the other way.
    let after = remove_own_streamer(&root, "ph1lza").unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(own_channels(&root).len(), 1);
    remove_own_channel(&root, "UCabcdefghijklmnopqrstuv").unwrap();
    assert_eq!(own_streamers(&root), after);
    assert!(own_channels(&root).is_empty());
}

#[test]
fn channel_marks_are_read_strictly() {
    assert_eq!(
        channel_mark(&json!({"kind": "twitch", "id": "ph1lza", "name": " Ph1LzA "})),
        Some(ChannelMark {
            kind: MarkKind::Twitch,
            id: "ph1lza".into(),
            name: "Ph1LzA".into()
        })
    );
    assert!(
        channel_mark(&json!({"kind": "youtube", "id": "UCabcdefghijklmnopqrstuv", "name": "C"}))
            .is_some()
    );
    for junk in [
        json!({"kind": "youtube", "id": "ph1lza", "name": "x"}),
        json!({"kind": "twitch", "id": "UCabcdefghijklmnopqrstuv", "name": "x"}),
        json!({"kind": "twitch", "id": "../x", "name": "x"}),
        json!({"kind": "rss", "id": "ph1lza", "name": "x"}),
        json!({"kind": "twitch", "id": "ph1lza"}),
        json!({"kind": "twitch", "id": "ph1lza", "name": "\u{0}"}),
        json!("ph1lza"),
    ] {
        assert_eq!(channel_mark(&junk), None, "{junk}");
    }
    assert_eq!(mark_name("  a\u{0}b  ").as_deref(), Some("a b"));
    assert_eq!(
        mark_name(&"й".repeat(500)).map(|n| n.chars().count()),
        Some(101)
    );
    assert_eq!(mark_name(" \n "), None);
}

fn twitch_mark(login: &str) -> ChannelMark {
    ChannelMark {
        kind: MarkKind::Twitch,
        id: login.into(),
        name: login.into(),
    }
}

#[test]
fn a_channel_is_on_one_list_at_most_and_the_others_are_kept() {
    let (_dir, root) = root();
    assert_eq!(marks(&root), FeedMarks::default());
    std::fs::write(
        root.feed_channels(),
        serde_json::to_vec(&json!({
            "channels": [{"channelId": "UCabcdefghijklmnopqrstuv", "title": "Mine"}],
            "twitch": [{"login": "docm77", "name": "docm77"}],
            "other": "dropped",
        }))
        .unwrap(),
    )
    .unwrap();
    let after = mark(&root, twitch_mark("zubarefff_1"), Some(MarkList::Hidden)).unwrap();
    assert_eq!(after.hidden, [twitch_mark("zubarefff_1")]);
    // Favourite takes it off the hidden list.
    let after = mark(&root, twitch_mark("zubarefff_1"), Some(MarkList::Favorite)).unwrap();
    assert!(after.hidden.is_empty());
    assert_eq!(after.favorite, [twitch_mark("zubarefff_1")]);
    assert_eq!(marks(&root), after);
    // No list takes it off both.
    let after = mark(&root, twitch_mark("zubarefff_1"), None).unwrap();
    assert_eq!(after, FeedMarks::default());
    // The player's own channels stay; what is not ours does not.
    assert_eq!(own_channels(&root).len(), 1);
    assert_eq!(own_streamers(&root).len(), 1);
    let file: Value =
        serde_json::from_slice(&std::fs::read(root.feed_channels()).unwrap()).unwrap();
    assert_eq!(file.get("other"), None);
    // Adding an own channel keeps the marks.
    mark(&root, twitch_mark("ph1lza"), Some(MarkList::Hidden)).unwrap();
    remove_own_streamer(&root, "docm77").unwrap();
    assert_eq!(marks(&root).hidden, [twitch_mark("ph1lza")]);
}

#[test]
fn marks_are_bounded() {
    let (_dir, root) = root();
    for i in 0..MAX_OWN_CHANNELS {
        mark(
            &root,
            twitch_mark(&format!("fav{i:03}")),
            Some(MarkList::Favorite),
        )
        .unwrap();
    }
    assert_eq!(
        mark(&root, twitch_mark("onemore"), Some(MarkList::Favorite)),
        Err(FeedProblem::Full)
    );
    // The limit is for each place.
    let youtube = ChannelMark {
        kind: MarkKind::Youtube,
        id: "UCabcdefghijklmnopqrstuv".into(),
        name: "C".into(),
    };
    assert!(mark(&root, youtube, Some(MarkList::Favorite)).is_ok());
    for i in 0..MAX_HIDDEN {
        mark(
            &root,
            twitch_mark(&format!("hid{i:03}")),
            Some(MarkList::Hidden),
        )
        .unwrap();
    }
    assert_eq!(
        mark(&root, twitch_mark("onemore"), Some(MarkList::Hidden)),
        Err(FeedProblem::Full)
    );
    // A file with more than allowed is read up to the limits.
    let mut favorite: Vec<Value> = (0..50)
        .map(|i| json!({"kind": "twitch", "id": format!("many{i:03}"), "name": "x"}))
        .collect();
    favorite.push(json!({"kind": "youtube", "id": "UCabcdefghijklmnopqrstuv", "name": "C"}));
    std::fs::write(
        root.feed_channels(),
        serde_json::to_vec(&json!({ "favorite": favorite })).unwrap(),
    )
    .unwrap();
    let read = marks(&root);
    assert_eq!(read.favorite.len(), MAX_OWN_CHANNELS + 1);
    assert_eq!(read.favorite[0].kind, MarkKind::Youtube);
}

#[test]
fn kept_copies_are_read_through_the_same_rules() {
    let (_dir, root) = root();
    let read = feed(&sample("en"), Lang::En).unwrap();
    let file = root.feed_cache_dir().join("feed-en.json");
    keep(
        &file,
        &serde_json::to_vec(&json!({ "feed": read })).unwrap(),
    );
    assert_eq!(cached_feed(&root, Lang::En), Some(read.clone()));
    assert_eq!(cached_feed(&root, Lang::Ru), None);
    // A copy changed on disk is read like an answer from the network.
    let mut changed: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    changed["feed"]["news"][0]["url"] = json!("javascript:alert(1)");
    changed["feed"]["videos"][0]["id"] = json!("<script>");
    std::fs::write(&file, serde_json::to_vec(&changed).unwrap()).unwrap();
    let reread = cached_feed(&root, Lang::En).unwrap();
    assert_eq!(reread.news[0].url, None);
    assert!(reread.videos.is_empty());

    let note = note(
        &note_json(json!([{"t": "p", "c": [{"text": "x", "i": true}]}])),
        "j-26-4-snapshot-4",
        Lang::Ar,
    )
    .unwrap();
    keep(
        &note_file(&root, "j-26-4-snapshot-4", Lang::Ar),
        &serde_json::to_vec(&json!({ "note": note })).unwrap(),
    );
    assert_eq!(
        cached_note(&root, "j-26-4-snapshot-4", Lang::Ar),
        Some(note)
    );
    assert_eq!(cached_note(&root, "j-26-4-snapshot-4", Lang::En), None);
    assert_eq!(cached_note(&root, "../feed-en", Lang::En), None);
}

#[test]
fn caches_drop_their_oldest_files() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..10 {
        let path = dir.path().join(format!("f{i}"));
        std::fs::write(&path, vec![0u8; 100]).unwrap();
        let time = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 + i * 60);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(time)
            .unwrap();
    }
    std::fs::write(dir.path().join(".f-unfinished"), vec![0u8; 100]).unwrap();
    trim_dir(dir.path(), 6, u64::MAX, u64::MAX);
    let mut left: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, [".f-unfinished", "f4", "f5", "f6", "f7", "f8", "f9"]);
    // Over 500 bytes: down to 300.
    trim_dir(dir.path(), usize::MAX, 500, 300);
    let count = files_by_age(dir.path()).len();
    assert_eq!(count, 3);
}

#[test]
fn languages_follow_the_settings() {
    let settings = FeedSettings::default();
    let ru = Languages::of("ru", &settings);
    assert_eq!(
        (ru.interface, ru.notes, ru.videos),
        (Lang::Ru, Lang::Ru, Lang::Ru)
    );
    let other = Languages::of("pt-BR", &settings);
    assert_eq!(other.interface, Lang::En);
    assert_eq!(Languages::of("ar-EG", &settings).interface, Lang::Ar);
    let settings = FeedSettings {
        machine_translation: false,
        video_language: VideoLanguage::Ar,
        ..FeedSettings::default()
    };
    let ru = Languages::of("ru", &settings);
    assert_eq!(
        (ru.interface, ru.notes, ru.videos),
        (Lang::Ru, Lang::En, Lang::Ar)
    );
}

#[test]
fn parts_are_composed_from_their_languages() {
    let ru = feed(&sample("ru"), Lang::Ru).unwrap();
    let mut en_value = sample("en");
    en_value["versions"][0]["title"] = json!("English title");
    en_value["videos"] = json!([]);
    let en = feed(&en_value, Lang::En).unwrap();
    let composed = compose(ru.clone(), Some(en.clone()), None);
    assert_eq!(composed.versions[0].title, "English title");
    assert_eq!(composed.videos, ru.videos);
    assert_eq!(composed.lang, Lang::Ru);
    let composed = compose(ru.clone(), None, Some(en));
    assert!(composed.videos.is_empty());
    assert_eq!(composed.versions, ru.versions);
    assert_eq!(compose(ru.clone(), None, None), ru);
}

#[test]
fn player_addresses_are_built_from_checked_ids() {
    let video = Target::parse("yt", "dQw4w9WgXcQ").unwrap();
    assert_eq!(
        video.player_url(Lang::Ru).as_str(),
        "https://feed.lapi.sh/play/yt/dQw4w9WgXcQ?lang=ru"
    );
    assert_eq!(
        video.browser_url().as_str(),
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
    );
    let stream = Target::parse("twitch", "some_streamer").unwrap();
    assert_eq!(
        stream.player_url(Lang::Ar).as_str(),
        "https://feed.lapi.sh/play/twitch/some_streamer?lang=ar"
    );
    assert_eq!(
        stream.browser_url().as_str(),
        "https://www.twitch.tv/some_streamer"
    );
    for (kind, id) in [
        ("yt", "dQw4w9WgXc"),
        ("yt", "../../admin"),
        ("yt", "dQw4w9WgXc?"),
        ("twitch", "Some"),
        ("twitch", "a/b"),
        ("twitch", "dQw4w9WgXcQ?x"),
        ("vimeo", "dQw4w9WgXcQ"),
        ("", ""),
    ] {
        assert_eq!(Target::parse(kind, id), None, "{kind} {id}");
    }
    // Both addresses pass the checks they will meet.
    assert!(navigation_allowed(&video.player_url(Lang::En)));
    assert!(crate::external::check_allowed(video.browser_url().as_str()).is_ok());
    assert!(crate::external::check_allowed(stream.browser_url().as_str()).is_ok());
}

#[test]
fn the_player_window_goes_nowhere_else() {
    let ok = |raw: &str| navigation_allowed(&url::Url::parse(raw).unwrap());
    for good in [
        "https://feed.lapi.sh/play/yt/dQw4w9WgXcQ?lang=en",
        "https://feed.lapi.sh/play/twitch/some_streamer",
        "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ?autoplay=1",
        "https://player.twitch.tv/?channel=some_streamer&parent=feed.lapi.sh",
        "about:blank",
        "about:srcdoc",
    ] {
        assert!(ok(good), "{good}");
    }
    for bad in [
        "https://feed.lapi.sh/",
        "https://feed.lapi.sh/v1/feed",
        "https://feed.lapi.sh/play/yt/../../v1/feed",
        "https://feed.lapi.sh/play/yt/short",
        "https://feed.lapi.sh/play/vimeo/dQw4w9WgXcQ",
        "http://feed.lapi.sh/play/yt/dQw4w9WgXcQ",
        "https://feed.lapi.sh:8443/play/yt/dQw4w9WgXcQ",
        "https://user@feed.lapi.sh/play/yt/dQw4w9WgXcQ",
        "https://feed.lapi.sh.evil.example/play/yt/dQw4w9WgXcQ",
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        "https://www.youtube-nocookie.com/",
        "https://www.twitch.tv/some_streamer",
        "https://evil.example/",
        "javascript:alert(1)",
        "data:text/html,x",
        "file:///etc/passwd",
        "about:config",
        "tauri://localhost/index.html",
        "http://ipc.localhost/feed_get",
    ] {
        assert!(!ok(bad), "{bad}");
    }
}

#[test]
fn watched_videos_are_bounded_and_checked_on_disk() {
    let (_dir, root) = root();
    assert!(watched(&root).is_empty());
    remember_watched(&root, "dQw4w9WgXcQ").unwrap();
    remember_watched(&root, "abcdefghijk").unwrap();
    // Played again, it moves last.
    remember_watched(&root, "dQw4w9WgXcQ").unwrap();
    assert_eq!(watched(&root), ["abcdefghijk", "dQw4w9WgXcQ"]);
    for bad in ["", "short", "dQw4w9WgXc<", "../../etc/pw", "dQw4w9WgXcQQ"] {
        assert_eq!(remember_watched(&root, bad), Err(FeedProblem::BadInput));
    }
    assert_eq!(watched(&root).len(), 2);
    // A file with junk keeps only sound ids, each once.
    fs::write(
        root.feed_watched(),
        json!({"videos": ["abcdefghijk", 7, null, "<script>", {"a": 1}, "abcdefghijk", "zyxwvutsrqp"]})
            .to_string(),
    )
    .unwrap();
    assert_eq!(watched(&root), ["abcdefghijk", "zyxwvutsrqp"]);
    for junk in [
        "",
        "null",
        "[]",
        "{\"videos\": \"abcdefghijk\"}",
        "\u{0}\u{1}",
    ] {
        fs::write(root.feed_watched(), junk).unwrap();
        assert!(watched(&root).is_empty(), "{junk:?}");
    }
    // Past the limit the oldest leave.
    let many: Vec<String> = (0..MAX_WATCHED + 20)
        .map(|i| format!("video{i:06}"))
        .collect();
    fs::write(root.feed_watched(), json!({ "videos": many }).to_string()).unwrap();
    let kept = watched(&root);
    assert_eq!(kept.len(), MAX_WATCHED);
    assert_eq!(kept.first().map(String::as_str), Some("video000020"));
    remember_watched(&root, "dQw4w9WgXcQ").unwrap();
    let kept = watched(&root);
    assert_eq!(kept.len(), MAX_WATCHED);
    assert_eq!(kept.first().map(String::as_str), Some("video000021"));
    assert_eq!(kept.last().map(String::as_str), Some("dQw4w9WgXcQ"));
    // A file too large is not read.
    fs::write(root.feed_watched(), "x".repeat(65 * 1024)).unwrap();
    assert!(watched(&root).is_empty());
}
