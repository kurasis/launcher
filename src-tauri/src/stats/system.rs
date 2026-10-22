//! What a ping says about this computer (D-071), in the few words the
//! service knows: never a name, a path or a serial number. Anything else
//! goes as `other`.

use std::sync::OnceLock;

/// The facts that do not change while the launcher runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub os: &'static str,
    /// `11` or `10` on Windows, `15` on macOS, the distribution on Linux.
    pub os_version: String,
    pub arch: &'static str,
    /// How the launcher was installed: `nsis`, `appimage`, `deb`, …
    pub bundle: &'static str,
    /// The system's language: `ru`, `kk`.
    pub language: String,
    /// The system's time zone: `Asia/Yekaterinburg`.
    pub time_zone: String,
    /// Logical processors, as a range: `5-8`.
    pub cores: &'static str,
}

/// Read once: the registry, `/etc/os-release` and the locale.
pub fn facts() -> &'static Facts {
    static FACTS: OnceLock<Facts> = OnceLock::new();
    FACTS.get_or_init(|| {
        let os = os(std::env::consts::OS);
        Facts {
            os,
            os_version: os_version(
                os,
                sysinfo::System::os_version().as_deref(),
                &sysinfo::System::distribution_id(),
            ),
            arch: arch(std::env::consts::ARCH),
            bundle: bundle(tauri::utils::platform::bundle_type()),
            language: language(sys_locale::get_locale().as_deref()),
            time_zone: time_zone(iana_time_zone::get_timezone().ok().as_deref()),
            cores: cores(std::thread::available_parallelism().map_or(0, usize::from)),
        }
    })
}

const OTHER: &str = "other";

pub fn os(name: &str) -> &'static str {
    match name {
        "windows" => "windows",
        "macos" => "macos",
        "linux" => "linux",
        _ => OTHER,
    }
}

pub fn arch(name: &str) -> &'static str {
    match name {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        _ => OTHER,
    }
}

/// A short lowercase word, as the service takes it.
fn word(text: &str) -> bool {
    (1..=20).contains(&text.len())
        && text
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric())
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
}

/// The major version on Windows (`11 (26100)` from sysinfo) and macOS
/// (`15.1.1`), the distribution's ID on Linux.
pub fn os_version(os: &str, version: Option<&str>, distribution: &str) -> String {
    let found = match os {
        "windows" | "macos" => version
            .and_then(|v| v.split([' ', '.']).next())
            .filter(|major| {
                (1..=4).contains(&major.len()) && major.bytes().all(|b| b.is_ascii_digit())
            })
            .map(str::to_owned),
        "linux" => Some(distribution.to_ascii_lowercase()).filter(|d| word(d)),
        _ => None,
    };
    found.unwrap_or_else(|| OTHER.to_owned())
}

pub fn bundle(kind: Option<tauri::utils::config::BundleType>) -> &'static str {
    use tauri::utils::config::BundleType;
    match kind {
        Some(BundleType::Nsis) => "nsis",
        Some(BundleType::Msi) => "msi",
        Some(BundleType::AppImage) => "appimage",
        Some(BundleType::Deb) => "deb",
        Some(BundleType::Rpm) => "rpm",
        Some(BundleType::App) => "app",
        Some(BundleType::Dmg) => "dmg",
        _ => "unknown",
    }
}

/// The primary subtag of the system's locale: `ru` of `ru-RU` or
/// `ru_RU.UTF-8`.
pub fn language(locale: Option<&str>) -> String {
    locale
        .and_then(|l| l.split(['-', '_', '.', '@']).next())
        .map(str::to_ascii_lowercase)
        .filter(|l| (2..=3).contains(&l.len()) && l.bytes().all(|b| b.is_ascii_lowercase()))
        .unwrap_or_else(|| OTHER.to_owned())
}

/// An IANA zone name; the service checks it against its database.
pub fn time_zone(zone: Option<&str>) -> String {
    zone.filter(|z| {
        (1..=40).contains(&z.len())
            && z.bytes().next().is_some_and(|b| b.is_ascii_alphabetic())
            && !z.contains("//")
            && z.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/_+-".contains(&b))
    })
    .map_or_else(|| OTHER.to_owned(), str::to_owned)
}

