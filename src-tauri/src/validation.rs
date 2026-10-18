//! Shared input bounds (docs/spec/06 "Snapshot and control details").
//!
//! The frontend mirrors these limits for convenience, but only these checks are
//! authoritative.

use serde::Serialize;

use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::storage::models::Resolution;

pub const MIN_HEAP_MIB: u32 = 1024;
pub const MAX_HEAP_CAP_MIB: u32 = 32 * 1024;
pub const INITIAL_HEAP_MIB: u32 = 512;
pub const MIN_WIDTH: u32 = 640;
pub const MAX_WIDTH: u32 = 7680;
pub const MIN_HEIGHT: u32 = 480;
pub const MAX_HEIGHT: u32 = 4320;
pub const MAX_DISPLAY_NAME_CHARS: usize = 64;

const MIB: u64 = 1024 * 1024;
const LARGE_MACHINE_MIB: u64 = 8 * 1024;
const LARGE_MACHINE_DEFAULT_HEAP_MIB: u32 = 4096;
const SMALL_MACHINE_DEFAULT_HEAP_MIB: u32 = 2048;

/// Heap limits for this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryPolicy {
    pub physical_mib: u64,
    pub min_heap_mib: u32,
    pub max_heap_mib: u32,
    pub recommended_heap_mib: u32,
    /// True on machines below 8 GiB, where the default is lowered.
    pub low_memory: bool,
}

impl MemoryPolicy {
    /// Upper bound: `min(32768, physical − max(2048, 20 % of physical))` MiB.
    pub fn from_physical_bytes(bytes: u64) -> LauncherResult<Self> {
        let physical_mib = bytes / MIB;
        let reserve = (physical_mib / 5).max(2048);
        let available = physical_mib.saturating_sub(reserve);
        let max_heap_mib =
            u32::try_from(available.min(u64::from(MAX_HEAP_CAP_MIB))).unwrap_or(MAX_HEAP_CAP_MIB);
        if max_heap_mib < MIN_HEAP_MIB {
            return Err(LauncherError::templated(
                ErrorCode::InsufficientMemory,
                "This computer reports {physicalMib} MiB of memory, which leaves less than \
                 {minHeapMib} MiB for the game after the operating system reserve.",
                crate::error::params([
                    ("physicalMib", physical_mib.into()),
                    ("minHeapMib", MIN_HEAP_MIB.into()),
                ]),
            )
            .with_detail(
                "physicalMib",
                i64::try_from(physical_mib).unwrap_or(i64::MAX),
            ));
        }
        let low_memory = physical_mib < LARGE_MACHINE_MIB;
        let preferred = if low_memory {
            SMALL_MACHINE_DEFAULT_HEAP_MIB
        } else {
            LARGE_MACHINE_DEFAULT_HEAP_MIB
        };
        Ok(Self {
            physical_mib,
            min_heap_mib: MIN_HEAP_MIB,
            max_heap_mib,
            recommended_heap_mib: preferred.clamp(MIN_HEAP_MIB, max_heap_mib),
            low_memory,
        })
    }

    /// Reads physical memory from the operating system.
    pub fn detect() -> LauncherResult<Self> {
        use sysinfo::{MemoryRefreshKind, RefreshKind, System};
        let system = System::new_with_specifics(
            RefreshKind::nothing().with_memory(MemoryRefreshKind::nothing().with_ram()),
        );
        Self::from_physical_bytes(system.total_memory())
    }

    pub fn validate_heap(&self, mib: u32) -> LauncherResult<u32> {
        if (self.min_heap_mib..=self.max_heap_mib).contains(&mib) {
            Ok(mib)
        } else {
            Err(LauncherError::templated(
                ErrorCode::InvalidInput,
                "Maximum memory must be between {min} and {max} MiB on this computer.",
                crate::error::params([
                    ("min", self.min_heap_mib.into()),
                    ("max", self.max_heap_mib.into()),
                ]),
            ))
        }
    }

