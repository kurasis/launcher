//! `lapi-upd`: publishes launcher updates to upd.lapi.sh (D-063).
//!
//! nginx serves everything from the root folder as static files; this tool is
//! the only thing that changes them:
//!
//! ```text
//! lapi-upd publish <version> [--channel C] [--rollout N] [--mandatory] [--partial]
//! lapi-upd rollout <channel> <percent>
//! lapi-upd pause <channel> | resume <channel>
//! lapi-upd revoke <channel> <version>
//! lapi-upd status
//! lapi-upd pack <version> --out <file> [--notes <json>] <target>:<bundle>:<path>...   (CI)
//! lapi-upd skins-mod-jars <mod version>
//! lapi-upd skins-mod <channel>
//! lapi-upd skins-mod-list <mod version> --channel C --builds <json> --sums <file>
//!          --jars <folder> --out <file> [--rollback]                               (CI)
//! ```
//!
//! `skins-mod-jars` puts a Launcher Skins release's jars from
//! `incoming/skins-mod-<version>/` (uploaded by CI with its SHA256SUMS) in place
//! at `/skins-mod/<version>/`, where players download them. `skins-mod` puts
//! `incoming/skins-mod-<channel>.json` (signed by `skins-mod-list` in CI) in
//! place as the channel's list of Launcher Skins jars, once all its jars are.
//!
//! `publish` reads `incoming/launcher-<version>-release.tar.gz` (built by the
//! Package workflow), checks it and makes the version current. Settings come
//! from the environment: `LAPI_UPD_ROOT` (default `/home/www/lapi/upd/data`),
//! `LAPI_UPD_URL` (default `https://upd.lapi.sh`) and `LAPI_UPD_KEY` (the public
//! key, default `/home/www/lapi/upd/update-key.pub`, which the deploy writes from
//! the repository variable `UPDATE_PUBLIC_KEY`). The key here only catches
//! mistakes early: the launcher checks every signature itself.

mod pack;
mod release;
mod skins_mod;

use std::path::PathBuf;

use release::{Options, Server};

