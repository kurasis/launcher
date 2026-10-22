//! Keeping R2 in step with the disk (D-066, stage 6), in the background:
//! served files are copied to the public bucket (and the API points at them
//! only once a copy is there), held files leave it and get a copy in the
//! private bucket, forgotten ones leave that too. Once a day the newest
//! database copy goes to the private bucket; once a week a sample of the
//! public bucket is checked against the disk. `lapi-mods restore` brings
//! back to the disk what only R2 still has.

use std::sync::Arc;

use crate::App;
use crate::blobs::Blobs;
use crate::import::now;
use crate::r2::{self, Bucket, Which};
use crate::store::Store;

/// How often the database is copied, and how often R2 is checked.
const DAY: i64 = 86_400;
const WEEK: i64 = 7 * DAY;
/// Daily database copies older than this many days are deleted (monthly
/// ones stay).
const DAILY_KEPT: i64 = 30;
/// Files of the public bucket checked each week.
const SAMPLE: i64 = 50;

/// Clears a kept file's addresses from Cloudflare's cache (our disk's and
/// R2's), when a token is set. Returns how many were cleared.
pub fn clear(
    store: &Store,
    urls: &crate::Urls,
    purge: Option<&dyn crate::owner::Purge>,
    sha256s: &[String],
) -> Result<Option<usize>, String> {
    let Some(purge) = purge else {
        return Ok(None);
    };
    let urls = crate::owner::addresses(store, urls, sha256s)?;
    if urls.is_empty() {
        return Ok(Some(0));
    }
    purge.purge(&urls).map(Some)
}

/// One step for one kept file; `Err` when R2 failed (the round stops).
fn step(
    store: &Store,
    blobs: &Blobs,
    bucket: &dyn Bucket,
    urls: &crate::Urls,
    purge: Option<&dyn crate::owner::Purge>,
    (sha256, media_type, state, copy): (String, String, String, String),
) -> Result<(), String> {
    match (state.as_str(), copy.as_str()) {
        ("stored", _) => {
            let path = blobs.path(&sha256);
            if !path.is_file() {
                // Lost on disk: `lapi-mods restore` brings it back first.
                return Ok(());
            }
            bucket.put(
                Which::Public,
                &r2::file_key(&sha256),
                &path,
                &sha256,
                &media_type,
            )?;
            store.set_copy(&sha256, "public")?;
            if copy == "private" {
                bucket.delete(Which::Private, &r2::held_key(&sha256))?;
            }
        }
        (_, "public") => {
            bucket.delete(Which::Public, &r2::file_key(&sha256))?;
            if let Err(e) = clear(store, urls, purge, std::slice::from_ref(&sha256)) {
                eprintln!(
                    "lapi-mods: clearing the cache: {}",
                    crate::model::clean(&e, 200, false)
                );
            }
            store.set_copy(&sha256, "")?;
        }
        ("quarantined", "") => {
            let path = blobs.held_path(&sha256);
            if path.is_file() {
                bucket.put(
                    Which::Private,
                    &r2::held_key(&sha256),
                    &path,
                    &sha256,
                    "application/octet-stream",
                )?;
                store.set_copy(&sha256, "private")?;
            }
        }
        ("forgotten", "private") => {
            bucket.delete(Which::Private, &r2::held_key(&sha256))?;
            store.set_copy(&sha256, "")?;
        }
        _ => {}
    }
    Ok(())
}

/// One round of copying: at most `limit` files. Returns how many were due.
pub async fn copy(app: &Arc<App>, limit: i64) -> Result<usize, String> {
    let Some(bucket) = app.r2.clone() else {
        return Ok(0);
    };
    let due = app.store.to_copy(limit)?;
    let count = due.len();
    let app = Arc::clone(app);
    tokio::task::spawn_blocking(move || {
        for row in due {
            step(
                &app.store,
                &app.blobs,
                bucket.as_ref(),
                &app.urls,
                app.purge.as_deref(),
                row,
            )?;
        }
        Ok::<_, String>(())
    })
    .await
    .map_err(|_| "copying stopped".to_owned())??;
    Ok(count)
}

/// A first copy of the database when there is none yet: the daily timer
/// makes the next ones, and a new server should not wait a day for one.
pub fn first_backup(store: &Store, backups: &std::path::Path) -> Result<bool, String> {
    if !crate::store::backup_copies(backups).is_empty() {
        return Ok(false);
    }
    store.backup(backups, 14)?;
    Ok(true)
}

