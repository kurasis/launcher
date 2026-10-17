//! Request bodies and strict response parsing for the Microsoft, Xbox and Minecraft
//! services (docs/spec/03 "Request construction contract").
//!
//! Requests are built with structured JSON/form serialization, never by string
//! substitution. Responses are validated field by field; anything unexpected is a
//! protocol error rather than a guess. Raw response bodies never leave this module:
//! only classified, non-secret diagnostics do.

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::{Value, json};

use super::contract;
use crate::error::{ErrorCode, LauncherError};
use crate::secrets::SecretString;

/// Microsoft OAuth token response.
pub struct MsaTokens {
    pub access_token: SecretString,
    /// Absent when the provider did not rotate the refresh token.
    pub refresh_token: Option<SecretString>,
    pub expires_at: DateTime<Utc>,
}

/// An Xbox user token or XSTS token.
pub struct XboxToken {
    pub token: SecretString,
    pub user_hash: String,
    /// The Xbox user ID when the service includes it (`xid`), otherwise `None`.
    pub xuid: Option<String>,
    pub not_after: DateTime<Utc>,
}

/// Minecraft services access token.
pub struct MinecraftToken {
    pub access_token: SecretString,
    pub expires_at: DateTime<Utc>,
}

/// Parsed current-access entitlements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entitlements {
    pub play: bool,
    pub ownership: bool,
}

/// Authenticated Java profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// 32 lowercase hex characters, without dashes, as returned by the service.
    pub id: String,
    pub name: String,
}

fn protocol_error(step: &str) -> LauncherError {
    LauncherError::new(
        ErrorCode::AuthProtocolError,
        "The sign-in service returned a response the launcher does not understand.",
    )
    .with_detail("step", step)
}

fn expiry_from_seconds(seconds: Option<i64>, step: &str) -> Result<DateTime<Utc>, LauncherError> {
    match seconds {
        Some(s) if (1..=60 * 60 * 24 * 30).contains(&s) => Ok(Utc::now() + Duration::seconds(s)),
        _ => Err(protocol_error(step)),
    }
}

fn required_str<'a>(value: &'a Value, key: &str, step: &str) -> Result<&'a str, LauncherError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| protocol_error(step))
}

// ----- Microsoft OAuth -----

/// Form for exchanging an authorization code.
pub fn code_exchange_form(
    client_id: &str,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> Vec<(&'static str, String)> {
    vec![
        ("client_id", client_id.to_owned()),
        ("grant_type", "authorization_code".to_owned()),
        ("code", code.to_owned()),
        ("redirect_uri", redirect_uri.to_owned()),
        ("code_verifier", verifier.to_owned()),
        ("scope", contract::SCOPES.to_owned()),
    ]
}

/// Form for the refresh-token grant.
pub fn refresh_form(client_id: &str, refresh_token: &str) -> Vec<(&'static str, String)> {
    vec![
        ("client_id", client_id.to_owned()),
        ("grant_type", "refresh_token".to_owned()),
        ("refresh_token", refresh_token.to_owned()),
        ("scope", contract::SCOPES.to_owned()),
    ]
}

/// Parses a successful (2xx) token response.
pub fn parse_msa_tokens(body: &[u8]) -> Result<MsaTokens, LauncherError> {
    let value: Value = serde_json::from_slice(body).map_err(|_| protocol_error("msa_token"))?;
    let access = required_str(&value, "access_token", "msa_token")?;
    let token_type = required_str(&value, "token_type", "msa_token")?;
    if !token_type.eq_ignore_ascii_case("bearer") {
        return Err(protocol_error("msa_token"));
    }
    let expires_at =
        expiry_from_seconds(value.get("expires_in").and_then(Value::as_i64), "msa_token")?;
    let refresh_token = match value.get("refresh_token") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if !s.is_empty() => Some(SecretString::new(s.clone())),
        Some(_) => return Err(protocol_error("msa_token")),
    };
    Ok(MsaTokens {
        access_token: SecretString::new(access),
        refresh_token,
        expires_at,
    })
}

/// OAuth error body (`error`, `error_description`, `error_codes`).
#[derive(Debug, Deserialize)]
struct OAuthError {
    error: String,
    #[serde(default)]
    error_codes: Vec<i64>,
}

