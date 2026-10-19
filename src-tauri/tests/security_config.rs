//! Guards the trust boundaries from docs/spec/07-PACKAGING-AND-SECURITY.md against
//! accidental loosening of the checked-in Tauri configuration.

use serde_json::Value;

const TAURI_CONF: &str = include_str!("../tauri.conf.json");
const DEFAULT_CAPABILITY: &str = include_str!("../capabilities/default.json");
const CONSOLE_CAPABILITY: &str = include_str!("../capabilities/console.json");
const PACKAGE_JSON: &str = include_str!("../../package.json");

fn json(source: &str) -> Value {
    serde_json::from_str(source).expect("valid JSON")
}

fn directives(csp: &str) -> Vec<(String, Vec<String>)> {
    csp.split(';')
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(|d| {
            let mut parts = d.split_whitespace().map(str::to_owned);
            let name = parts.next().expect("directive name");
            (name, parts.collect())
        })
        .collect()
}

fn source_list<'a>(csp: &'a [(String, Vec<String>)], name: &str) -> &'a [String] {
    &csp.iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("CSP lacks {name}"))
        .1
}

#[test]
fn production_csp_allows_only_packaged_assets_and_ipc() {
    let conf = json(TAURI_CONF);
    let csp = conf["app"]["security"]["csp"]
        .as_str()
        .expect("production CSP must be set");
    let csp = directives(csp);

    for name in [
        "default-src",
        "script-src",
        "style-src",
        "connect-src",
        "object-src",
        "base-uri",
    ] {
        source_list(&csp, name);
    }
    assert_eq!(source_list(&csp, "script-src"), ["'self'"]);
    assert_eq!(source_list(&csp, "object-src"), ["'none'"]);
    assert_eq!(source_list(&csp, "base-uri"), ["'none'"]);
    // Screenshots come only from the gallery protocol that Rust serves (D-050).
    assert_eq!(
        source_list(&csp, "img-src"),
        ["'self'", "data:", "gallery:", "http://gallery.localhost"]
    );

    for (name, sources) in &csp {
        for source in sources {
            assert!(
                !matches!(
                    source.as_str(),
                    "'unsafe-eval'" | "'unsafe-inline'" | "*" | "http:" | "https:" | "data:"
                ) || (name == "img-src" && source == "data:"),
                "{name} must not allow {source}"
            );
            if source.starts_with("http://") || source.starts_with("https://") {
                // Windows serves Tauri IPC and the app's own protocols from these
                // origins; nothing remote is allowed.
                assert!(
                    source == "http://ipc.localhost"
                        || (name == "img-src" && source == "http://gallery.localhost"),
                    "{name} allows remote {source}"
                );
            }
        }
    }
}

#[test]
fn development_csp_is_separate_and_local_only() {
    let conf = json(TAURI_CONF);
    let dev = conf["app"]["security"]["devCsp"]
        .as_str()
        .expect("dev CSP must be explicit");
    assert_ne!(Some(dev), conf["app"]["security"]["csp"].as_str());
    for (name, sources) in directives(dev) {
        for source in sources {
            assert_ne!(source, "'unsafe-eval'", "{name}");
            if source.contains("://") {
                assert!(
                    source == "http://ipc.localhost"
                        || source == "ws://localhost:1420"
                        || source == "http://gallery.localhost",
                    "{name} allows non-local {source}"
                );
            }
        }
    }
}

#[test]
fn frontend_is_packaged_locally() {
    let conf = json(TAURI_CONF);
    assert_eq!(conf["build"]["frontendDist"], "../dist");
    assert_eq!(conf["build"]["devUrl"], "http://localhost:1420");
    let windows = conf["app"]["windows"].as_array().expect("windows");
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0]["label"], "main");
    assert!(
        windows[0].get("url").is_none(),
        "window must load packaged UI"
    );
}