    /// Initial heap for a launch: never above the maximum.
    pub fn initial_heap_mib(max_heap_mib: u32) -> u32 {
        INITIAL_HEAP_MIB.min(max_heap_mib)
    }
}

/// What an installation runs, for its recommended memory (D-058).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Workload {
    /// Minecraft 1.12.2 and older without a mod loader.
    Legacy,
    /// Minecraft 1.13 and newer without a mod loader.
    Vanilla,
    /// With a mod loader; the mods in the game folder decide the amount.
    Modded { mods: u32 },
}

/// Recommended heap per workload on a machine with room for it. Measured on
/// Minecraft 1.20.1 (D-058): the game itself keeps about 0.4 GiB live and
/// peaks near 0.9 GiB in the old generation, a loader adds little, and large
/// mod packs need several GiB.
const LEGACY_HEAP_MIB: u32 = 2048;
const VANILLA_HEAP_MIB: u32 = 3072;
const FEW_MODS_HEAP_MIB: u32 = 4096;
const MANY_MODS_HEAP_MIB: u32 = 6144;
const LARGE_PACK_HEAP_MIB: u32 = 8192;
/// Up to this many mods count as a few (performance or utility mods).
const FEW_MODS: u32 = 50;
/// More than this many mods is a large pack.
const MANY_MODS: u32 = 150;

/// Release versions before 1.13 and every alpha, beta, classic and indev id.
fn legacy_version(version_id: &str) -> bool {
    if let Some(rest) = version_id.strip_prefix("1.") {
        let minor: String = rest.chars().take_while(char::is_ascii_digit).collect();
        return minor.parse::<u32>().is_ok_and(|m| m <= 12);
    }
    ["a", "b", "c", "rd-", "inf-"]
        .iter()
        .any(|p| version_id.starts_with(p) && !version_id.contains('w'))
}

impl Workload {
    pub fn of(version_id: &str, modded: bool, mods: u32) -> Self {
        if modded {
            Self::Modded { mods }
        } else if legacy_version(version_id) {
            Self::Legacy
        } else {
            Self::Vanilla
        }
    }

    fn heap_mib(self) -> u32 {
        match self {
            Self::Legacy => LEGACY_HEAP_MIB,
            Self::Vanilla => VANILLA_HEAP_MIB,
            Self::Modded { mods } if mods <= FEW_MODS => FEW_MODS_HEAP_MIB,
            Self::Modded { mods } if mods <= MANY_MODS => MANY_MODS_HEAP_MIB,
            Self::Modded { .. } => LARGE_PACK_HEAP_MIB,
        }
    }
}

/// A recommended maximum heap and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRecommendation {
    pub mib: u32,
    pub workload: Workload,
    /// What the workload asks for, when this computer cannot give that much.
    pub wanted_mib: Option<u32>,
}

impl MemoryPolicy {
    /// The heap for `workload` on this computer: what it needs, but at most
    /// half of the physical memory (the rest stays for the system, the game's
    /// native memory and the launcher) and within the allowed range.
    pub fn recommend(&self, workload: Workload) -> MemoryRecommendation {
        let wanted = workload.heap_mib();
        let half = u32::try_from(self.physical_mib / 2 / 256 * 256).unwrap_or(u32::MAX);
        let mib = wanted
            .min(half)
            .min(self.max_heap_mib)
            .max(self.min_heap_mib);
        MemoryRecommendation {
            mib,
            workload,
            wanted_mib: (mib < wanted).then_some(wanted),
        }
    }
}

/// Bounds reported to the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolutionBounds {
    pub min_width: u32,
    pub max_width: u32,
    pub min_height: u32,
    pub max_height: u32,
}

pub const RESOLUTION_BOUNDS: ResolutionBounds = ResolutionBounds {
    min_width: MIN_WIDTH,
    max_width: MAX_WIDTH,
    min_height: MIN_HEIGHT,
    max_height: MAX_HEIGHT,
};