/// Memory as the service counts it, in GiB rounded to the nearest: a
/// "16 GB" computer shows a little less and still counts as 16.
pub fn ram(physical_mib: Option<u64>) -> &'static str {
    let Some(mib) = physical_mib else {
        return OTHER;
    };
    match (mib + 512) / 1024 {
        0..4 => "<4",
        4..8 => "4-8",
        8..16 => "8-16",
        16..32 => "16-32",
        32..64 => "32-64",
        _ => "64+",
    }
}

pub fn cores(count: usize) -> &'static str {
    match count {
        0 => OTHER,
        1..=2 => "1-2",
        3..=4 => "3-4",
        5..=8 => "5-8",
        9..=16 => "9-16",
        17..=32 => "17-32",
        _ => "33+",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_a_number_or_a_distribution() {
        assert_eq!(os_version("windows", Some("11 (26100)"), "windows"), "11");
        assert_eq!(os_version("windows", Some("10 (19045)"), "windows"), "10");
        assert_eq!(os_version("macos", Some("15.1.1"), "macos"), "15");
        assert_eq!(os_version("linux", Some("24.04"), "ubuntu"), "ubuntu");
        assert_eq!(
            os_version("linux", None, "opensuse-tumbleweed"),
            "opensuse-tumbleweed"
        );
        for (os, version, distribution) in [
            ("windows", None, "windows"),
            ("windows", Some("Eleven"), "windows"),
            ("macos", Some("123456.1"), "macos"),
            ("linux", None, "My Distro"),
            ("linux", None, "../etc"),
            ("linux", None, &"x".repeat(21)),
            ("freebsd", Some("14.1"), "freebsd"),
        ] {
            assert_eq!(
                os_version(os, version, distribution),
                "other",
                "{version:?} {distribution}"
            );
        }
    }

    #[test]
    fn languages_and_zones_are_plain_words() {
        assert_eq!(language(Some("ru-RU")), "ru");
        assert_eq!(language(Some("kk_KZ.UTF-8")), "kk");
        assert_eq!(language(Some("fil")), "fil");
        assert_eq!(language(Some("C")), "other");
        assert_eq!(language(Some("POSIX")), "other");
        assert_eq!(language(Some("ру-RU")), "other");
        assert_eq!(language(None), "other");
        assert_eq!(time_zone(Some("Asia/Yekaterinburg")), "Asia/Yekaterinburg");
        assert_eq!(
            time_zone(Some("America/Argentina/Buenos_Aires")),
            "America/Argentina/Buenos_Aires"
        );
        assert_eq!(time_zone(Some("Etc/GMT+3")), "Etc/GMT+3");
        for zone in [
            "/etc/localtime",
            "Europe//Moscow",
            "Europe/Moscow\n",
            "Moscow time",
            "",
        ] {
            assert_eq!(time_zone(Some(zone)), "other", "{zone:?}");
        }
        assert_eq!(time_zone(Some(&"A".repeat(41))), "other");
    }

    #[test]
    fn memory_and_cores_go_into_ranges() {
        assert_eq!(ram(Some(3 * 1024)), "<4");
        assert_eq!(ram(Some(7_900)), "8-16");
        assert_eq!(ram(Some(16_200)), "16-32");
        // 15.8 GiB, a "16 GB" computer.
        assert_eq!(ram(Some(16_179)), "16-32");
        assert_eq!(ram(Some(15_000)), "8-16");
        assert_eq!(ram(Some(128 * 1024)), "64+");
        assert_eq!(ram(None), "other");
        assert_eq!(cores(1), "1-2");
        assert_eq!(cores(8), "5-8");
        assert_eq!(cores(12), "9-16");
        assert_eq!(cores(64), "33+");
        assert_eq!(cores(0), "other");
    }

    #[test]
    fn bundles_have_the_services_names() {
        use tauri::utils::config::BundleType;
        assert_eq!(bundle(Some(BundleType::Nsis)), "nsis");
        assert_eq!(bundle(Some(BundleType::AppImage)), "appimage");
        assert_eq!(bundle(None), "unknown");
    }
}
