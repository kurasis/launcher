//! A stand-in for skins.lapi.sh on this computer: one test player per skin
//! variant, with textures drawn here (HD, slim, animated strips, capes), served
//! the way the real service answers so the skins mod cannot tell the difference.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;

/// What one test player wears.
#[derive(Debug, Clone, Copy)]
pub struct Variant {
    pub name: &'static str,
    /// Skin width (64…512), frames, slim model.
    pub skin: Option<(u32, u32, bool)>,
    /// Cape width (64…512) and frames.
    pub cape: Option<(u32, u32)>,
}

impl Variant {
    /// The test player's nick: what the skins mod asks the service about.
    pub fn nick(&self) -> String {
        format!("GT_{}", self.name)
    }

    /// The skin the bot should see: `none`, `wide` or `slim`.
    pub fn expect_skin(&self) -> &'static str {
        match self.skin {
            None => "none",
            Some((_, _, true)) => "slim",
            Some(_) => "wide",
        }
    }

    pub fn skin_frames(&self) -> u32 {
        self.skin.map_or(1, |(_, frames, _)| frames)
    }

    pub fn cape_frames(&self) -> u32 {
        self.cape.map_or(0, |(_, frames)| frames)
    }

    pub fn describe(&self) -> String {
        let skin = match self.skin {
            None => "no skin".to_owned(),
            Some((w, 1, slim)) => format!("skin {w}×{w}{}", if slim { " slim" } else { "" }),
            Some((w, f, slim)) => {
                format!(
                    "skin {w}×{w}, {f} frames{}",
                    if slim { ", slim" } else { "" }
                )
            }
        };
        let cape = match self.cape {
            None => "no cape".to_owned(),
            Some((w, 1)) => format!("cape {w}×{}", w / 2),
            Some((w, f)) => format!("cape {w}×{}, {f} frames", w / 2),
        };
        format!("{skin}, {cape}")
    }
}

/// Every variant; a run picks one by name.
pub const VARIANTS: &[Variant] = &[
    Variant {
        name: "classic",
        skin: Some((64, 1, false)),
        cape: None,
    },
    Variant {
        name: "slim128",
        skin: Some((128, 1, true)),
        cape: Some((64, 1)),
    },
    Variant {
        name: "hd256",
        skin: Some((256, 1, false)),
        cape: Some((256, 1)),
    },
    Variant {
        name: "hd512",
        skin: Some((512, 1, true)),
        cape: Some((512, 1)),
    },
    Variant {
        name: "anim64",
        skin: Some((64, 8, false)),
        cape: Some((64, 4)),
    },
    Variant {
        name: "anim256",
        skin: Some((256, 16, true)),
        cape: Some((128, 8)),
    },
    Variant {
        name: "cape",
        skin: None,
        cape: Some((128, 1)),
    },
    Variant {
        name: "none",
        skin: None,
        cape: None,
    },
];

pub fn variant(name: &str) -> Option<Variant> {
    VARIANTS.iter().copied().find(|v| v.name == name)
}

/// A texture as the service describes it, and its PNG.
struct Texture {
    json: Value,
    png: Vec<u8>,
}

/// Colour of one frame of one variant: the same every run, different per frame.
fn colour(seed: u32, frame: u32) -> [u8; 3] {
    let h = seed
        .wrapping_mul(2_654_435_761)
        .wrapping_add(frame.wrapping_mul(40_503));
    [
        64 + (h & 0x7f) as u8,
        64 + ((h >> 8) & 0x7f) as u8,
        64 + ((h >> 16) & 0x7f) as u8,
    ]
}

/// A vertical strip of `frames` frames, `width` wide and `frame_height` high each:
/// a checkerboard in the frame's colour with a bright border per 8×8 game pixel,
/// so HD detail and every frame change show on a screenshot.
fn draw(width: u32, frame_height: u32, frames: u32, seed: u32) -> Vec<u8> {
    let height = frame_height * frames;
    let scale = (width / 64).max(1);
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let frame = y / frame_height;
        let [r, g, b] = colour(seed, frame);
        let fy = y % frame_height;
        for x in 0..width {
            let cell = ((x / (4 * scale)) + (fy / (4 * scale))).is_multiple_of(2);
            let edge = x.is_multiple_of(8 * scale) || fy.is_multiple_of(8 * scale);
            let (r, g, b) = if edge {
                (255, 255, 255)
            } else if cell {
                (r, g, b)
            } else {
                (r / 2, g / 2, b / 2)
            };
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
    }
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("PNG header");
        writer.write_image_data(&rgba).expect("PNG data");
    }
    png
}

fn texture(width: u32, frame_height: u32, frames: u32, seed: u32, slim: Option<bool>) -> Texture {
    let png = draw(width, frame_height, frames, seed);
    let hash = hex::encode(Sha256::digest(&png));
    let mut json = json!({
        "hash": hash,
        "width": width,
        "height": frame_height * frames,
        "frames": frames,
    });
    if frames > 1 {
        json["frame_ms"] = json!([150]);
    }
    if let Some(slim) = slim {
        json["model"] = json!(if slim { "slim" } else { "default" });
    }
    Texture { json, png }
}

/// The running stand-in: its address and how often it was asked.
pub struct Server {
    pub base: String,
    pub profiles_asked: Arc<AtomicUsize>,
    pub textures_sent: Arc<AtomicUsize>,
}

