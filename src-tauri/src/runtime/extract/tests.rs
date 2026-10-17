use std::io::Write;

use super::*;

/// One entry of a synthetic test archive.
pub(crate) enum E<'a> {
    Dir(&'a str),
    File(&'a str, &'a [u8], u32),
    Link(&'a str, &'a str),
    Hard(&'a str, &'a str),
    Fifo(&'a str),
    /// Raw header name bytes, bypassing the tar crate's path checks.
    RawFile(&'a [u8], &'a [u8]),
}

pub(crate) fn tar_gz(entries: &[E<'_>]) -> Vec<u8> {
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut builder = tar::Builder::new(encoder);
    for entry in entries {
        let mut header = tar::Header::new_gnu();
        header.set_mtime(0);
        match entry {
            E::Dir(path) => {
                header.set_entry_type(tar::EntryType::Directory);
                header.set_mode(0o755);
                header.set_size(0);
                builder.append_data(&mut header, path, &[][..]).unwrap();
            }
            E::File(path, body, mode) => {
                header.set_entry_type(tar::EntryType::Regular);
                header.set_mode(*mode);
                header.set_size(body.len() as u64);
                builder.append_data(&mut header, path, *body).unwrap();
            }
            E::Link(path, target) | E::Hard(path, target) => {
                header.set_entry_type(if matches!(entry, E::Link(..)) {
                    tar::EntryType::Symlink
                } else {
                    tar::EntryType::Link
                });
                header.set_size(0);
                header.set_mode(0o777);
                builder.append_link(&mut header, path, target).unwrap();
            }
            E::Fifo(path) => {
                header.set_entry_type(tar::EntryType::Fifo);
                header.set_size(0);
                builder.append_data(&mut header, path, &[][..]).unwrap();
            }
            E::RawFile(name, body) => {
                header.set_entry_type(tar::EntryType::Regular);
                header.set_mode(0o644);
                header.set_size(body.len() as u64);
                let raw = &mut header.as_old_mut().name;
                raw[..name.len()].copy_from_slice(name);
                header.set_cksum();
                builder.append(&header, *body).unwrap();
            }
        }
    }
    builder.into_inner().unwrap().finish().unwrap()
}

pub(crate) fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, body) in entries {
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o755);
        if name.ends_with('/') {
            writer.add_directory(*name, options).unwrap();
        } else {
            writer.start_file(*name, options).unwrap();
            writer.write_all(body).unwrap();
        }
    }
    writer.finish().unwrap().into_inner()
}

fn run(
    bytes: &[u8],
    format: ArchiveFormat,
    limits: Limits,
) -> (tempfile::TempDir, LauncherResult<()>) {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("archive");
    std::fs::write(&archive, bytes).unwrap();
    let dest = dir.path().join("out");
    std::fs::create_dir(&dest).unwrap();
    let result = extract(&archive, format, &dest, limits);
    (dir, result)
}

fn reason(result: LauncherResult<()>) -> String {
    let error = result.unwrap_err();
    assert_eq!(error.code, ErrorCode::UnsafeArchive);
    match &error.safe_details.unwrap()["reason"] {
        crate::error::SafeValue::Text(t) => t.clone(),
        other => format!("{other:?}"),
    }
}

