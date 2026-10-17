//! The service exchange chain: Microsoft code/refresh → Xbox user token → XSTS →
//! Minecraft token → entitlements + profile.

use chrono::{DateTime, Utc};

use super::decision;
use super::http::AuthHttp;
use super::protocol::{self, MsaTokens, Profile};
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::secrets::SecretString;

/// Minecraft session obtained from a Microsoft access token.
pub struct MinecraftSession {
    pub access_token: SecretString,
    pub expires_at: DateTime<Utc>,
    /// Authenticated Xbox user ID when the XSTS response carried it.
    pub xuid: Option<String>,
}

/// Exchanges the one-use authorization code. Never retried automatically: after an
/// ambiguous failure the user must start sign-in again (doc 03).
pub async fn exchange_code(
    http: &AuthHttp,
    client_id: &str,
    code: &SecretString,
    redirect_uri: &str,
    verifier: &SecretString,
) -> LauncherResult<MsaTokens> {
    let form =
        protocol::code_exchange_form(client_id, code.expose(), redirect_uri, verifier.expose());
    let response = http.post_form(&http.endpoints.token, &form).await?;
    if response.is_success() {
        protocol::parse_msa_tokens(&response.body)
    } else {
        Err(protocol::classify_msa_error(
            response.status,
            &response.body,
        ))
    }
}

pub async fn refresh(
    http: &AuthHttp,
    client_id: &str,
    refresh_token: &SecretString,
) -> LauncherResult<MsaTokens> {
    let form = protocol::refresh_form(client_id, refresh_token.expose());
    let response = http.post_form(&http.endpoints.token, &form).await?;
    if response.is_success() {
        protocol::parse_msa_tokens(&response.body)
    } else {
        Err(protocol::classify_msa_error(
            response.status,
            &response.body,
        ))
    }
}

pub async fn minecraft_session(
    http: &AuthHttp,
    msa_access_token: &SecretString,
) -> LauncherResult<MinecraftSession> {
    let user = http
        .post_json(
            &http.endpoints.xbox_user,
            &protocol::xbox_user_request(msa_access_token.expose()),
            false,
        )
        .await?;
    if !user.is_success() {
        return Err(protocol::classify_http_status(user.status, "xbox_user"));
    }
    let user = protocol::parse_xbox_token(&user.body, "xbox_user")?;

    let xsts = http
        .post_json(
            &http.endpoints.xsts,
            &protocol::xsts_request(user.token.expose()),
            true,
        )
        .await?;
    if !xsts.is_success() {
        return Err(protocol::classify_xsts_error(xsts.status, &xsts.body));
    }
    let xsts = protocol::parse_xbox_token(&xsts.body, "xsts")?;
    // The XSTS user hash must belong to the same user token identity.
    if xsts.user_hash != user.user_hash {
        return Err(LauncherError::new(
            ErrorCode::AuthProtocolError,
            "The Xbox sign-in returned an inconsistent identity.",
        )
        .with_detail("step", "xsts"));
    }

    let login = http
        .post_json(
            &http.endpoints.minecraft_login,
            &protocol::minecraft_login_request(&xsts.user_hash, xsts.token.expose()),
            false,
        )
        .await?;
    if !login.is_success() {
        return Err(protocol::classify_minecraft_error(
            login.status,
            &login.body,
            "minecraft_login",
        ));
    }
    let token = protocol::parse_minecraft_token(&login.body)?;
    Ok(MinecraftSession {
        access_token: token.access_token,
        expires_at: token.expires_at,
        xuid: xsts.xuid,
    })
}

/// Fresh current-access check: entitlements first, then the Java profile.
pub async fn check_access(http: &AuthHttp, token: &SecretString) -> LauncherResult<Profile> {
    let url = protocol::entitlements_url(
        &http.endpoints.entitlements,
        &uuid::Uuid::new_v4().to_string(),
    );
    let entitlements = http.get_bearer(&url, token.expose()).await?;
    let entitlements = if entitlements.is_success() {
        protocol::parse_entitlements(&entitlements.body)
    } else {
        Err(protocol::classify_minecraft_error(
            entitlements.status,
            &entitlements.body,
            "entitlements",
        ))
    };
    // The profile request only happens after play access was established.
    decision::require_play(entitlements?)?;
    let profile = http
        .get_bearer(&http.endpoints.profile, token.expose())
        .await?;
    if profile.is_success() {
        protocol::parse_profile(&profile.body)
    } else {
        Err(protocol::classify_minecraft_error(
            profile.status,
            &profile.body,
            "profile",
        ))
    }
}