/// Starts the stand-in on a free loopback port with the profile of every variant.
pub async fn start() -> Server {
    let mut profiles = HashMap::new();
    let mut textures = HashMap::new();
    for (i, v) in VARIANTS.iter().enumerate() {
        let seed = i as u32 + 1;
        let mut profile = json!({ "nick": v.nick() });
        if let Some((w, frames, slim)) = v.skin {
            let t = texture(w, w, frames, seed, Some(slim));
            profile["skin"] = t.json.clone();
            textures.insert(
                t.json["hash"].as_str().unwrap_or_default().to_owned(),
                t.png,
            );
        }
        if let Some((w, frames)) = v.cape {
            let t = texture(w, w / 2, frames, seed + 100, None);
            profile["cape"] = t.json.clone();
            textures.insert(
                t.json["hash"].as_str().unwrap_or_default().to_owned(),
                t.png,
            );
        }
        if v.skin.is_some() || v.cape.is_some() {
            profiles.insert(v.nick().to_ascii_lowercase(), profile);
        }
    }
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port");
    let base = format!("http://{}", listener.local_addr().expect("its address"));
    let profiles = Arc::new(profiles);
    let textures = Arc::new(textures);
    let profiles_asked = Arc::new(AtomicUsize::new(0));
    let textures_sent = Arc::new(AtomicUsize::new(0));
    let (asked, sent) = (Arc::clone(&profiles_asked), Arc::clone(&textures_sent));
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                continue;
            };
            let (profiles, textures) = (Arc::clone(&profiles), Arc::clone(&textures));
            let (asked, sent) = (Arc::clone(&asked), Arc::clone(&sent));
            tokio::spawn(async move {
                let Some((method, path, body)) = read_request(&mut stream).await else {
                    return;
                };
                let (status, kind, bytes) = answer(&method, &path, &body, &profiles, &textures);
                if path.starts_with("/v1/profile") {
                    asked.fetch_add(1, Ordering::Relaxed);
                } else if status == 200 {
                    sent.fetch_add(1, Ordering::Relaxed);
                }
                let head = format!(
                    "HTTP/1.1 {status} {}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    if status == 200 { "OK" } else { "Not Found" },
                    bytes.len()
                );
                let _ = stream.write_all(head.as_bytes()).await;
                let _ = stream.write_all(&bytes).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    Server {
        base,
        profiles_asked,
        textures_sent,
    }
}

fn answer(
    method: &str,
    path: &str,
    body: &[u8],
    profiles: &HashMap<String, Value>,
    textures: &HashMap<String, Vec<u8>>,
) -> (u16, &'static str, Vec<u8>) {
    let not_found = (404, "text/plain", b"not found".to_vec());
    match (method, path) {
        ("GET", p) if p.starts_with("/v1/profile/") => {
            let nick = p["/v1/profile/".len()..].to_ascii_lowercase();
            profiles.get(&nick).map_or(not_found, |profile| {
                (200, "application/json", profile.to_string().into_bytes())
            })
        }
        ("POST", "/v1/profiles") => {
            let asked: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
            let mut found = serde_json::Map::new();
            for nick in asked["nicks"].as_array().into_iter().flatten() {
                if let Some(nick) = nick.as_str()
                    && let Some(profile) = profiles.get(&nick.to_ascii_lowercase())
                {
                    found.insert(nick.to_owned(), profile.clone());
                }
            }
            let answer = json!({ "profiles": found });
            (200, "application/json", answer.to_string().into_bytes())
        }
        ("GET", p) if p.starts_with("/v1/textures/") => textures
            .get(&p["/v1/textures/".len()..])
            .map_or(not_found, |png| (200, "image/png", png.clone())),
        _ => not_found,
    }
}

/// Method, path and body of one small HTTP/1.1 request; None for anything odd.
async fn read_request(stream: &mut tokio::net::TcpStream) -> Option<(String, String, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if buf.len() > 64 * 1024 {
            return None;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.lines();
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?.to_owned();
    let path = first.next()?.to_owned();
    let length = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse::<usize>().ok())
        .unwrap_or(0)
        .min(256 * 1024);
    let mut body = buf[head_end..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(length);
    Some((method, path, body))
}

/// Every test texture is one the skins mod accepts (mod/core Rules): widths
/// 64 to 512, a skin frame as high as wide and a cape frame half as high, up
/// to 32 skin and 16 cape frames, and within the size limits.
#[test]
fn every_variant_fits_the_mods_rules() {
    for (i, v) in VARIANTS.iter().enumerate() {
        if let Some((w, frames, slim)) = v.skin {
            assert!(
                [64, 128, 256, 512].contains(&w) && (1..=32).contains(&frames),
                "{}",
                v.name
            );
            let t = texture(w, w, frames, i as u32, Some(slim));
            assert!(t.png.len() <= 2 * 1024 * 1024, "{}", v.name);
            assert_eq!(t.json["height"], w * frames);
        }
        if let Some((w, frames)) = v.cape {
            assert!(
                [64, 128, 256, 512].contains(&w) && (1..=16).contains(&frames),
                "{}",
                v.name
            );
            let t = texture(w, w / 2, frames, i as u32, None);
            assert!(t.png.len() <= 1024 * 1024, "{}", v.name);
        }
        assert!(v.nick().len() <= 16, "{}", v.name);
    }
}
