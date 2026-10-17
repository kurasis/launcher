//! Ordered library/argument rules (docs/spec/04 "Rules engine").
//!
//! No `rules` property: allowed. Otherwise start disallowed and evaluate in order;
//! each fully matching rule sets the result to its action, so the last match wins.
//! All predicates in a rule must match. Unknown predicates or actions are errors,
//! never silently ignored, so an unsupported rule cannot broaden applicability.

use std::collections::BTreeMap;

use regex::Regex;
use serde_json::Value;

use crate::error::{ErrorCode, LauncherError, LauncherResult};

/// The OS names used by Minecraft metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetaOs {
    Windows,
    Osx,
    Linux,
}

impl MetaOs {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Osx => "osx",
            Self::Linux => "linux",
        }
    }
}

/// Evaluation target: OS, JVM architecture in metadata naming, native OS version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleContext {
    pub os: MetaOs,
    /// `x86_64` or `arm64`. Metadata `arch: "x86"` means a 32-bit JVM and never
    /// matches the supported 64-bit targets.
    pub arch: &'static str,
    /// Native OS version for `os.version` regexes; `None` when unknown.
    pub os_version: Option<String>,
    /// Feature flags; a missing feature counts as `false`.
    pub features: BTreeMap<String, bool>,
}

impl RuleContext {
    /// MVP feature set: custom resolution supplied, no demo, no quick play.
    pub fn with_default_features(
        os: MetaOs,
        arch: &'static str,
        os_version: Option<String>,
    ) -> Self {
        let features = BTreeMap::from([
            ("is_demo_user".to_owned(), false),
            ("has_custom_resolution".to_owned(), true),
            ("has_quick_plays_support".to_owned(), false),
            ("is_quick_play_singleplayer".to_owned(), false),
            ("is_quick_play_multiplayer".to_owned(), false),
            ("is_quick_play_realms".to_owned(), false),
        ]);
        Self {
            os,
            arch,
            os_version,
            features,
        }
    }

    /// The context for the machine this launcher was built for.
    pub fn current() -> LauncherResult<Self> {
        let os = if cfg!(windows) {
            MetaOs::Windows
        } else if cfg!(target_os = "macos") {
            MetaOs::Osx
        } else if cfg!(target_os = "linux") {
            MetaOs::Linux
        } else {
            return Err(unsupported_platform());
        };
        let arch = match std::env::consts::ARCH {
            "x86_64" => "x86_64",
            "aarch64" => "arm64",
            _ => return Err(unsupported_platform()),
        };
        Ok(Self::with_default_features(
            os,
            arch,
            sysinfo::System::os_version(),
        ))
    }
}

fn unsupported_platform() -> LauncherError {
    LauncherError::new(
        ErrorCode::UnsupportedPlatform,
        "This operating system or processor is not supported.",
    )
}

fn rule_error(detail: &str) -> LauncherError {
    LauncherError::new(
        ErrorCode::UnsupportedVersionMetadata,
        "This version's metadata uses a rule the launcher does not support.",
    )
    .with_detail("rule", detail.to_owned())
}

/// Evaluates an optional `rules` array. Returns whether the item applies.
pub fn evaluate(rules: Option<&Value>, ctx: &RuleContext) -> LauncherResult<bool> {
    let Some(rules) = rules else {
        return Ok(true);
    };
    let rules = rules
        .as_array()
        .ok_or_else(|| rule_error("rules is not an array"))?;
    let mut allowed = false;
    for rule in rules {
        let rule = rule
            .as_object()
            .ok_or_else(|| rule_error("rule is not an object"))?;
        let action = match rule.get("action").and_then(Value::as_str) {
            Some("allow") => true,
            Some("disallow") => false,
            _ => return Err(rule_error("unknown action")),
        };
        let mut matches = true;
        for (key, predicate) in rule {
            match key.as_str() {
                "action" => {}
                "os" => matches &= os_matches(predicate, ctx)?,
                "features" => matches &= features_match(predicate, ctx)?,
                other => return Err(rule_error(other)),
            }
        }
        if matches {
            allowed = action;
        }
    }
    Ok(allowed)
}

fn os_matches(predicate: &Value, ctx: &RuleContext) -> LauncherResult<bool> {
    let predicate = predicate
        .as_object()
        .ok_or_else(|| rule_error("os is not an object"))?;
    let mut matches = true;
    for (key, value) in predicate {
        let value = value.as_str().ok_or_else(|| rule_error("os value"))?;
        match key.as_str() {
            "name" => matches &= value == ctx.os.as_str(),
            "arch" => matches &= value == ctx.arch,
            "version" => {
                let regex = Regex::new(value).map_err(|_| rule_error("invalid os.version"))?;
                let version = ctx
                    .os_version
                    .as_deref()
                    .ok_or_else(|| rule_error("os.version needs the OS version"))?;
                matches &= regex.is_match(version);
            }
            other => return Err(rule_error(other)),
        }
    }
    Ok(matches)
}

