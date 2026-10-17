//! The Java play-access decision (docs/spec/03 "Access decision").
//!
//! `verified_access` requires a fresh, successful entitlement response granting play
//! access *and* a valid Java profile from the same session. Ownership is not
//! required: subscription access is play access. Every other outcome maps to a
//! distinct account state so a service outage is never shown as "no license".

use super::protocol::{Entitlements, Profile};
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::state::dto::AccountState;

/// Fails with `NO_GAME_ACCESS` unless the entitlements grant current play access.
/// Ownership alone is not play access; subscription play access needs no ownership.
pub fn require_play(entitlements: Entitlements) -> LauncherResult<()> {
    if entitlements.play {
        Ok(())
    } else {
        Err(LauncherError::new(
            ErrorCode::NoGameAccess,
            "This Microsoft account does not currently have access to Minecraft: Java \
             Edition. You can buy the game or check your subscription on minecraft.net.",
        )
        .with_detail("ownership", entitlements.ownership))
    }
}

/// Grants access only for play entitlement plus a valid profile.
pub fn decide(
    entitlements: LauncherResult<Entitlements>,
    profile: impl FnOnce() -> LauncherResult<Profile>,
) -> LauncherResult<Profile> {
    require_play(entitlements?)?;
    profile()
}

/// The account state shown for a failed access check.
pub fn state_for_error(code: ErrorCode) -> AccountState {
    match code {
        ErrorCode::NoGameAccess => AccountState::NoAccess,
        ErrorCode::ProfileSetupRequired => AccountState::ProfileSetupRequired,
        ErrorCode::AuthReauthRequired => AccountState::ReauthRequired,
        ErrorCode::AuthAppNotApproved => AccountState::AppNotApproved,
        ErrorCode::AuthAccountRestricted | ErrorCode::AuthForbidden => AccountState::Restricted,
        ErrorCode::ServiceUnavailable | ErrorCode::NetworkError | ErrorCode::RateLimited => {
            AccountState::ServiceUnavailable
        }
        ErrorCode::SecretStoreDeleteFailed => AccountState::SignOutPending,
        ErrorCode::AuthCancelled | ErrorCode::AuthTimeout | ErrorCode::AuthStateMismatch => {
            AccountState::SignedOut
        }
        _ => AccountState::ProtocolError,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> LauncherResult<Profile> {
        Ok(Profile {
            id: "0f1e2d3c4b5a69788796a5b4c3d2e1f0".into(),
            name: "Player".into(),
        })
    }

    fn ent(play: bool, ownership: bool) -> LauncherResult<Entitlements> {
        Ok(Entitlements { play, ownership })
    }

    #[test]
    fn owned_and_subscription_play_access_are_both_accepted() {
        assert!(decide(ent(true, true), profile).is_ok());
        assert!(
            decide(ent(true, false), profile).is_ok(),
            "Game Pass has no ownership item"
        );
    }

    #[test]
    fn no_play_access_is_denied_even_with_ownership_item() {
        for (play, ownership) in [(false, false), (false, true)] {
            let error = decide(ent(play, ownership), profile).unwrap_err();
            assert_eq!(error.code, ErrorCode::NoGameAccess);
        }
    }

    #[test]
    fn profile_is_not_requested_without_play_access() {
        let mut asked = false;
        let _ = decide(ent(false, false), || {
            asked = true;
            profile()
        });
        assert!(!asked);
    }

    #[test]
    fn errors_propagate_with_distinct_states() {
        let outage = Err(LauncherError::new(ErrorCode::ServiceUnavailable, "x"));
        let error = decide(outage, profile).unwrap_err();
        assert_eq!(
            state_for_error(error.code),
            AccountState::ServiceUnavailable
        );

        let missing_profile = decide(ent(true, false), || {
            Err(LauncherError::new(ErrorCode::ProfileSetupRequired, "x"))
        })
        .unwrap_err();
        assert_eq!(
            state_for_error(missing_profile.code),
            AccountState::ProfileSetupRequired
        );

        let expected = [
            (ErrorCode::NoGameAccess, AccountState::NoAccess),
            (ErrorCode::AuthReauthRequired, AccountState::ReauthRequired),
            (ErrorCode::AuthAppNotApproved, AccountState::AppNotApproved),
            (ErrorCode::AuthAccountRestricted, AccountState::Restricted),
            (ErrorCode::AuthForbidden, AccountState::Restricted),
            (ErrorCode::RateLimited, AccountState::ServiceUnavailable),
            (ErrorCode::AuthProtocolError, AccountState::ProtocolError),
        ];
        for (code, state) in expected {
            assert_eq!(state_for_error(code), state, "{code:?}");
        }
    }
}