/// Copies the newest database copy (lapi-mods-backup.timer makes one a day)
/// to the private bucket once a day; daily copies are kept for a month,
/// one a month for good.
pub async fn copy_database(app: &Arc<App>, backups: std::path::PathBuf) -> Result<bool, String> {
    let Some(bucket) = app.r2.clone() else {
        return Ok(false);
    };
    if app
        .store
        .upkeep("database_copied")?
        .is_some_and(|at| now() - at < DAY - 3600)
    {
        return Ok(false);
    }
    let Some(newest) = crate::store::backup_copies(&backups).pop() else {
        return Ok(false);
    };
    let store = Arc::clone(app);
    tokio::task::spawn_blocking(move || {
        let hashed = crate::blobs::hash_file(&newest).map_err(|e| e.to_string())?;
        let today = chrono::Utc::now().date_naive();
        let (daily, monthly) = r2::database_keys(today);
        for key in [&daily, &monthly] {
            bucket.put(
                Which::Private,
                key,
                &newest,
                &hashed.sha256,
                "application/vnd.sqlite3",
            )?;
        }
        // Days that fell out of the month (a few, in case some were missed).
        for back in DAILY_KEPT + 1..=DAILY_KEPT + 10 {
            let day = today - chrono::Duration::days(back);
            bucket.delete(Which::Private, &r2::database_keys(day).0)?;
        }
        store.store.set_upkeep("database_copied", now())
    })
    .await
    .map_err(|_| "copying the database stopped".to_owned())??;
    Ok(true)
}

/// Once a week: a sample of the public bucket against the disk. A missing
/// or wrong copy is copied again.
pub async fn check(app: &Arc<App>) -> Result<Option<i64>, String> {
    let Some(bucket) = app.r2.clone() else {
        return Ok(None);
    };
    if app
        .store
        .upkeep("r2_checked")?
        .is_some_and(|at| now() - at < WEEK)
    {
        return Ok(None);
    }
    let sample = app.store.sample_public(SAMPLE)?;
    let app = Arc::clone(app);
    tokio::task::spawn_blocking(move || {
        let mut wrong = 0;
        for (sha256, size) in sample {
            let found = bucket.size(Which::Public, &r2::file_key(&sha256))?;
            if found != u64::try_from(size).ok() {
                wrong += 1;
                app.store.set_copy(&sha256, "")?;
            }
        }
        app.store.set_upkeep("r2_checked", now())?;
        app.store.set_upkeep("r2_mismatched", wrong)?;
        Ok::<_, String>(Some(wrong))
    })
    .await
    .map_err(|_| "checking stopped".to_owned())?
}

/// Brings back to the disk the kept files only R2 still has, each checked
/// against its SHA-256. Numbers only.
pub fn restore(store: &Store, blobs: &Blobs, bucket: &dyn Bucket) -> Result<Vec<String>, String> {
    let (mut restored, mut missing, mut wrong, mut there) = (0, 0, 0, 0);
    let mut after = String::new();
    loop {
        let page = store.copied(&after, 500)?;
        let Some(last) = page.last() else {
            break;
        };
        after = last.0.clone();
        for (sha256, state, copy, _) in page {
            let (target, which, key) = match (state.as_str(), copy.as_str()) {
                ("stored", "public") => (blobs.path(&sha256), Which::Public, r2::file_key(&sha256)),
                ("quarantined", "private") => (
                    blobs.held_path(&sha256),
                    Which::Private,
                    r2::held_key(&sha256),
                ),
                _ => continue,
            };
            if target.is_file() {
                there += 1;
                continue;
            }
            let part = blobs.part();
            if !bucket.get(which, &key, &part)? {
                missing += 1;
                continue;
            }
            let hashed = crate::blobs::hash_file(&part).map_err(|e| e.to_string())?;
            if hashed.sha256 != sha256 {
                let _ = std::fs::remove_file(&part);
                wrong += 1;
                continue;
            }
            let placed = if which == Which::Public {
                blobs.keep(&part, &hashed)
            } else {
                blobs.hold(&part, &hashed)
            };
            placed.map_err(|e| format!("placing a file: {e}"))?;
            restored += 1;
        }
    }
    Ok(vec![
        format!("restored {restored}"),
        format!("already there {there}"),
        format!("missing in R2 {missing}"),
        format!("wrong in R2 {wrong}"),
        "the service copies what R2 lacks on its own".to_owned(),
    ])
}