pub fn validate_resolution(resolution: Resolution) -> LauncherResult<Resolution> {
    if (MIN_WIDTH..=MAX_WIDTH).contains(&resolution.width)
        && (MIN_HEIGHT..=MAX_HEIGHT).contains(&resolution.height)
    {
        Ok(resolution)
    } else {
        Err(LauncherError::templated(
            ErrorCode::InvalidInput,
            "Window size must be {minWidth}–{maxWidth} × {minHeight}–{maxHeight} pixels.",
            crate::error::params([
                ("minWidth", MIN_WIDTH.into()),
                ("maxWidth", MAX_WIDTH.into()),
                ("minHeight", MIN_HEIGHT.into()),
                ("maxHeight", MAX_HEIGHT.into()),
            ]),
        ))
    }
}

/// Trims surrounding whitespace and enforces 1–64 characters without control
/// characters. The name is display text only and never becomes a path.
pub fn validate_display_name(raw: &str) -> LauncherResult<String> {
    let name = raw.trim();
    let length = name.chars().count();
    if length == 0 || length > MAX_DISPLAY_NAME_CHARS {
        return Err(LauncherError::templated(
            ErrorCode::InvalidInput,
            "The installation name must be 1–{max} characters.",
            crate::error::params([("max", (MAX_DISPLAY_NAME_CHARS as u64).into())]),
        ));
    }
    if name.chars().any(char::is_control) {
        return Err(LauncherError::invalid_input(
            "The installation name cannot contain control characters.",
        ));
    }
    Ok(name.to_owned())
}

/// Longest installation note, in characters.
pub const MAX_NOTES_CHARS: usize = 4000;