/// Classifies a non-2xx token endpoint response. The description text is never
/// copied: it can contain trace and correlation IDs.
pub fn classify_msa_error(status: u16, body: &[u8]) -> LauncherError {
    let parsed: Option<OAuthError> = serde_json::from_slice(body).ok();
    let Some(error) = parsed else {
        return classify_http_status(status, "msa_token");
    };
    let first_code = error.error_codes.first().copied();
    let base = match error.error.as_str() {
        "invalid_grant" | "interaction_required" | "consent_required" => LauncherError::new(
            ErrorCode::AuthReauthRequired,
            "Your sign-in has expired or was revoked. Sign in again.",
        ),
        "invalid_client" | "unauthorized_client" | "invalid_scope" | "unsupported_grant_type" => {
            LauncherError::new(
                ErrorCode::AuthForbidden,
                "Microsoft rejected this launcher's sign-in configuration. The project owner \
                 must check the application registration.",
            )
        }
        "temporarily_unavailable" | "server_error" => LauncherError::new(
            ErrorCode::ServiceUnavailable,
            "Microsoft sign-in is temporarily unavailable. Try again later.",
        )
        .with_retryable(true),
        _ => protocol_error("msa_token"),
    };
    let error = base.with_detail("oauthError", error.error);
    match first_code {
        Some(code) => error.with_detail("providerCode", code),
        None => error,
    }
}

// ----- Xbox -----

pub fn xbox_user_request(msa_access_token: &str) -> Value {
    json!({
        "Properties": {
            "AuthMethod": "RPS",
            "SiteName": contract::XBOX_SITE_NAME,
            "RpsTicket": format!("d={msa_access_token}"),
        },
        "RelyingParty": contract::XBOX_USER_RELYING_PARTY,
        "TokenType": "JWT",
    })
}

pub fn xsts_request(user_token: &str) -> Value {
    json!({
        "Properties": {
            "SandboxId": contract::SANDBOX,
            "UserTokens": [user_token],
        },
        "RelyingParty": contract::MINECRAFT_RELYING_PARTY,
        "TokenType": "JWT",
    })
}

/// Parses an Xbox user or XSTS token response.
pub fn parse_xbox_token(body: &[u8], step: &str) -> Result<XboxToken, LauncherError> {
    let value: Value = serde_json::from_slice(body).map_err(|_| protocol_error(step))?;
    let token = required_str(&value, "Token", step)?;
    let not_after = required_str(&value, "NotAfter", step)?;
    let not_after = DateTime::parse_from_rfc3339(not_after)
        .map_err(|_| protocol_error(step))?
        .with_timezone(&Utc);
    let claims = value
        .pointer("/DisplayClaims/xui")
        .and_then(Value::as_array)
        .ok_or_else(|| protocol_error(step))?;
    let claim = claims
        .iter()
        .find(|c| c.get("uhs").and_then(Value::as_str).is_some())
        .ok_or_else(|| protocol_error(step))?;
    let user_hash = required_str(claim, "uhs", step)?.to_owned();
    let xuid = claim
        .get("xid")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .map(str::to_owned);
    Ok(XboxToken {
        token: SecretString::new(token),
        user_hash,
        xuid,
        not_after,
    })
}

/// Known XSTS `XErr` values (doc 03 "Error handling").
pub fn classify_xsts_error(status: u16, body: &[u8]) -> LauncherError {
    let xerr = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v.get("XErr").and_then(Value::as_i64));
    let Some(code) = xerr else {
        return classify_http_status(status, "xsts");
    };
    let (error_code, message) = match code {
        2_148_916_233 => (
            ErrorCode::ProfileSetupRequired,
            "This Microsoft account has no Xbox profile yet. Sign in once at xbox.com to create \
             one, then try again.",
        ),
        2_148_916_235 => (
            ErrorCode::AuthAccountRestricted,
            "Xbox Live is not available in this account's country or region.",
        ),
        2_148_916_236 | 2_148_916_237 => (
            ErrorCode::AuthAccountRestricted,
            "This account needs adult verification or has reached a time limit set for it.",
        ),
        2_148_916_238 | 2_148_916_229 => (
            ErrorCode::AuthAccountRestricted,
            "This child account must be added to a Microsoft family, or a parent must allow \
             online play.",
        ),
        2_148_916_234 => (
            ErrorCode::AuthAccountRestricted,
            "This account has not accepted the Xbox terms. Sign in at xbox.com to accept them.",
        ),
        2_148_916_227 => (
            ErrorCode::AuthAccountRestricted,
            "Xbox has restricted this account.",
        ),
        _ => (
            ErrorCode::AuthForbidden,
            "Xbox refused the sign-in for this account.",
        ),
    };
    LauncherError::new(error_code, message).with_detail("xerr", code)
}

// ----- Minecraft services -----