#[test]
fn normalizes_and_rejects_unsafe_paths() {
    assert_eq!(normalize("./jdk/bin/java").unwrap(), ["jdk", "bin", "java"]);
    assert_eq!(normalize("jdk//lib/").unwrap(), ["jdk", "lib"]);
    for bad in [
        "/etc/passwd",
        "\\x",
        "a/../b",
        "..",
        "c:/x",
        "a/b:stream",
        "a\\b",
        "a/\0",
        "a/CON",
        "a/nul.txt",
        "a/b ",
        "",
        "./",
    ] {
        assert!(normalize(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn link_targets_resolve_inside_the_root_only() {
    let link = |p: &str| p.split('/').map(str::to_owned).collect::<Vec<_>>();
    assert_eq!(
        resolve_link(
            &link("jdk/Contents/MacOS/libjli.dylib"),
            "../Home/lib/libjli.dylib"
        )
        .unwrap(),
        link("jdk/Contents/Home/lib/libjli.dylib")
    );
    assert!(resolve_link(&link("jdk/a"), "../../etc/passwd").is_none());
    assert!(resolve_link(&link("jdk/a"), "/etc/passwd").is_none());
    assert!(resolve_link(&link("a"), "..").is_none());
    assert!(resolve_link(&link("jdk/a"), "C:\\x").is_none());
}

#[cfg(unix)]
#[test]
fn extracts_files_dirs_modes_and_internal_links() {
    let bytes = tar_gz(&[
        E::Dir("jdk/"),
        E::File("jdk/bin/java", b"#!/bin/sh\n", 0o755),
        E::File("jdk/lib/libjli.so", b"lib", 0o644),
        E::Link("jdk/lib/alias.so", "libjli.so"),
        E::Link("jdk/legal", "lib"),
        E::Hard("jdk/lib/copy.so", "jdk/lib/libjli.so"),
    ]);
    let (dir, result) = run(&bytes, ArchiveFormat::TarGz, Limits::default());
    result.unwrap();
    let out = dir.path().join("out/jdk");
    assert!(is_executable(&out.join("bin/java")));
    assert!(!is_executable(&out.join("lib/libjli.so")));
    assert_eq!(
        std::fs::read_link(out.join("lib/alias.so")).unwrap(),
        Path::new("libjli.so")
    );
    assert_eq!(std::fs::read(out.join("lib/copy.so")).unwrap(), b"lib");
    assert!(
        !std::fs::symlink_metadata(out.join("lib/copy.so"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(single_root(&dir.path().join("out")).unwrap(), out);
}

#[test]
fn escaping_links_and_traversal_are_rejected() {
    let cases: Vec<(Vec<u8>, &str)> = vec![
        (
            tar_gz(&[E::Link("jdk/evil", "../../outside")]),
            "link leaves the runtime",
        ),
        (
            tar_gz(&[E::Link("jdk/evil", "/etc/passwd")]),
            "link leaves the runtime",
        ),
        (
            tar_gz(&[E::RawFile(b"../escape", b"x")]),
            "parent directory reference",
        ),
        (tar_gz(&[E::RawFile(b"/abs", b"x")]), "absolute path"),
        (tar_gz(&[E::Fifo("jdk/pipe")]), "special file entry"),
        (
            tar_gz(&[E::Hard("jdk/h", "jdk/missing")]),
            "hard link to a missing or non-file entry",
        ),
    ];
    for (bytes, expected) in cases {
        let (dir, result) = run(&bytes, ArchiveFormat::TarGz, Limits::default());
        assert_eq!(reason(result), expected);
        assert!(!dir.path().join("outside").exists() && !dir.path().join("escape").exists());
    }
}

#[cfg(unix)]
#[test]
fn nothing_is_written_through_a_link() {
    // A link to an internal directory followed by a file "inside" it.
    let bytes = tar_gz(&[
        E::Dir("jdk/real/"),
        E::Link("jdk/via", "real"),
        E::File("jdk/via/file", b"x", 0o644),
    ]);
    let (dir, result) = run(&bytes, ArchiveFormat::TarGz, Limits::default());
    assert_eq!(reason(result), "entry below a file or link");
    assert!(!dir.path().join("out/jdk/real/file").exists());

    let dangling = tar_gz(&[E::Dir("jdk/"), E::Link("jdk/a", "missing")]);
    assert_eq!(
        reason(run(&dangling, ArchiveFormat::TarGz, Limits::default()).1),
        "dangling link"
    );
    let looped = tar_gz(&[E::Dir("jdk/"), E::Link("jdk/a", "b"), E::Link("jdk/b", "a")]);
    assert_eq!(
        reason(run(&looped, ArchiveFormat::TarGz, Limits::default()).1),
        "link loop"
    );
}

#[test]
fn duplicates_and_case_collisions_are_rejected() {
    for bytes in [
        tar_gz(&[E::File("jdk/a", b"1", 0o644), E::File("jdk/a", b"2", 0o644)]),
        tar_gz(&[
            E::File("jdk/Readme", b"1", 0o644),
            E::File("jdk/README", b"2", 0o644),
        ]),
        tar_gz(&[
            E::File("jdk/x", b"1", 0o644),
            E::File("jdk/x/y", b"2", 0o644),
        ]),
    ] {
        let reason = reason(run(&bytes, ArchiveFormat::TarGz, Limits::default()).1);
        assert!(
            reason == "duplicate or case-colliding path" || reason == "entry below a file or link",
            "{reason}"
        );
    }
}

#[test]
fn size_and_count_limits_hold() {
    let big = vec![0u8; 10_000];
    let limits = Limits {
        max_entries: 3,
        max_file_bytes: 4096,
        max_total_bytes: 6000,
    };
    let one_big = tar_gz(&[E::File("jdk/big", &big, 0o644)]);
    assert_eq!(
        reason(run(&one_big, ArchiveFormat::TarGz, limits).1),
        "file too large"
    );
    let total = tar_gz(&[
        E::File("jdk/a", &big[..4000], 0o644),
        E::File("jdk/b", &big[..4000], 0o644),
    ]);
    assert_eq!(
        reason(run(&total, ArchiveFormat::TarGz, limits).1),
        "expanded size too large"
    );
    let many = tar_gz(&[
        E::Dir("jdk/"),
        E::File("jdk/1", b"", 0o644),
        E::File("jdk/2", b"", 0o644),
        E::File("jdk/3", b"", 0o644),
    ]);
    assert_eq!(
        reason(run(&many, ArchiveFormat::TarGz, limits).1),
        "too many entries"
    );
    // A zip bomb is bounded by the bytes actually inflated.
    let bomb = zip_bytes(&[("jdk/zeros", &vec![0u8; 100_000])]);
    let limits = Limits {
        max_file_bytes: 50_000,
        ..Limits::default()
    };
    assert_eq!(
        reason(run(&bomb, ArchiveFormat::Zip, limits).1),
        "file too large"
    );
}

#[test]
fn zip_packages_extract_with_the_same_rules() {
    let bytes = zip_bytes(&[
        ("jdk/", b""),
        ("jdk/bin/java.exe", b"MZ"),
        ("jdk/lib/modules", b"m"),
    ]);
    let (dir, result) = run(&bytes, ArchiveFormat::Zip, Limits::default());
    result.unwrap();
    assert_eq!(
        std::fs::read(dir.path().join("out/jdk/bin/java.exe")).unwrap(),
        b"MZ"
    );
    for bad in [
        zip_bytes(&[("../evil", b"x")]),
        zip_bytes(&[("jdk/a:stream", b"x")]),
        zip_bytes(&[("jdk/A", b"x"), ("jdk/a", b"y")]),
    ] {
        assert!(run(&bad, ArchiveFormat::Zip, Limits::default()).1.is_err());
    }
}

#[test]
fn package_root_must_be_a_single_directory() {
    let (dir, result) = run(
        &tar_gz(&[E::File("a/x", b"", 0o644), E::File("b/y", b"", 0o644)]),
        ArchiveFormat::TarGz,
        Limits::default(),
    );
    result.unwrap();
    assert!(single_root(&dir.path().join("out")).is_err());
    let (dir, result) = run(
        &tar_gz(&[E::File("loose", b"", 0o644)]),
        ArchiveFormat::TarGz,
        Limits::default(),
    );
    result.unwrap();
    assert!(single_root(&dir.path().join("out")).is_err());
}