#[test]
fn main_window_capability_grants_only_app_commands() {
    let capability = json(DEFAULT_CAPABILITY);
    assert_eq!(capability["windows"], serde_json::json!(["main"]));
    assert!(
        capability.get("remote").is_none(),
        "no remote origin may receive capabilities"
    );
    let permissions: Vec<&str> = capability["permissions"]
        .as_array()
        .expect("permissions")
        .iter()
        .map(|p| p.as_str().expect("plain permission identifier"))
        .collect();
    assert_eq!(
        permissions,
        [
            "core:event:allow-listen",
            "core:event:allow-unlisten",
            "allow-get-app-info",
            "allow-get-app-snapshot",
            "allow-create-instance",
            "allow-update-instance-settings",
            "allow-set-instance-notes",
            "allow-set-app-settings",
            "allow-show-main-window",
            "allow-open-console",
            "allow-get-data-location",
            "allow-move-data-location",
            "allow-pick-data-folder",
            "allow-check-java",
            "allow-set-instance-java",
            "allow-set-instance-loader",
            "allow-pick-java-file",
            "allow-list-backups",
            "allow-create-backup",
            "allow-restore-backup",
            "allow-delete-backup",
            "allow-inspect-instance",
            "allow-delete-instance",
            "allow-reset-preview",
            "allow-reset-launcher",
            "allow-recommend-memory",
            "allow-list-screenshots",
            "allow-delete-screenshot",
            "allow-open-screenshot",
            "allow-request-quit",
            "allow-begin-login",
            "allow-cancel-login",
            "allow-sign-out",
            "allow-verify-account",
            "allow-add-notoken-profile",
            "allow-remove-notoken-profile",
            "allow-set-default-profile",
            "allow-set-instance-profile",
            "allow-open-external",
            "allow-list-supported-versions",
            "allow-preview-install-plan",
            "allow-list-loader-game-versions",
            "allow-list-loader-versions",
            "allow-install-instance",
            "allow-repair-instance",
            "allow-cancel-operation",
            "allow-answer-download",
            "allow-check-network",
            "allow-get-traffic",
            "allow-reset-traffic",
            "allow-launch-game",
            "allow-stop-game",
            "allow-open-instance-folder",
            "allow-export-diagnostics",
            "allow-report-frontend-error",
            "allow-note-shown-error",
            "allow-list-launcher-failures",
            "allow-dismiss-launcher-failure",
            "allow-preview-crash-report",
            "allow-send-crash-report",
            "allow-send-explanation-feedback",
            "allow-check-for-update",
            "allow-download-update",
            "allow-restart-to-update",
            "allow-dismiss-whats-new",
            "allow-open-update-page",
            "allow-interface-ready",
            "allow-launcher-account-open",
            "allow-launcher-account-sign-out",
            "allow-launcher-account-set-nick",
            "allow-launcher-account-resend-confirmation",
            "allow-friends-state",
            "allow-friends-request",
            "allow-friends-act",
            "allow-friends-open-direct",
            "allow-friends-create-group",
            "allow-friends-change-room",
            "allow-friends-messages",
            "allow-friends-send",
            "allow-friends-read",
            "allow-friends-report",
            "allow-start-lifecycle-test",
            "allow-stop-lifecycle-test",
        ]
    );
}

/// The console window (D-037) reads the journal and retitles itself (D-038),
/// nothing else: no events, no commands that change state or open anything.
#[test]
fn console_window_capability_only_reads_the_journal() {
    let capability = json(CONSOLE_CAPABILITY);
    assert_eq!(capability["windows"], serde_json::json!(["console"]));
    assert!(capability.get("remote").is_none());
    assert_eq!(
        capability["permissions"],
        serde_json::json!(["allow-read-console", "core:window:allow-set-title"])
    );
    let capabilities: Vec<_> =
        std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/capabilities"))
            .expect("capabilities")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
    let mut sorted = capabilities.clone();
    sorted.sort();
    assert_eq!(sorted, ["console.json", "default.json"]);
}

#[test]
fn versions_are_consistent() {
    let conf = json(TAURI_CONF);
    assert!(
        conf.get("version").is_none(),
        "tauri.conf.json inherits the Cargo version"
    );
    assert_eq!(json(PACKAGE_JSON)["version"], env!("CARGO_PKG_VERSION"));
}

/// The lifecycle test harness must not exist in release builds (P03 "test controls
/// cannot launch arbitrary processes from the release UI").
#[test]
fn harness_commands_are_registered_only_in_debug_builds() {
    let lib = include_str!("../src/lib.rs");
    let start = lib.find("tauri::generate_handler![").expect("handler list");
    let list = &lib[start..start + lib[start..].find("\n    ]").expect("end of handler list")];
    let lines: Vec<&str> = list.lines().map(str::trim).collect();
    let harness: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i].contains("lifecycle_test"))
        .collect();
    assert_eq!(harness.len(), 2, "both harness commands are listed");
    for i in harness {
        assert_eq!(
            lines[i - 1],
            "#[cfg(debug_assertions)]",
            "{} is registered in release builds",
            lines[i]
        );
    }
    let main = include_str!("../src/main.rs");
    assert!(main.contains("#[cfg(debug_assertions)]"));
}