/// Installation notes: up to [`MAX_NOTES_CHARS`] characters of text, with line
/// breaks and tabs but no other control characters. Line endings become `\n`
/// and surrounding blank space is dropped.
pub fn validate_notes(raw: &str) -> LauncherResult<String> {
    let notes = raw.replace("\r\n", "\n").replace('\r', "\n");
    let notes = notes.trim();
    if notes.chars().count() > MAX_NOTES_CHARS {
        return Err(LauncherError::templated(
            ErrorCode::InvalidInput,
            "Notes can be at most {max} characters long.",
            crate::error::params([("max", (MAX_NOTES_CHARS as u64).into())]),
        ));
    }
    if notes
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(LauncherError::invalid_input(
            "Notes cannot contain control characters.",
        ));
    }
    Ok(notes.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_keep_lines_and_refuse_control_characters() {
        assert_eq!(validate_notes("  a\r\nb\tc\n ").unwrap(), "a\nb\tc");
        assert_eq!(validate_notes("").unwrap(), "");
        assert!(validate_notes("a\u{0007}").is_err());
        assert!(validate_notes(&"x".repeat(MAX_NOTES_CHARS + 1)).is_err());
        assert!(validate_notes(&"ж".repeat(MAX_NOTES_CHARS)).is_ok());
    }

    const GIB: u64 = 1024 * MIB;

    #[test]
    fn memory_policy_matches_specified_formula() {
        let p16 = MemoryPolicy::from_physical_bytes(16 * GIB).unwrap();
        // reserve = max(2048, 3276) = 3276; max = 16384 - 3276
        assert_eq!(p16.max_heap_mib, 16384 - 3276);
        assert_eq!(p16.recommended_heap_mib, 4096);
        assert!(!p16.low_memory);

        let p8 = MemoryPolicy::from_physical_bytes(8 * GIB).unwrap();
        assert_eq!(p8.max_heap_mib, 8192 - 2048);
        assert_eq!(p8.recommended_heap_mib, 4096);

        let p4 = MemoryPolicy::from_physical_bytes(4 * GIB).unwrap();
        assert_eq!(p4.max_heap_mib, 2048);
        assert_eq!(p4.recommended_heap_mib, 2048);
        assert!(p4.low_memory);

        let p256 = MemoryPolicy::from_physical_bytes(256 * GIB).unwrap();
        assert_eq!(p256.max_heap_mib, MAX_HEAP_CAP_MIB);
    }

    #[test]
    fn recommendations_follow_the_workload_and_the_machine() {
        let p16 = MemoryPolicy::from_physical_bytes(16 * GIB).unwrap();
        let mib = |w| p16.recommend(w).mib;
        assert_eq!(mib(Workload::of("1.7.10", false, 0)), 2048);
        assert_eq!(mib(Workload::of("b1.7.3", false, 0)), 2048);
        assert_eq!(mib(Workload::of("1.12.2", false, 0)), 2048);
        assert_eq!(mib(Workload::of("1.13", false, 0)), 3072);
        assert_eq!(mib(Workload::of("1.21.1", false, 0)), 3072);
        assert_eq!(mib(Workload::of("26.3", false, 0)), 3072);
        assert_eq!(mib(Workload::of("24w14a", false, 0)), 3072);
        assert_eq!(mib(Workload::of("1.20.1", true, 0)), 4096);
        assert_eq!(mib(Workload::of("1.20.1", true, 50)), 4096);
        assert_eq!(mib(Workload::of("1.12.2", true, 51)), 6144);
        assert_eq!(mib(Workload::of("1.20.1", true, 330)), 8192);
        assert_eq!(
            p16.recommend(Workload::of("1.20.1", true, 330)).wanted_mib,
            None
        );

        // 8 GiB: at most half for the game.
        let p8 = MemoryPolicy::from_physical_bytes(8 * GIB).unwrap();
        let large = p8.recommend(Workload::Modded { mods: 330 });
        assert_eq!((large.mib, large.wanted_mib), (4096, Some(8192)));
        assert_eq!(p8.recommend(Workload::Vanilla).mib, 3072);
        // 4 GiB: the allowed maximum (2048) wins.
        let p4 = MemoryPolicy::from_physical_bytes(4 * GIB).unwrap();
        assert_eq!(p4.recommend(Workload::Vanilla).mib, 2048);
        assert_eq!(p4.recommend(Workload::Legacy).wanted_mib, None);
    }

    #[test]
    fn small_machines_get_a_clear_error() {
        let error = MemoryPolicy::from_physical_bytes(3 * GIB - 1).unwrap_err();
        assert_eq!(error.code, ErrorCode::InsufficientMemory);
        assert!(MemoryPolicy::from_physical_bytes(3 * GIB).is_ok());
    }

    #[test]
    fn heap_bounds_are_enforced() {
        let policy = MemoryPolicy::from_physical_bytes(16 * GIB).unwrap();
        assert!(policy.validate_heap(1023).is_err());
        assert!(policy.validate_heap(1024).is_ok());
        assert!(policy.validate_heap(policy.max_heap_mib).is_ok());
        assert!(policy.validate_heap(policy.max_heap_mib + 1).is_err());
        assert_eq!(MemoryPolicy::initial_heap_mib(4096), 512);
        assert_eq!(MemoryPolicy::initial_heap_mib(256), 256);
    }

    #[test]
    fn detected_memory_is_plausible() {
        // The CI and development hosts have well over 3 GiB.
        let policy = MemoryPolicy::detect().unwrap();
        assert!(policy.physical_mib > 1024);
    }

    #[test]
    fn resolution_bounds_are_inclusive() {
        let ok = |w, h| {
            validate_resolution(Resolution {
                width: w,
                height: h,
            })
            .is_ok()
        };
        assert!(ok(640, 480));
        assert!(ok(7680, 4320));
        assert!(!ok(639, 480));
        assert!(!ok(640, 479));
        assert!(!ok(7681, 720));
        assert!(!ok(1280, 4321));
        assert!(!ok(0, 0));
    }

    #[test]
    fn display_names_are_trimmed_and_bounded() {
        assert_eq!(validate_display_name("  My world  ").unwrap(), "My world");
        assert_eq!(
            validate_display_name(&"é".repeat(64))
                .unwrap()
                .chars()
                .count(),
            64
        );
        assert!(validate_display_name(&"é".repeat(65)).is_err());
        assert!(validate_display_name("   ").is_err());
        assert!(validate_display_name("a\nb").is_err());
        assert!(
            validate_display_name("../../etc").is_ok(),
            "names are not paths"
        );
    }
}
