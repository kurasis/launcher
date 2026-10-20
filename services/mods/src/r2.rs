//! Cloudflare R2 (D-066, stage 6): kept files are served from a public
//! bucket at cdn-mods.lapi.sh, and our disk stays the reserve. A private
//! bucket keeps copies of held files and of the database.
//!
//! Requests go through the plain curl with AWS signatures (`--aws-sigv4`);
//! the key and secret reach it on standard input, never in a command line.
//! An upload names its SHA-256 in `x-amz-content-sha256`, so R2 refuses
//! bytes that do not match it.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Which bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    /// What players download, behind cdn-mods.lapi.sh.
    Public,
    /// Held files and database copies; nobody downloads from it.
    Private,
}

/// The two buckets, as the service and the owner's commands use them.
pub trait Bucket: Send + Sync {
    /// Uploads a file under `key`; R2 checks it against `sha256`.
    fn put(
        &self,
        which: Which,
        key: &str,
        path: &Path,
        sha256: &str,
        media_type: &str,
    ) -> Result<(), String>;
    /// Deletes an object; a missing one is no error.
    fn delete(&self, which: Which, key: &str) -> Result<(), String>;
    /// An object's size, `None` when there is none.
    fn size(&self, which: Which, key: &str) -> Result<Option<u64>, String>;
    /// Downloads an object into `to`; `false` when there is none.
    fn get(&self, which: Which, key: &str, to: &Path) -> Result<bool, String>;
}

/// Where a kept file lies in the public bucket.
pub fn file_key(sha256: &str) -> String {
    format!("f/{sha256}")
}

/// Where a held file's copy lies in the private bucket.
pub fn held_key(sha256: &str) -> String {
    format!("q/{sha256}")
}

/// A database copy of a day, and the one kept for its month.
pub fn database_keys(day: chrono::NaiveDate) -> (String, String) {
    (
        format!("db/daily/{}.db", day.format("%Y-%m-%d")),
        format!("db/monthly/{}.db", day.format("%Y-%m")),
    )
}

/// R2 through curl.
pub struct R2 {
    account: String,
    key: String,
    secret: String,
    public: String,
    private: String,
    curl: PathBuf,
}

fn hex_of(text: &str, len: usize) -> bool {
    text.len() == len && text.bytes().all(|b| b.is_ascii_hexdigit())
}

fn bucket_name(text: &str) -> bool {
    (3..=63).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !text.starts_with('-')
        && !text.ends_with('-')
}

/// What one call to curl ends with.
enum Answer {
    Status(u16),
    Failed,
}

impl R2 {
    /// `None` unless every value looks right: the account ID, an access
    /// key's ID and secret, and two bucket names.
    pub fn new(
        account: &str,
        key: &str,
        secret: &str,
        public: &str,
        private: &str,
        curl: PathBuf,
    ) -> Option<Self> {
        let (account, key, secret) = (account.trim(), key.trim(), secret.trim());
        (hex_of(account, 32)
            && hex_of(key, 32)
            && hex_of(secret, 64)
            && bucket_name(public)
            && bucket_name(private)
            && public != private)
            .then(|| Self {
                account: account.to_ascii_lowercase(),
                key: key.to_owned(),
                secret: secret.to_owned(),
                public: public.to_owned(),
                private: private.to_owned(),
                curl,
            })
    }

    fn url(&self, which: Which, key: &str) -> String {
        let bucket = match which {
            Which::Public => &self.public,
            Which::Private => &self.private,
        };
        format!(
            "https://{}.r2.cloudflarestorage.com/{bucket}/{key}",
            self.account
        )
    }

    /// Runs curl with `args`; the credentials go on its standard input.
    fn run(&self, args: &[&str]) -> Answer {
        let Ok(mut child) = Command::new(&self.curl)
            .args(["-sS", "--max-time", "600", "-w", "%{http_code}", "-K", "-"])
            .args(["--aws-sigv4", "aws:amz:auto:s3"])
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        else {
            return Answer::Failed;
        };
        if let Some(mut stdin) = child.stdin.take()
            && stdin
                .write_all(credentials(&self.key, &self.secret).as_bytes())
                .is_err()
        {
            let _ = child.kill();
            let _ = child.wait();
            return Answer::Failed;
        }
        match child.wait_with_output() {
            Ok(output) => {
                // The status is the last thing curl prints.
                let text = String::from_utf8_lossy(&output.stdout);
                let code = text.get(text.len().saturating_sub(3)..).unwrap_or("");
                code.parse().map_or(Answer::Failed, Answer::Status)
            }
            Err(_) => Answer::Failed,
        }
    }
}

