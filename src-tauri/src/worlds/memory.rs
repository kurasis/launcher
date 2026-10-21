//! Memory budget shown before a server starts (PLAN W3).
//!
//! The host usually plays in the same world, so the game's heap and the
//! server's heap share the computer with the launcher and the operating
//! system. The reserve is the one the game's own heap check uses
//! (`validation::MemoryPolicy`): at least 2 GiB or a fifth of the memory.

use serde::Serialize;

use crate::error::{ErrorCode, LauncherError, LauncherResult};

const MIB: u64 = 1024 * 1024;
pub const MIN_SERVER_HEAP_MIB: u32 = 1024;
pub const DEFAULT_SERVER_HEAP_MIB: u32 = 2048;
/// A server with mods loads more classes and keeps more per chunk (W12).
pub const DEFAULT_MODDED_SERVER_HEAP_MIB: u32 = 3072;
pub const MAX_SERVER_HEAP_MIB: u32 = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Budget {
    pub physical_mib: u64,
    /// The launcher and the operating system.
    pub reserve_mib: u64,
    /// Zero when the host does not play on this computer.
    pub game_heap_mib: u32,
    pub server_heap_mib: u32,
    /// Memory left after all of the above; negative when it does not fit.
    pub headroom_mib: i64,
}

impl Budget {
    pub fn fits(&self) -> bool {
        self.headroom_mib >= 0
    }
}

/// The budget for a server heap beside a game heap on this much memory.
pub fn budget(physical_bytes: u64, game_heap_mib: u32, server_heap_mib: u32) -> Budget {
    let physical_mib = physical_bytes / MIB;
    let reserve_mib = (physical_mib / 5).max(2048);
    let used = i128::from(reserve_mib) + i128::from(game_heap_mib) + i128::from(server_heap_mib);
    let headroom = i128::from(physical_mib) - used;
    Budget {
        physical_mib,
        reserve_mib,
        game_heap_mib,
        server_heap_mib,
        headroom_mib: i64::try_from(headroom).unwrap_or(i64::MIN),
    }
}

/// Validates a server heap the player chose.
pub fn validate_server_heap(mib: u32) -> LauncherResult<()> {
    if (MIN_SERVER_HEAP_MIB..=MAX_SERVER_HEAP_MIB).contains(&mib) {
        Ok(())
    } else {
        Err(LauncherError::invalid_input(
            "A world server uses 1024 to 16384 MiB of memory.",
        ))
    }
}

/// Refuses a start whose budget cannot fit.
pub fn check(budget: &Budget) -> LauncherResult<()> {
    if budget.fits() {
        return Ok(());
    }
    Err(LauncherError::templated(
        ErrorCode::InsufficientMemory,
        "This computer has {physicalMib} MiB of memory. The game ({gameMib} MiB), the world \
         server ({serverMib} MiB) and the system ({reserveMib} MiB) need more. Give the server \
         or the game less memory.",
        crate::error::params([
            ("physicalMib", budget.physical_mib.into()),
            ("gameMib", u64::from(budget.game_heap_mib).into()),
            ("serverMib", u64::from(budget.server_heap_mib).into()),
            ("reserveMib", budget.reserve_mib.into()),
        ]),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * MIB;

    #[test]
    fn a_game_and_a_server_fit_on_sixteen_gigabytes() {
        let b = budget(16 * GIB, 4096, DEFAULT_SERVER_HEAP_MIB);
        assert_eq!(b.reserve_mib, 3276);
        assert!(b.fits());
        assert!(check(&b).is_ok());
    }

    #[test]
    fn an_impossible_budget_is_refused() {
        let b = budget(8 * GIB, 6144, DEFAULT_SERVER_HEAP_MIB);
        assert!(!b.fits());
        let error = check(&b).unwrap_err();
        assert_eq!(error.code, ErrorCode::InsufficientMemory);
        assert!(budget(4 * GIB, 0, 2048).fits(), "server alone on 4 GiB");
        assert!(!budget(0, 0, 1024).fits());
    }

    #[test]
    fn server_heap_is_bounded() {
        assert!(validate_server_heap(DEFAULT_SERVER_HEAP_MIB).is_ok());
        for bad in [0, 1023, 16 * 1024 + 1, u32::MAX] {
            assert!(validate_server_heap(bad).is_err());
        }
    }
}