fn env(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_owned())
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn percent(text: Option<&str>) -> Result<u8, String> {
    text.ok_or("a percent is needed")?
        .parse::<u8>()
        .ok()
        .filter(|p| *p <= 100)
        .ok_or_else(|| "a percent is 0 to 100".to_owned())
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let server = Server::new(
        PathBuf::from(env("LAPI_UPD_ROOT", "/home/www/lapi/upd/data")),
        env("LAPI_UPD_URL", "https://upd.lapi.sh"),
        PathBuf::from(env("LAPI_UPD_KEY", "/home/www/lapi/upd/update-key.pub")),
    );
    let arg = |i: usize| args.get(i).map(String::as_str);
    let result = match arg(0) {
        Some("publish") => match arg(1) {
            Some(version) => (|| {
                let options = Options {
                    channel: flag(&args, "--channel").unwrap_or("stable").to_owned(),
                    rollout: match flag(&args, "--rollout") {
                        Some(p) => percent(Some(p))?,
                        None => 100,
                    },
                    mandatory: has(&args, "--mandatory"),
                    partial: has(&args, "--partial"),
                };
                server.publish(version, &options)
            })(),
            None => Err("publish needs a version".to_owned()),
        },
        Some("rollout") => match (arg(1), percent(arg(2))) {
            (Some(channel), Ok(p)) => server.set_rollout(channel, p),
            (None, _) => Err("rollout needs a channel".to_owned()),
            (_, Err(e)) => Err(e),
        },
        Some("pause") => match arg(1) {
            Some(channel) => server.pause(channel, true),
            None => Err("pause needs a channel".to_owned()),
        },
        Some("resume") => match arg(1) {
            Some(channel) => server.pause(channel, false),
            None => Err("resume needs a channel".to_owned()),
        },
        Some("revoke") => match (arg(1), arg(2)) {
            (Some(channel), Some(version)) => server.revoke(channel, version),
            _ => Err("revoke needs a channel and a version".to_owned()),
        },
        Some("status") => server
            .status()
            .and_then(|s| Ok(format!("{s}\n{}", server.skins_mod_status()?))),
        Some("skins-mod-jars") => match arg(1) {
            Some(version) => server.install_skins_mod_jars(version),
            None => Err("skins-mod-jars needs a mod version".to_owned()),
        },
        Some("skins-mod") => match arg(1) {
            Some(channel) => server.install_skins_mod(channel),
            None => Err("skins-mod needs a channel".to_owned()),
        },
        Some("skins-mod-list") => (|| {
            let version = arg(1).ok_or("skins-mod-list needs a mod version")?;
            let channel = flag(&args, "--channel").ok_or("skins-mod-list needs --channel")?;
            let read = |name: &str| -> Result<Vec<u8>, String> {
                let path = flag(&args, name).ok_or_else(|| format!("skins-mod-list needs {name}"))?;
                std::fs::read(path).map_err(|e| format!("reading {path}: {e}"))
            };
            let serial = u64::try_from(chrono::Utc::now().timestamp())
                .map_err(|_| "the clock is before 1970")?;
            let list = skins_mod::build(
                version,
                channel,
                serial,
                has(&args, "--rollback"),
                &read("--builds")?,
                &String::from_utf8_lossy(&read("--sums")?),
                std::path::Path::new(flag(&args, "--jars").ok_or("skins-mod-list needs --jars")?),
            )?;
            let key = pack::secret_key(
                &std::env::var("TAURI_SIGNING_PRIVATE_KEY")
                    .map_err(|_| "TAURI_SIGNING_PRIVATE_KEY is not set")?,
                &std::env::var("TAURI_SIGNING_PRIVATE_KEY_PASSWORD").unwrap_or_default(),
            )?;
            let envelope = skins_mod::sign(&list, &key)?;
            let out = flag(&args, "--out").ok_or("skins-mod-list needs --out")?;
            std::fs::write(out, serde_json::to_vec(&envelope).map_err(|e| e.to_string())?)
                .map_err(|e| format!("writing {out}: {e}"))?;
            Ok(format!(
                "{out}: skins mod {version} for {channel}, {} jars, serial {serial}",
                list.jars.len()
            ))
        })(),
        Some("pack") => (|| {
            let version = arg(1).ok_or("pack needs a version")?;
            let out = flag(&args, "--out").ok_or("pack needs --out")?;
            let notes = match flag(&args, "--notes") {
                Some(path) => serde_json::from_slice(
                    &std::fs::read(path).map_err(|e| format!("reading {path}: {e}"))?,
                )
                .map_err(|e| format!("{path}: {e}"))?,
                None => Default::default(),
            };
            let key = pack::secret_key(
                &std::env::var("TAURI_SIGNING_PRIVATE_KEY")
                    .map_err(|_| "TAURI_SIGNING_PRIVATE_KEY is not set")?,
                &std::env::var("TAURI_SIGNING_PRIVATE_KEY_PASSWORD").unwrap_or_default(),
            )?;
            let items: Vec<String> = args
                .iter()
                .skip(2)
                .filter(|a| a.matches(':').count() >= 2)
                .cloned()
                .collect();
            pack::pack(version, std::path::Path::new(out), notes, &items, &key)
        })(),
        Some("--version") => Ok(format!("lapi-upd {}", env!("CARGO_PKG_VERSION"))),
        _ => Err(
            "usage: lapi-upd publish <version> [--channel C] [--rollout N] [--mandatory] [--partial] | rollout <channel> <percent> | pause <channel> | resume <channel> | revoke <channel> <version> | status | skins-mod-jars <mod version> | skins-mod <channel>"
                .to_owned(),
        ),
    };
    match result {
        Ok(text) => {
            println!("{text}");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("lapi-upd: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