pub fn minecraft_login_request(user_hash: &str, xsts_token: &str) -> Value {
    json!({
        "xtoken": format!("XBL3.0 x={user_hash};{xsts_token}"),
        "platform": contract::MINECRAFT_PLATFORM,
    })
}

pub fn parse_minecraft_token(body: &[u8]) -> Result<MinecraftToken, LauncherError> {
    let value: Value =
        serde_json::from_slice(body).map_err(|_| protocol_error("minecraft_login"))?;
    let token = required_str(&value, "access_token", "minecraft_login")?;
    let expires_at = expiry_from_seconds(
        value.get("expires_in").and_then(Value::as_i64),
        "minecraft_login",
    )?;
    Ok(MinecraftToken {
        access_token: SecretString::new(token),
        expires_at,
    })
}

/// Classifies a non-2xx Minecraft services response. `AUTH_APP_NOT_APPROVED` is
/// reported only for the recognized registration diagnostic; any other 403 stays
/// `AUTH_FORBIDDEN` (doc 03 "Mandatory early gate").
pub fn classify_minecraft_error(status: u16, body: &[u8], step: &str) -> LauncherError {
    if status == 403 {
        let message = serde_json::from_slice::<Value>(body).ok().and_then(|v| {
            v.get("errorMessage")
                .or_else(|| v.get("error"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
        if message
            .as_deref()
            .is_some_and(|m| m.contains(contract::APP_NOT_APPROVED_MARKER))
        {
            return LauncherError::new(
                ErrorCode::AuthAppNotApproved,
                "Minecraft services have not approved this launcher's Microsoft application. \
                 The project owner must request access (see docs/AUTH-SETUP.md).",
            )
            .with_detail("step", step);
        }
        return LauncherError::new(
            ErrorCode::AuthForbidden,
            "Minecraft services refused this request. This does not mean the account lacks \
             the game.",
        )
        .with_detail("step", step)
        .with_detail("httpStatus", i64::from(status));
    }
    classify_http_status(status, step)
}

/// Builds the entitlement check URL with a fresh request ID.
pub fn entitlements_url(base: &str, request_id: &str) -> String {
    format!("{base}?requestId={request_id}")
}

/// Parses the entitlement response strictly: a missing `items` array is a protocol
/// error, not an empty license list.
pub fn parse_entitlements(body: &[u8]) -> Result<Entitlements, LauncherError> {
    let value: Value = serde_json::from_slice(body).map_err(|_| protocol_error("entitlements"))?;
    let items = value
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| protocol_error("entitlements"))?;
    let mut result = Entitlements {
        play: false,
        ownership: false,
    };
    for item in items {
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| protocol_error("entitlements"))?;
        match name {
            contract::PLAY_ENTITLEMENT => result.play = true,
            contract::OWNERSHIP_ENTITLEMENT => result.ownership = true,
            _ => {}
        }
    }
    Ok(result)
}

/// Parses the Java profile. Only `id` and `name` are used.
pub fn parse_profile(body: &[u8]) -> Result<Profile, LauncherError> {
    let value: Value = serde_json::from_slice(body).map_err(|_| protocol_error("profile"))?;
    let id = required_str(&value, "id", "profile")?;
    let name = required_str(&value, "name", "profile")?;
    let id_valid = id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit());
    let name_valid = (1..=16).contains(&name.len())
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    if !id_valid || !name_valid {
        return Err(protocol_error("profile"));
    }
    Ok(Profile {
        id: id.to_ascii_lowercase(),
        name: name.to_owned(),
    })
}

/// Maps transport-level HTTP statuses that carry no recognized body.
pub fn classify_http_status(status: u16, step: &str) -> LauncherError {
    let error = match status {
        401 => LauncherError::new(
            ErrorCode::AuthReauthRequired,
            "Your sign-in has expired. Sign in again.",
        ),
        403 => LauncherError::new(
            ErrorCode::AuthForbidden,
            "The sign-in service refused this request.",
        ),
        404 if step == "profile" => LauncherError::new(
            ErrorCode::ProfileSetupRequired,
            "This account has no Minecraft: Java Edition profile yet. Create one at \
             minecraft.net, then try again.",
        ),
        429 => LauncherError::new(
            ErrorCode::RateLimited,
            "The sign-in service is rate limiting requests. Wait a moment and try again.",
        ),
        500..=599 => LauncherError::new(
            ErrorCode::ServiceUnavailable,
            "The sign-in service is temporarily unavailable. Try again later.",
        ),
        _ => protocol_error(step),
    };
    error
        .with_detail("step", step)
        .with_detail("httpStatus", i64::from(status))
}

#[cfg(test)]
mod tests;