/// curl's config for the signature: the key and its secret.
pub fn credentials(key: &str, secret: &str) -> String {
    format!("user = \"{key}:{secret}\"\n")
}

/// The arguments of an upload, after the common ones.
pub fn put_args(path: &str, sha256: &str, media_type: &str, url: &str) -> Vec<String> {
    let mut args = vec![
        "-o".to_owned(),
        "/dev/null".to_owned(),
        "-T".to_owned(),
        path.to_owned(),
        "-H".to_owned(),
        format!("x-amz-content-sha256: {sha256}"),
        "-H".to_owned(),
        format!("Content-Type: {media_type}"),
        "-H".to_owned(),
        "Cache-Control: public, max-age=31536000, immutable".to_owned(),
    ];
    if !media_type.starts_with("image/") {
        args.extend([
            "-H".to_owned(),
            "Content-Disposition: attachment".to_owned(),
        ]);
    }
    args.push(url.to_owned());
    args
}

fn failed(what: &str, answer: &Answer) -> String {
    match answer {
        Answer::Status(code) => format!("R2 answered {code} to {what}"),
        Answer::Failed => format!("R2 could not be asked to {what}"),
    }
}

impl Bucket for R2 {
    fn put(
        &self,
        which: Which,
        key: &str,
        path: &Path,
        sha256: &str,
        media_type: &str,
    ) -> Result<(), String> {
        let path = path.to_str().ok_or("a path that is not text")?;
        let args = put_args(path, sha256, media_type, &self.url(which, key));
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        match self.run(&args) {
            Answer::Status(200) => Ok(()),
            other => Err(failed("an upload", &other)),
        }
    }

    fn delete(&self, which: Which, key: &str) -> Result<(), String> {
        match self.run(&["-o", "/dev/null", "-X", "DELETE", &self.url(which, key)]) {
            Answer::Status(200 | 204 | 404) => Ok(()),
            other => Err(failed("a deletion", &other)),
        }
    }

    fn size(&self, which: Which, key: &str) -> Result<Option<u64>, String> {
        // The size comes in the header curl writes out before the status.
        let url = self.url(which, key);
        let Ok(mut child) = Command::new(&self.curl)
            .args(["-sS", "--max-time", "60", "-K", "-"])
            .args(["--aws-sigv4", "aws:amz:auto:s3"])
            .args([
                "-I",
                "-o",
                "/dev/null",
                "-w",
                "%{http_code} %header{content-length}",
                &url,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        else {
            return Err(failed("a size", &Answer::Failed));
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(credentials(&self.key, &self.secret).as_bytes());
        }
        let output = child
            .wait_with_output()
            .map_err(|_| failed("a size", &Answer::Failed))?;
        let text = String::from_utf8_lossy(&output.stdout);
        let mut words = text.split_whitespace();
        match words.next().and_then(|c| c.parse::<u16>().ok()) {
            Some(200) => Ok(words.next().and_then(|n| n.parse().ok())),
            Some(404) => Ok(None),
            Some(code) => Err(failed("a size", &Answer::Status(code))),
            None => Err(failed("a size", &Answer::Failed)),
        }
    }

    fn get(&self, which: Which, key: &str, to: &Path) -> Result<bool, String> {
        let to_text = to.to_str().ok_or("a path that is not text")?;
        match self.run(&["-o", to_text, &self.url(which, key)]) {
            Answer::Status(200) => Ok(true),
            Answer::Status(404) => {
                let _ = std::fs::remove_file(to);
                Ok(false)
            }
            other => {
                let _ = std::fs::remove_file(to);
                Err(failed("a download", &other))
            }
        }
    }
}
