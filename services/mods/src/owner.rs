//! The owner's commands (D-066, stage 6), run by hand on the server:
//!
//! ```text
//! lapi-mods find <address, slug, ID, SHA-256 or SHA-1>
//! lapi-mods review [--count]          what waits for a decision
//! lapi-mods review <n> keep|block [reason]
//! lapi-mods block <SHA-256 or SHA-1> [reason]
//! lapi-mods unblock <SHA-256 or SHA-1>
//! lapi-mods remove <project> [reason] at its author's request
//! lapi-mods forget <SHA-256> [--yes]  deletes a held file for good
//! lapi-mods virustotal status         VirusTotal's quota and queue
//! lapi-mods virustotal check <SHA-256 or SHA-1>
//!                                     asks VirusTotal about a file now
//! lapi-mods add <address>...          adds projects as a player would
//! lapi-mods add --status              how the owner's additions went
//! lapi-mods slug <project> <slug>     gives a project another slug
//! lapi-mods health                    the service's own checks, once a day
//! ```
//!
//! `find` and `review` show names and titles: they are for the owner's
//! terminal. `block`, `unblock`, `review --count`, `virustotal` and `health`
//! print fixed words, numbers and hashes only, and `add` and `slug` the
//! catalog addresses and slugs they were given, so the workflow may run them
//! in its public log.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::Urls;
use crate::blobs::{Blobs, is_sha256};
use crate::r2::{self, Bucket, Which};
use crate::store::Store;

/// The account the owner's additions are made under.
const OWNER: &str = "owner";
/// Addresses one `add` takes (the workflow passes at most 32 arguments).
const MAX_ADD: usize = 32;

/// What `health` calls a problem.
const HOUR: i64 = 3600;
const DAY: i64 = 24 * HOUR;
/// A job due this long and still waiting: the workers are stuck or swamped.
const QUEUE_LATE: i64 = 12 * HOUR;
/// Failed jobs of the last day: at least this many, and over a quarter.
const JOBS_FAILING: i64 = 20;
/// Failed requests to add projects of the last day: at least this many, and
/// over half.
const REQUESTS_FAILING: i64 = 5;
/// An open review older than this waits for the owner.
const REVIEW_LATE: i64 = 3 * DAY;
/// The database copy to R2 and the look for popular projects, daily: two
/// days without one is a problem.
const DAILY_LATE: i64 = 2 * DAY;

/// Whether a slug the owner gives is one: small letters, digits and inner
/// hyphens, at most 64, and not shaped like a project's ID.
pub fn plain_slug(text: &str) -> bool {
    (1..=64).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !text.starts_with('-')
        && !text.ends_with('-')
        && uuid::Uuid::parse_str(text).is_err()
}

/// The addresses a kept file is served at: under each of its names on our
/// disk, and by its hash in R2.
pub fn addresses(store: &Store, urls: &Urls, sha256s: &[String]) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for sha256 in sha256s {
        for name in store.served_names(sha256)? {
            out.push(urls.file(sha256, &name));
        }
        if let Some(cdn) = urls.cdn_file(sha256) {
            out.push(cdn);
        }
    }
    Ok(out)
}

/// Lets go held files of trusted projects (`lapi-mods external trust`) that
/// the scanner only suspects, `sha256` alone or all of `source_id`'s: served
/// again and never held by a later scan, as if the owner unblocked each.
/// Known malware and the owner's blocks stay held. How many were let go.
pub fn release_trusted(
    store: &Store,
    blobs: &Blobs,
    source_id: Option<&str>,
    sha256: Option<&str>,
    now: i64,
) -> Result<usize, String> {
    let suspects = store.trusted_suspects(source_id, sha256)?;
    for sha256 in &suspects {
        if !blobs.has(sha256) {
            blobs
                .release(sha256)
                .map_err(|e| format!("putting the file back: {e}"))?;
        }
        store.set_allowed(sha256)?;
        store.decide_reviews(sha256, "keep", now)?;
        store.log_decision("trust", sha256, "trusted project", now)?;
    }
    Ok(suspects.len())
}

/// Clears addresses from the cache in front of the kept files.
pub trait Purge: Send + Sync {
    /// How many addresses were cleared.
    fn purge(&self, urls: &[String]) -> Result<usize, String>;
}

/// A file named by one of its hashes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hash {
    Sha256(String),
    Sha1(String),
}

impl Hash {
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().to_ascii_lowercase();
        if is_sha256(&text) {
            Some(Self::Sha256(text))
        } else if text.len() == 40 && text.bytes().all(|b| b.is_ascii_hexdigit()) {
            Some(Self::Sha1(text))
        } else {
            None
        }
    }

    fn text(&self) -> &str {
        match self {
            Self::Sha256(h) | Self::Sha1(h) => h,
        }
    }
}

