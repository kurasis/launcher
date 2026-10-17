//! The versioned authentication contract (docs/AUTH-CONTRACT.md).
//!
//! These values come from the specification's source review (Prism Launcher at
//! commit 60bcc2694d7433e2d2a657fbf62b0d022f9116e0) and Microsoft documentation.
//! They have not yet been accepted with this project's own client ID; gate G0 must
//! confirm them. Changing any value is a contract change that needs a decision
//! record and positive/negative account tests. There is no fallback chain.

/// Identifier of this contract revision, recorded with account diagnostics.
pub const CONTRACT_VERSION: &str = "2026-10-candidate-1";

pub const AUTHORIZE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";
pub const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
pub const XBOX_USER_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
pub const XSTS_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
pub const MINECRAFT_LOGIN_URL: &str = "https://api.minecraftservices.com/launcher/login";
pub const ENTITLEMENTS_URL: &str = "https://api.minecraftservices.com/entitlements/license";
pub const PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";

/// Scopes requested at authorization. Candidate from the reviewed source; the
/// alternative `XboxLive.signin offline_access` is recorded in AUTH-CONTRACT.md.
pub const SCOPES: &str = "XboxLive.SignIn XboxLive.offline_access";

/// Loopback redirect host. The registered native redirect is `http://localhost`;
/// Microsoft ignores the port for loopback redirects of public clients.
pub const REDIRECT_HOST: &str = "localhost";
/// Path the browser is redirected to. Must match the registration exactly.
pub const REDIRECT_PATH: &str = "/";

pub const XBOX_SITE_NAME: &str = "user.auth.xboxlive.com";
pub const XBOX_USER_RELYING_PARTY: &str = "http://auth.xboxlive.com";
pub const MINECRAFT_RELYING_PARTY: &str = "rp://api.minecraftservices.com/";
pub const SANDBOX: &str = "RETAIL";
pub const MINECRAFT_PLATFORM: &str = "PC_LAUNCHER";

/// Entitlement item meaning current Java Edition play access (owned or subscription).
pub const PLAY_ENTITLEMENT: &str = "game_minecraft";
/// Entitlement item meaning permanent ownership. Not required for play.
pub const OWNERSHIP_ENTITLEMENT: &str = "product_minecraft";

/// Minecraft services response text that identifies a client ID without Minecraft
/// API approval. Only this recognized diagnostic maps to `AUTH_APP_NOT_APPROVED`.
pub const APP_NOT_APPROVED_MARKER: &str = "Invalid app registration";

/// Seconds subtracted from token expiry to absorb clock skew.
pub const EXPIRY_SKEW_SECONDS: i64 = 300;

/// How long a browser sign-in may take before the attempt expires.
pub const LOGIN_TIMEOUT_SECONDS: u64 = 300;