fn features_match(predicate: &Value, ctx: &RuleContext) -> LauncherResult<bool> {
    let predicate = predicate
        .as_object()
        .ok_or_else(|| rule_error("features is not an object"))?;
    let mut matches = true;
    for (name, expected) in predicate {
        let expected = expected
            .as_bool()
            .ok_or_else(|| rule_error("feature value"))?;
        let actual = ctx.features.get(name).copied().unwrap_or(false);
        matches &= actual == expected;
    }
    Ok(matches)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn ctx(os: MetaOs) -> RuleContext {
        RuleContext::with_default_features(os, "x86_64", Some("10.0.22631".into()))
    }

    fn eval(rules: Value, os: MetaOs) -> bool {
        evaluate(Some(&rules), &ctx(os)).unwrap()
    }

    #[test]
    fn no_rules_means_allowed() {
        assert!(evaluate(None, &ctx(MetaOs::Linux)).unwrap());
    }

    #[test]
    fn rules_start_disallowed() {
        assert!(!eval(json!([]), MetaOs::Linux));
        assert!(!eval(
            json!([{"action": "allow", "os": {"name": "osx"}}]),
            MetaOs::Linux
        ));
        assert!(eval(
            json!([{"action": "allow", "os": {"name": "osx"}}]),
            MetaOs::Osx
        ));
    }

    #[test]
    fn last_matching_rule_wins_not_any_allow() {
        let rules = json!([
            {"action": "allow"},
            {"action": "disallow", "os": {"name": "osx"}}
        ]);
        assert!(eval(rules.clone(), MetaOs::Windows));
        assert!(!eval(rules, MetaOs::Osx));

        let reversed = json!([
            {"action": "disallow", "os": {"name": "osx"}},
            {"action": "allow"}
        ]);
        assert!(
            eval(reversed, MetaOs::Osx),
            "a later allow overrides the earlier disallow"
        );
    }

    #[test]
    fn predicates_are_conjunctive() {
        let rule = json!([{"action": "allow", "os": {"name": "windows", "arch": "x86"}}]);
        assert!(
            !eval(rule, MetaOs::Windows),
            "64-bit JVM never matches arch x86"
        );
        let rule = json!([{"action": "allow", "os": {"name": "windows", "arch": "x86_64"}}]);
        assert!(eval(rule, MetaOs::Windows));
        let arm = RuleContext::with_default_features(MetaOs::Osx, "arm64", None);
        let rule = json!([{"action": "allow", "os": {"arch": "arm64"}}]);
        assert!(evaluate(Some(&rule), &arm).unwrap());
    }

    #[test]
    fn os_version_regex() {
        let rule = json!([{"action": "allow", "os": {"name": "windows", "version": "^10\\."}}]);
        assert!(eval(rule.clone(), MetaOs::Windows));
        let old = RuleContext::with_default_features(MetaOs::Windows, "x86_64", Some("6.1".into()));
        assert!(!evaluate(Some(&rule), &old).unwrap());
        let unknown = RuleContext::with_default_features(MetaOs::Windows, "x86_64", None);
        assert!(evaluate(Some(&rule), &unknown).is_err());
        let invalid = json!([{"action": "allow", "os": {"version": "("}}]);
        assert!(evaluate(Some(&invalid), &ctx(MetaOs::Windows)).is_err());
    }

    #[test]
    fn features_default_to_false() {
        let demo = json!([{"action": "allow", "features": {"is_demo_user": true}}]);
        assert!(!eval(demo, MetaOs::Linux));
        let resolution = json!([{"action": "allow", "features": {"has_custom_resolution": true}}]);
        assert!(eval(resolution, MetaOs::Linux));
        let unknown_false = json!([{"action": "allow", "features": {"some_new_feature": false}}]);
        assert!(
            eval(unknown_false, MetaOs::Linux),
            "missing feature is false and can match false"
        );
        let unknown_true = json!([{"action": "allow", "features": {"some_new_feature": true}}]);
        assert!(!eval(unknown_true, MetaOs::Linux));
    }

    #[test]
    fn every_target_os_is_distinct() {
        let rule = json!([{"action": "allow", "os": {"name": "linux"}}]);
        assert!(eval(rule.clone(), MetaOs::Linux));
        assert!(!eval(rule.clone(), MetaOs::Windows));
        assert!(!eval(rule, MetaOs::Osx));
    }

    #[test]
    fn unsupported_rules_are_errors() {
        for bad in [
            json!([{"action": "maybe"}]),
            json!([{"os": {"name": "linux"}}]),
            json!([{"action": "allow", "os": {"name": "linux", "distro": "x"}}]),
            json!([{"action": "allow", "network": true}]),
            json!([{"action": "allow", "features": {"x": "yes"}}]),
            json!({"action": "allow"}),
            json!([5]),
        ] {
            assert!(evaluate(Some(&bad), &ctx(MetaOs::Linux)).is_err(), "{bad}");
        }
    }

    #[test]
    fn current_platform_is_supported_on_ci_targets() {
        let ctx = RuleContext::current().unwrap();
        assert!(matches!(ctx.arch, "x86_64" | "arm64"));
    }
}