pub struct Owner<'a> {
    pub store: &'a Store,
    pub blobs: &'a Blobs,
    pub urls: &'a Urls,
    pub purge: Option<&'a dyn Purge>,
    /// R2, when the catalog serves from it.
    pub bucket: Option<&'a dyn Bucket>,
    /// VirusTotal, when its key is there.
    pub virustotal: Option<&'a dyn crate::virustotal::Api>,
    pub now: i64,
}

/// A reason as kept in the journal: one line, bounded.
pub fn reason(words: &[String]) -> String {
    crate::model::clean(&words.join(" "), 300, false)
}

fn day(at: i64) -> String {
    chrono::DateTime::from_timestamp(at, 0)
        .map(|t| t.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

impl Owner<'_> {
    /// The SHA-256 of a file named by either hash, when it is here.
    fn kept(&self, hash: &Hash) -> Result<Option<String>, String> {
        match hash {
            Hash::Sha256(h) => Ok(self.store.blob_info(h)?.map(|b| b.sha256)),
            Hash::Sha1(h) => self.store.blob_by_sha1(h),
        }
    }

    /// Clears a file's addresses from the cache; a line saying how it went.
    fn clear(&self, sha256s: &[String]) -> Result<String, String> {
        for sha256 in sha256s {
            self.unpublish(sha256)?;
        }
        let urls = addresses(self.store, self.urls, sha256s)?;
        if urls.is_empty() {
            return Ok("cache nothing to clear".to_owned());
        }
        Ok(match self.purge {
            None => format!(
                "cache not cleared: no Cloudflare token ({} addresses stay cached up to a year)",
                urls.len()
            ),
            Some(purge) => match purge.purge(&urls) {
                Ok(n) => format!("cache cleared {n} addresses"),
                Err(e) => format!("cache not cleared: {e}"),
            },
        })
    }

    /// Deletes a held file's copy from the public bucket at once (the
    /// service would in its next round).
    fn unpublish(&self, sha256: &str) -> Result<(), String> {
        if let Some(bucket) = self.bucket
            && self.store.copy_of(sha256)?.as_deref() == Some("public")
        {
            bucket.delete(Which::Public, &r2::file_key(sha256))?;
            self.store.set_copy(sha256, "")?;
        }
        Ok(())
    }

    /// Takes a kept file out of service at once and lists it for launchers.
    pub fn block(&self, hash: &Hash, reason: &str) -> Result<Vec<String>, String> {
        let sha256 = self.kept(hash)?;
        let info = match &sha256 {
            Some(s) => self.store.blob_info(s)?,
            None => None,
        };
        let (block256, block1) = match hash {
            Hash::Sha256(h) => (Some(h.clone()), info.as_ref().map(|b| b.sha1.clone())),
            Hash::Sha1(h) => (sha256.clone(), Some(h.clone())),
        };
        let new = self
            .store
            .block(block256.as_deref(), block1.as_deref(), reason, self.now)?;
        let mut lines = vec![format!("blocked {}", u8::from(new))];
        let mut withdrawn = 0;
        if let Some(info) = info {
            if info.state == "stored" && self.blobs.has(&info.sha256) {
                self.blobs
                    .withdraw(&info.sha256)
                    .map_err(|e| format!("holding back the file: {e}"))?;
                withdrawn = 1;
            }
            self.store
                .set_held(&info.sha256, Some("blocked"), self.now)?;
            self.store.decide_reviews(&info.sha256, "block", self.now)?;
            lines.push(format!("withdrawn {withdrawn}"));
            lines.push(self.clear(&[info.sha256])?);
        } else {
            lines.push("not here yet: held back if it ever comes".to_owned());
        }
        self.store
            .log_decision("block", hash.text(), reason, self.now)?;
        Ok(lines)
    }

    /// Lifts a block, or lets a file the scanner held go: served again and
    /// never held by a later scan.
    pub fn unblock(&self, hash: &Hash) -> Result<Vec<String>, String> {
        let sha256 = self.kept(hash)?;
        let mut lifted = self.store.unblock(hash.text())?;
        if let Some(sha256) = &sha256 {
            lifted += self.store.unblock(sha256)?;
        }
        let mut released = 0;
        if let Some(info) = match &sha256 {
            Some(s) => self.store.blob_info(s)?,
            None => None,
        } {
            if info.state == "quarantined" {
                if !self.blobs.has(&info.sha256) {
                    self.blobs
                        .release(&info.sha256)
                        .map_err(|e| format!("putting the file back: {e}"))?;
                }
                self.store.set_allowed(&info.sha256)?;
                released = 1;
            }
            self.store.decide_reviews(&info.sha256, "keep", self.now)?;
        }
        self.store
            .log_decision("unblock", hash.text(), "", self.now)?;
        Ok(vec![
            format!("unblocked {lifted}"),
            format!("released {released}"),
        ])
    }

    /// Deletes a held file for good; without `yes`, says what it would do.
    pub fn forget(&self, sha256: &str, yes: bool) -> Result<Vec<String>, String> {
        let info = self
            .store
            .blob_info(sha256)?
            .filter(|b| b.state == "quarantined")
            .ok_or("only a held file can be forgotten: block it first")?;
        if !yes {
            return Ok(vec![
                format!(
                    "would delete {} ({} bytes) for good",
                    info.sha256, info.size
                ),
                "run again with --yes to delete it".to_owned(),
            ]);
        }
        if let Some(bucket) = self.bucket
            && self.store.copy_of(&info.sha256)?.as_deref() == Some("private")
        {
            bucket.delete(Which::Private, &r2::held_key(&info.sha256))?;
            self.store.set_copy(&info.sha256, "")?;
        }
        self.blobs
            .forget(&info.sha256)
            .map_err(|e| format!("deleting the file: {e}"))?;
        self.store.set_forgotten(&info.sha256)?;
        self.store
            .log_decision("forget", &info.sha256, "", self.now)?;
        Ok(vec!["forgotten 1".to_owned()])
    }

    /// Takes a project out of the catalog at its author's request; the files
    /// only it needs go into quarantine.
    pub fn remove(&self, reference: &str, reason: &str) -> Result<Vec<String>, String> {
        let id = self
            .project(reference)?
            .ok_or("no such project in the catalog")?;
        let only = self.store.remove_project(&id)?;
        let mut withdrawn = 0;
        for sha256 in &only {
            if self.blobs.has(sha256) {
                self.blobs
                    .withdraw(sha256)
                    .map_err(|e| format!("holding back a file: {e}"))?;
                withdrawn += 1;
            }
            self.store.set_held(sha256, None, self.now)?;
        }
        self.store.log_decision("remove", &id, reason, self.now)?;
        Ok(vec![
            format!("removed project {id}"),
            format!("withdrawn {withdrawn}"),
            self.clear(&only)?,
        ])
    }

    /// A project by an address of Modrinth or CurseForge, a slug or an ID.
    fn project(&self, reference: &str) -> Result<Option<String>, String> {
        let reference = reference.trim();
        let wanted = match crate::input::parse(reference) {
            Some(
                crate::input::Input::Modrinth { project, .. }
                | crate::input::Input::ModrinthFile { project, .. },
            ) => project,
            Some(crate::input::Input::CurseForge { slug, .. }) => slug,
            Some(crate::input::Input::Name(name)) => name,
            _ => return Err("not an address of a project".to_owned()),
        };
        self.store.find_project(&wanted)
    }

    /// What the catalog knows of a project or a file.
    pub fn find(&self, query: &str) -> Result<Vec<String>, String> {
        let Some(hash) = Hash::parse(query) else {
            let Some(id) = self.project(query)? else {
                return Ok(vec!["nothing found".to_owned()]);
            };
            let mut lines = self.store.project_report(&id)?;
            lines.extend(self.decided(&id)?);
            return Ok(lines);
        };
        let mut lines = Vec::new();
        let kept = self.kept(&hash)?;
        if let Some(sha256) = &kept
            && let Some(b) = self.store.blob_info(sha256)?
        {
            lines.push(format!(
                "file {} sha1 {} {} bytes {} {} {}{}{}",
                b.sha256,
                b.sha1,
                b.size,
                b.media_type,
                b.state,
                b.verdict,
                if b.signs.is_empty() {
                    String::new()
                } else {
                    format!(" {}", b.signs)
                },
                b.held
                    .map(|at| format!(" held since {}", day(at)))
                    .unwrap_or_default(),
            ));
            if b.allowed {
                lines.push("  allowed by the owner".to_owned());
            }
            if let Some(line) = self.virustotal_line(sha256, &b.verdict)? {
                lines.push(format!("  {line}"));
            }
            lines.extend(self.store.file_uses(sha256)?);
        }
        if let Some((_, reason)) = self.store.block_of(hash.text())? {
            lines.push(format!("  blocked: {reason}").trim_end().to_owned());
        }
        if lines.is_empty() {
            lines.push("nothing found".to_owned());
        }
        lines.extend(self.decided(hash.text())?);
        if let Some(sha256) = kept.filter(|s| s != hash.text()) {
            lines.extend(self.decided(&sha256)?);
        }
        Ok(lines)
    }

    fn decided(&self, subject: &str) -> Result<Vec<String>, String> {
        Ok(self
            .store
            .decisions(subject)?
            .into_iter()
            .map(|(at, action, reason)| format!("  {} {action} {reason}", day(at)))
            .map(|l| l.trim_end().to_owned())
            .collect())
    }

    /// What waits for a decision, oldest first.
    pub fn review(&self) -> Result<Vec<String>, String> {
        let mut lines = Vec::new();
        for (id, kind, subject, created) in self.store.open_reviews(100)? {
            let what = match kind.as_str() {
                "scan" => match self.store.blob_info(&subject)? {
                    Some(b) => format!("{subject} {} {} {} bytes", b.verdict, b.signs, b.size),
                    None => subject.clone(),
                },
                "report" => {
                    let (reasons, _) = self.store.reports(&subject)?;
                    let reasons: Vec<String> =
                        reasons.iter().map(|(r, n)| format!("{r} {n}")).collect();
                    format!("{subject} {}", reasons.join(", "))
                }
                _ => self
                    .store
                    .version_brief(&subject)?
                    .map_or_else(|| subject.clone(), |brief| format!("{subject} {brief}")),
            };
            lines.push(format!("#{id} {} {kind} {what}", day(created)));
            if kind == "scan"
                && let Some(line) = self.virustotal_line(&subject, "suspicious")?
            {
                lines.push(format!("  {line}"));
            }
            if kind == "scan" {
                lines.extend(self.store.file_uses(&subject)?.into_iter().take(3));
            }
            if kind == "report" {
                lines.extend(self.store.project_report(&subject)?.into_iter().take(2));
                let (_, texts) = self.store.reports(&subject)?;
                lines.extend(texts.into_iter().map(|t| format!("  \"{t}\"")));
            }
        }
        if lines.is_empty() {
            lines.push("nothing waits".to_owned());
        }
        Ok(lines)
    }

    /// How many wait, by kind: numbers only.
    pub fn review_count(&self) -> Result<Vec<String>, String> {
        let counts = self.store.review_counts()?;
        let total: i64 = counts.iter().map(|(_, n)| n).sum();
        let mut lines: Vec<String> = counts
            .into_iter()
            .filter(|(kind, _)| matches!(kind.as_str(), "scan" | "gone" | "report"))
            .map(|(kind, n)| format!("{kind} {n}"))
            .collect();
        lines.push(format!("total {total}"));
        let (states, flagged) = self.store.virustotal_counts()?;
        let of = |state: &str| {
            states
                .iter()
                .find(|(s, _)| s == state)
                .map_or(0, |(_, n)| *n)
        };
        lines.push(format!(
            "virustotal checked {}, with detections {flagged}, waiting {}",
            of("found"),
            of("waiting") + of("sent")
        ));
        Ok(lines)
    }

    /// The `virustotal: …` line of a file: known there, or one the scanner
    /// suspects and so asked about in turn.
    fn virustotal_line(&self, sha256: &str, verdict: &str) -> Result<Option<String>, String> {
        let on = self.virustotal.is_some();
        Ok(match self.store.virustotal(sha256)? {
            Some(row) => Some(crate::virustotal::line(&row, on)),
            None if verdict == "suspicious" => {
                Some(crate::virustotal::line(&Default::default(), on))
            }
            None => None,
        })
    }

    /// `virustotal status`: numbers only.
    pub fn virustotal_status(&self) -> Result<Vec<String>, String> {
        use crate::virustotal::{QUOTA, day_and_month};
        let (day, month) = day_and_month(self.now);
        let (today, this_month) = self.store.virustotal_usage(&day, &month)?;
        let (states, flagged) = self.store.virustotal_counts()?;
        let states: Vec<String> = states
            .iter()
            .map(|(state, n)| format!("{} {n}", state.replace('_', " ")))
            .collect();
        Ok(vec![
            format!(
                "virustotal {}",
                if self.virustotal.is_some() {
                    "on"
                } else {
                    "off"
                }
            ),
            format!(
                "requests today {today} of {}, this month {this_month} of {}",
                QUOTA.day, QUOTA.month
            ),
            if states.is_empty() {
                "no files yet".to_owned()
            } else {
                states.join(", ")
            },
            format!("with detections {flagged}"),
        ])
    }

    /// `virustotal check <hash>`: asks now, from the owner's share of the
    /// quota, about a kept file (suspected or not).
    pub fn virustotal_check(&self, hash: &Hash) -> Result<Vec<String>, String> {
        let api = self
            .virustotal
            .ok_or("VirusTotal is not set up (LAPI_MODS_VIRUSTOTAL_KEY)")?;
        let sha256 = self.kept(hash)?.ok_or("no such file here")?;
        let checked = crate::virustotal::check(
            self.store,
            self.blobs,
            api,
            &sha256,
            crate::virustotal::QUOTA,
            self.now,
        )?;
        let mut lines = vec![sha256.clone()];
        if checked == crate::virustotal::Checked::NoQuota {
            lines.push("no quota left: try again tomorrow (UTC)".to_owned());
        }
        if let Some(row) = self.store.virustotal(&sha256)? {
            lines.push(crate::virustotal::line(&row, true));
        }
        Ok(lines)
    }

    /// Adds projects to the catalog by their Modrinth or CurseForge address,
    /// as "Add to the catalog" does for a player (D-066); names are refused,
    /// since a search asks someone to choose.
    pub fn add(&self, inputs: &[String]) -> Result<Vec<String>, String> {
        use crate::input::Input;
        if inputs.is_empty() || inputs.len() > MAX_ADD {
            return Err(format!("give 1 to {MAX_ADD} addresses"));
        }
        let mut addresses = Vec::new();
        for input in inputs {
            let text = input.trim();
            match crate::input::parse(text) {
                Some(Input::Name(_) | Input::CurseForgeFile) | None => {
                    return Err(
                        "each is the address of a project on Modrinth or CurseForge".to_owned()
                    );
                }
                Some(_) => addresses.push(text),
            }
        }
        let mut lines = Vec::new();
        for text in addresses {
            let id = uuid::Uuid::now_v7().to_string();
            self.store.create_request(&id, OWNER, text, self.now)?;
            self.store.enqueue(
                &format!("resolve:{id}"),
                "resolve",
                &serde_json::json!({ "request": id }),
                Some(&id),
                None,
                self.now,
                None,
            )?;
            lines.push(format!("queued {text}"));
        }
        Ok(lines)
    }

    /// How the owner's additions went: how many in each state, and the
    /// addresses of those that failed with why.
    pub fn add_status(&self) -> Result<Vec<String>, String> {
        let requests = self.store.requests_of(OWNER, 1000)?;
        let mut states = std::collections::BTreeMap::<&str, usize>::new();
        let mut lines = Vec::new();
        for request in &requests {
            *states.entry(request.state.as_str()).or_default() += 1;
            if request.state == "failed" {
                lines.push(format!(
                    "failed {} ({})",
                    request.input,
                    request.error.as_deref().unwrap_or("?")
                ));
            }
        }
        let counts = states
            .iter()
            .map(|(state, n)| format!("{state} {n}"))
            .collect::<Vec<_>>();
        lines.insert(
            0,
            if counts.is_empty() {
                "no additions yet".to_owned()
            } else {
                counts.join(", ")
            },
        );
        Ok(lines)
    }

    /// The list of mods that do not start together (D-066): `list`, or `add`
    /// and `remove` with two mod IDs as the jars name them, the first being
    /// the one shown as not starting with the second.
    pub fn clash(&self, args: &[String]) -> Result<Vec<String>, String> {
        let pair = |a: &str, b: &str| {
            let clash = mod_meta::Clash {
                a: a.trim().to_ascii_lowercase(),
                b: b.trim().to_ascii_lowercase(),
            };
            if clash.is_clean() {
                Ok(clash)
            } else {
                Err("two different mod IDs: small letters, digits, _ - and . (1 to 64)".to_owned())
            }
        };
        match args {
            [list] if list == "list" => {
                let clashes = self.store.clashes()?;
                let mut lines: Vec<String> =
                    clashes.iter().map(|c| format!("{} {}", c.a, c.b)).collect();
                lines.push(format!("{} clashes", clashes.len()));
                Ok(lines)
            }
            [verb, a, b] if verb == "add" || verb == "remove" => {
                let clash = pair(a, b)?;
                let listed = verb == "add";
                let changed = self.store.set_clash(&clash, listed, self.now)?;
                Ok(vec![format!(
                    "clash {} {} {}",
                    clash.a,
                    clash.b,
                    match (changed, listed) {
                        (true, true) => "added",
                        (true, false) => "removed",
                        (false, true) => "listed already",
                        (false, false) => "not listed",
                    }
                )])
            }
            _ => {
                Err("usage: lapi-mods clash list | add <mod> <mod> | remove <mod> <mod>".to_owned())
            }
        }
    }

    /// The owner's collections in the catalog (D-072): `list`, `add <key>
    /// <kind> [game|-] [loader|-]`, `title` or `note <key> <ru|en|ar>
    /// [text]` (no text takes it away), `put <key> <project>…` in their order,
    /// and `remove <key>`.
    pub fn collection(&self, args: &[String]) -> Result<Vec<String>, String> {
        const USAGE: &str = "usage: lapi-mods collection list | add <key> <kind> [game|-] [loader|-] \
                             | title <key> <ru|en|ar> [text] | note <key> <ru|en|ar> [text] \
                             | put <key> <project>... | remove <key>";
        let key = |k: &str| {
            let k = k.trim();
            if crate::api::collection_key(k) {
                Ok(k.to_owned())
            } else {
                Err("a key is 1 to 32 small letters, digits and hyphens".to_owned())
            }
        };
        let optional = |v: Option<&String>| -> Result<Option<String>, String> {
            match v.map(|v| v.trim()) {
                None | Some("-" | "") => Ok(None),
                Some(v) => crate::model::word(v)
                    .filter(|w| w.len() <= 32)
                    .map(Some)
                    .ok_or_else(|| "a game version or loader is one word".to_owned()),
            }
        };
        match args {
            [list] if list == "list" => {
                let rows = self.store.all_collections()?;
                let mut lines: Vec<String> = rows
                    .iter()
                    .map(|r| {
                        let langs = |m: &serde_json::Map<String, serde_json::Value>| {
                            let mut l: Vec<&str> = m.keys().map(String::as_str).collect();
                            l.sort_unstable();
                            if l.is_empty() {
                                "-".to_owned()
                            } else {
                                l.join(",")
                            }
                        };
                        format!(
                            "{} {} {} {} {} projects, titles {}, notes {}",
                            r.key,
                            r.kind,
                            r.game.as_deref().unwrap_or("-"),
                            r.loader.as_deref().unwrap_or("-"),
                            r.count,
                            langs(&r.titles),
                            langs(&r.notes)
                        )
                    })
                    .collect();
                lines.push(format!("{} collections", rows.len()));
                Ok(lines)
            }
            [verb, k, kind, rest @ ..] if verb == "add" && rest.len() <= 2 => {
                let k = key(k)?;
                if crate::api::PRESETS.contains(&k.as_str()) {
                    return Err(format!("{k} is one of the catalog's own collections"));
                }
                let kind = kind.trim().to_ascii_lowercase();
                if !crate::model::KINDS.contains(&kind.as_str()) {
                    return Err(format!(
                        "a kind is one of {}",
                        crate::model::KINDS.join(", ")
                    ));
                }
                let game = optional(rest.first())?;
                let loader = optional(rest.get(1))?;
                self.store.set_collection(
                    &k,
                    &kind,
                    game.as_deref(),
                    loader.as_deref(),
                    self.now,
                )?;
                Ok(vec![format!("collection {k} set")])
            }
            [verb, k, lang, text @ ..] if verb == "title" || verb == "note" => {
                let k = key(k)?;
                let lang = lang.trim();
                if !matches!(lang, "ru" | "en" | "ar") {
                    return Err("a language is ru, en or ar".to_owned());
                }
                let text = text.join(" ");
                let text = text.trim();
                let longest = if verb == "title" { 64 } else { 160 };
                if text.chars().count() > longest || text.chars().any(char::is_control) {
                    return Err(format!("one line of at most {longest} characters"));
                }
                let field = if verb == "title" { "titles" } else { "notes" };
                if !self
                    .store
                    .set_collection_text(&k, field, lang, text, self.now)?
                {
                    return Err(format!("no collection {k}"));
                }
                Ok(vec![format!(
                    "collection {k} {verb} {lang} {}",
                    if text.is_empty() { "removed" } else { "set" }
                )])
            }
            [verb, k, projects @ ..] if verb == "put" => {
                let k = key(k)?;
                if projects.len() > 100 {
                    return Err("at most 100 projects".to_owned());
                }
                let mut found = Vec::new();
                let mut missing = 0;
                for project in projects {
                    match self.store.project_id(project.trim())? {
                        Some(id) if !found.contains(&id) => found.push(id),
                        Some(_) => {}
                        None => missing += 1,
                    }
                }
                match self.store.set_collection_items(&k, &found, self.now)? {
                    Some(put) => Ok(vec![format!(
                        "collection {k}: {put} projects, {missing} not found"
                    )]),
                    None => Err(format!("no collection {k}")),
                }
            }
            [verb, k] if verb == "remove" => {
                let k = key(k)?;
                if self.store.remove_collection(&k)? {
                    Ok(vec![format!("collection {k} removed")])
                } else {
                    Err(format!("no collection {k}"))
                }
            }
            _ => Err(USAGE.to_owned()),
        }
    }

    /// How many projects of a kind (`mod` when none is given) are in each
    /// category, and how many of the sites' slugs no category maps.
    pub fn categories(&self, args: &[String]) -> Result<Vec<String>, String> {
        let kind = match args {
            [] => "mod".to_owned(),
            [kind] if crate::model::KINDS.contains(&kind.as_str()) => kind.clone(),
            _ => {
                return Err(format!(
                    "usage: lapi-mods categories [{}]",
                    crate::model::KINDS.join("|")
                ));
            }
        };
        let search = crate::store::Search {
            kind: Some(kind.clone()),
            sort: "relevance",
            ..crate::store::Search::default()
        };
        let mut lines: Vec<String> = self
            .store
            .category_counts(&search)?
            .into_iter()
            .map(|(id, n)| format!("{id} {n}"))
            .collect();
        let (slugs, projects) = self.store.unmapped_categories(&search)?;
        lines.push(format!(
            "{kind}: {slugs} slugs in no category, {projects} projects in none"
        ));
        Ok(lines)
    }

    /// Gives a project another slug, for one the catalog named badly (as
    /// `fabric-api-modrinth-20953b` was); another project's slug is refused.
    /// The old slug leads to it only when a source has it.
    pub fn slug(&self, reference: &str, slug: &str) -> Result<Vec<String>, String> {
        let slug = slug.trim();
        if !plain_slug(slug) {
            return Err("a slug is 1 to 64 small letters, digits and hyphens inside".to_owned());
        }
        let found = self
            .project(reference)?
            .ok_or("no such project in the catalog")?;
        // A project joined into another: the one it joined.
        let id = self.store.project_id(&found)?.unwrap_or(found);
        let old = self.store.set_slug(&id, slug)?;
        Ok(vec![format!("slug {old} is now {slug}")])
    }

    /// The service's own checks: a line each, `ok` or `problem` with its
    /// numbers, and a last line `all ok` or `problems <n>`. `popular` is how
    /// many most downloaded projects of each kind are looked for (0: off).
    pub fn health(&self, popular: usize, backups: &std::path::Path) -> Result<Vec<String>, String> {
        let (day, month) = crate::virustotal::day_and_month(self.now);
        let h = self.store.health(self.now, &day, &month)?;
        // How long ago in hours; `never` for a time not yet recorded.
        let ago = |at: i64| {
            if at <= 0 {
                "never".to_owned()
            } else {
                format!("{} h ago", (self.now - at) / HOUR)
            }
        };
        let mut lines = Vec::new();
        let mut problems = 0;
        // `None`: the part is off.
        let mut check = |name: &str, fine: Option<bool>, numbers: String| match fine {
            Some(fine) => {
                if !fine {
                    problems += 1;
                }
                lines.push(format!(
                    "{name} {} {numbers}",
                    if fine { "ok" } else { "problem" }
                ));
            }
            None => lines.push(format!("{name} off")),
        };
        let waiting = self.now - h.queue_oldest;
        check(
            "queue",
            Some(waiting < QUEUE_LATE),
            format!("oldest waits {} h", waiting / HOUR),
        );
        check(
            "imports",
            Some(
                !(h.jobs_failed >= JOBS_FAILING && h.jobs_failed * 4 > h.jobs_done + h.jobs_failed),
            ),
            format!("{} done, {} failed in a day", h.jobs_done, h.jobs_failed),
        );
        check(
            "requests",
            Some(!(h.requests_failed >= REQUESTS_FAILING && h.requests_failed * 2 > h.requests)),
            format!("{}, {} failed in a day", h.requests, h.requests_failed),
        );
        let review = self.now - h.review_oldest;
        check(
            "review",
            Some(review < REVIEW_LATE),
            format!("oldest open {} days", review / DAY),
        );
        let free = self.blobs.free();
        let quota = self.blobs.quota();
        let kept = u64::try_from(h.kept_bytes).unwrap_or(0);
        let gb = |bytes: u64| bytes / crate::blobs::GB;
        check(
            "disk",
            // Free space near the reserve, or kept files over 90% of the
            // quota: soon nothing new is kept.
            Some(
                free >= self.blobs.min_free().saturating_mul(2)
                    && kept.saturating_mul(10) < quota.saturating_mul(9),
            ),
            format!(
                "{} GB free, kept {} GB of {} GB",
                gb(free),
                gb(kept),
                gb(quota)
            ),
        );
        // The daily copies of the database (lapi-mods-backup.timer).
        let copies = crate::store::backup_copies(backups);
        let newest = copies
            .last()
            .and_then(|p| p.metadata().ok())
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
        check(
            "backups",
            Some(newest > 0 && self.now - newest < DAILY_LATE),
            format!("{} copies, newest {}", copies.len(), ago(newest)),
        );
        let limit = crate::virustotal::BACKGROUND.month;
        check(
            "virustotal",
            self.virustotal
                .is_some()
                .then_some(h.virustotal_month * 10 < limit * 9),
            format!("{} of {limit} this month", h.virustotal_month),
        );
        let copied = self.now - h.database_copied;
        check(
            "r2",
            self.bucket
                .is_some()
                .then_some(h.r2_mismatched == 0 && copied < DAILY_LATE),
            format!(
                "{} wrong copies, database copied {}",
                h.r2_mismatched,
                ago(h.database_copied)
            ),
        );
        let looked = self.now - h.popular_seeded;
        check(
            "popular",
            (popular > 0).then_some(looked < DAILY_LATE),
            format!("looked {}", ago(h.popular_seeded)),
        );
        lines.push(if problems == 0 {
            "all ok".to_owned()
        } else {
            format!("problems {problems}")
        });
        Ok(lines)
    }

    /// The owner's decision on an item of the queue: `keep` lets it be,
    /// `block` blocks its files (a reported pack is removed).
    pub fn decide(&self, id: i64, decision: &str, reason: &str) -> Result<Vec<String>, String> {
        let (kind, subject, decided) = self
            .store
            .review_item(id)?
            .ok_or("no such item in the queue")?;
        if decided {
            return Err("that item is decided already".to_owned());
        }
        let mut lines = match (kind.as_str(), decision) {
            ("scan", "keep") => self.unblock(&Hash::Sha256(subject.clone()))?,
            ("scan", "block") => self.block(&Hash::Sha256(subject.clone()), reason)?,
            // A report: `block` removes the pack for good.
            ("report", "keep") => Vec::new(),
            ("report", "block") => self.remove(&subject, reason)?,
            (_, "keep") => Vec::new(),
            (_, "block") => {
                let mut lines = Vec::new();
                for sha256 in self.store.version_blobs(&subject)? {
                    lines.extend(self.block(&Hash::Sha256(sha256), reason)?);
                }
                lines
            }
            _ => return Err("decide with keep or block".to_owned()),
        };
        self.store.decide_reviews(&subject, decision, self.now)?;
        self.store
            .log_decision(decision, &subject, reason, self.now)?;
        lines.push(format!("decided #{id} {decision}"));
        Ok(lines)
    }
}

/// Cloudflare's cache in front of files-mods.lapi.sh, cleared through its
/// API with a token that may only purge the zone's cache.
pub struct Cloudflare {
    zone: String,
    token: String,
    curl: PathBuf,
}

/// Cloudflare takes at most this many addresses a call.
const PURGE_BATCH: usize = 30;

impl Cloudflare {
    /// `None` unless both look right: a zone ID and a token.
    pub fn new(zone: &str, token: &str, curl: PathBuf) -> Option<Self> {
        let zone = zone.trim();
        let token = token.trim();
        let zone_ok = zone.len() == 32 && zone.bytes().all(|b| b.is_ascii_hexdigit());
        let token_ok = (20..=200).contains(&token.len())
            && token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        (zone_ok && token_ok).then(|| Self {
            zone: zone.to_ascii_lowercase(),
            token: token.to_owned(),
            curl,
        })
    }
}

/// What curl reads on its standard input for one call: the token never
/// appears in a command line.
pub fn purge_config(zone: &str, token: &str, urls: &[String]) -> String {
    let body = serde_json::json!({ "files": urls }).to_string();
    let quoted = body.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "url = \"https://api.cloudflare.com/client/v4/zones/{zone}/purge_cache\"\n\
         request = \"POST\"\n\
         header = \"Authorization: Bearer {token}\"\n\
         header = \"Content-Type: application/json\"\n\
         data = \"{quoted}\"\n"
    )
}

impl Purge for Cloudflare {
    fn purge(&self, urls: &[String]) -> Result<usize, String> {
        let mut cleared = 0;
        for batch in urls.chunks(PURGE_BATCH) {
            let mut child = Command::new(&self.curl)
                .args([
                    "-sS",
                    "--max-time",
                    "30",
                    "-o",
                    "/dev/null",
                    "-w",
                    "%{http_code}",
                    "-K",
                    "-",
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| "curl did not start".to_owned())?;
            if let Some(mut stdin) = child.stdin.take() {
                stdin
                    .write_all(purge_config(&self.zone, &self.token, batch).as_bytes())
                    .map_err(|_| "curl did not take the request".to_owned())?;
            }
            let output = child
                .wait_with_output()
                .map_err(|_| "curl did not finish".to_owned())?;
            let code: u16 = String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse()
                .unwrap_or(0);
            if code != 200 {
                return Err(format!("Cloudflare answered {code}"));
            }
            cleared += batch.len();
        }
        Ok(cleared)
    }
}
