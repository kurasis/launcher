//! The screenshot gallery: listing, names, thumbnails and deletion.

use super::*;
use crate::state::NoopSink;
use crate::state::dto::{CreateInstancePayload, MutationRequest};
use crate::validation::MemoryPolicy;
use uuid::Uuid;

fn setup() -> (tempfile::TempDir, AppState, InstanceId, InstanceId) {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::initialize(
        DataRoot::new(dir.path().to_path_buf()),
        "test",
        MemoryPolicy::from_physical_bytes(16 << 30),
        Box::new(NoopSink),
    );
    let create = |name: &str| {
        state
            .create_instance(MutationRequest {
                request_id: Uuid::new_v4().to_string(),
                payload: CreateInstancePayload {
                    display_name: name.into(),
                    version_id: "1.21.1".into(),
                    loader: None,
                },
            })
            .unwrap()
            .id
    };
    let (a, b) = (create("Survival"), create("Creative"));
    (dir, state, a, b)
}

/// A real PNG of `width` × `height` RGBA pixels: left half red, right half blue.
fn png_bytes(width: u32, height: u32) -> Vec<u8> {
    let mut data = Vec::new();
    for _ in 0..height {
        for x in 0..width {
            data.extend(if x < width / 2 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            });
        }
    }
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&data).unwrap();
    writer.finish().unwrap();
    out
}

fn shot(state: &AppState, id: InstanceId, file: &str, bytes: &[u8], age_secs: u64) -> PathBuf {
    let dir = state.data_root().unwrap().game_dir(id).join("screenshots");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(file);
    fs::write(&path, bytes).unwrap();
    let time = std::time::SystemTime::now() - std::time::Duration::from_secs(age_secs);
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(time)
        .unwrap();
    path
}

#[test]
fn screenshots_are_listed_by_installation_newest_first() {
    let (_dir, state, a, b) = setup();
    let image = png_bytes(64, 36);
    shot(&state, a, "2026-10-01_10.00.00.png", &image, 300);
    shot(&state, a, "2026-10-02_10.00.00.png", &image, 200);
    shot(&state, b, "2026-10-03_10.00.00.png", &image, 100);
    // Not screenshots: other files, hidden files and folders.
    shot(&state, a, "notes.txt", b"text", 10);
    shot(&state, a, ".hidden.png", &image, 10);
    fs::create_dir_all(
        state
            .data_root()
            .unwrap()
            .game_dir(a)
            .join("screenshots")
            .join("folder.png"),
    )
    .unwrap();

    let gallery = list(&state).unwrap();
    let groups: Vec<_> = gallery
        .installations
        .iter()
        .map(|g| g.instance_id)
        .collect();
    assert_eq!(
        groups,
        [b, a],
        "the installation with the newest screenshot first"
    );
    let files: Vec<_> = gallery.installations[1]
        .screenshots
        .iter()
        .map(|s| s.file.as_str())
        .collect();
    assert_eq!(
        files,
        ["2026-10-02_10.00.00.png", "2026-10-01_10.00.00.png"]
    );
    let first = &gallery.installations[0].screenshots[0];
    assert_eq!((first.width, first.height), (Some(64), Some(36)));
    assert_eq!(gallery.total_bytes, 3 * image.len() as u64);
}

#[test]
fn installations_without_screenshots_are_left_out() {
    let (_dir, state, _, _) = setup();
    assert!(list(&state).unwrap().installations.is_empty());
}

#[test]
fn names_cannot_leave_the_screenshots_folder() {
    for bad in [
        "",
        "../options.png",
        "..\\x.png",
        "a/b.png",
        "C:x.png",
        ".png",
        "shot.txt",
        "shot\n.png",
    ] {
        assert!(!valid_file_name(bad), "{bad:?}");
    }
    assert!(valid_file_name("2026-10-17_12.00.00_2.PNG"));
    let id = InstanceId::generate();
    assert!(parse_request(&format!("/thumb/{id}/..%2Foptions.png")).is_none());
    assert!(parse_request(&format!("/thumb/{id}/a.png/extra")).is_none());
    assert!(parse_request("/thumb/not-an-id/a.png").is_none());
    assert!(parse_request(&format!("/other/{id}/a.png")).is_none());
    assert_eq!(
        parse_request(&format!("/full/{id}/my%20shot.png")),
        Some((Size::Full, id, "my shot.png".to_owned()))
    );
}

#[cfg(unix)]
#[test]
fn links_are_not_served() {
    let (dir, state, a, _) = setup();
    let outside = dir.path().join("secret.png");
    fs::write(&outside, png_bytes(4, 4)).unwrap();
    let folder = state.data_root().unwrap().game_dir(a).join("screenshots");
    fs::create_dir_all(&folder).unwrap();
    std::os::unix::fs::symlink(&outside, folder.join("link.png")).unwrap();
    assert!(serve(&state, Size::Full, a, "link.png").is_err());
    assert!(list(&state).unwrap().installations.is_empty());
}

#[test]
fn thumbnails_are_small_cached_and_pruned() {
    let (_dir, state, a, b) = setup();
    shot(&state, a, "big.png", &png_bytes(1920, 1080), 10);
    let full = serve(&state, Size::Full, a, "big.png").unwrap();
    assert_eq!(full, png_bytes(1920, 1080));
    // Another installation cannot reach it.
    assert!(serve(&state, Size::Full, b, "big.png").is_err());

    let thumb = serve(&state, Size::Thumbnail, a, "big.png").unwrap();
    let decoder = png::Decoder::new(std::io::Cursor::new(&thumb));
    let mut reader = decoder.read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    assert_eq!((info.width, info.height), (THUMB_WIDTH, 270));
    assert_eq!(info.color_type, png::ColorType::Rgb);
    // Colors survive the scaling: red on the left, blue on the right.
    assert_eq!(&pixels[..3], [255, 0, 0]);
    let last = pixels.len() - 3;
    assert_eq!(&pixels[last..], [0, 0, 255]);

    let cache = state.data_root().unwrap().thumbnails_dir();
    assert_eq!(fs::read_dir(&cache).unwrap().count(), 1);
    assert_eq!(serve(&state, Size::Thumbnail, a, "big.png").unwrap(), thumb);

    // A small screenshot is not scaled up.
    shot(&state, a, "small.png", &png_bytes(40, 20), 5);
    let small = make_thumbnail(
        &state
            .data_root()
            .unwrap()
            .game_dir(a)
            .join("screenshots/small.png"),
        THUMB_WIDTH,
    )
    .unwrap();
    let reader = png::Decoder::new(std::io::Cursor::new(&small))
        .read_info()
        .unwrap();
    assert_eq!(reader.info().size(), (40, 20));

    // Deleting removes the screenshot and its thumbnail; listing prunes the rest.
    delete(&state, a, "big.png").unwrap();
    assert!(delete(&state, a, "big.png").is_err());
    assert_eq!(fs::read_dir(&cache).unwrap().count(), 0);
    serve(&state, Size::Thumbnail, a, "small.png").unwrap();
    fs::remove_file(
        state
            .data_root()
            .unwrap()
            .game_dir(a)
            .join("screenshots/small.png"),
    )
    .unwrap();
    list(&state).unwrap();
    assert_eq!(fs::read_dir(&cache).unwrap().count(), 0);
}

#[test]
fn files_that_are_not_png_are_refused() {
    let (_dir, state, a, _) = setup();
    shot(&state, a, "broken.png", b"not a png", 1);
    assert!(serve(&state, Size::Thumbnail, a, "broken.png").is_err());
    let gallery = list(&state).unwrap();
    assert_eq!(gallery.installations[0].screenshots[0].width, None);
}
