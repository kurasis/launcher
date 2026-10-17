//! Credential-bearing HTTP client for the auth services (docs/spec/07 "Trust
//! boundaries", doc 03 "Request construction contract").
//!
//! This client is separate from any artifact download client. It accepts HTTPS only,
//! follows no redirects (an unexpected redirect is a protocol error, so a token is
//! never forwarded to another host), bounds response sizes and never logs bodies.

use std::time::Duration;

use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::Value;

use super::contract;
use crate::error::{ErrorCode, LauncherError, LauncherResult};

const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Service URLs. Production values are fixed; tests substitute a local server.
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub authorize: String,
    pub token: String,
    pub xbox_user: String,
    pub xsts: String,
    pub minecraft_login: String,
    pub entitlements: String,
    pub profile: String,
}

impl Endpoints {
    pub fn production() -> Self {
        Self {
            authorize: contract::AUTHORIZE_URL.into(),
            token: contract::TOKEN_URL.into(),
            xbox_user: contract::XBOX_USER_URL.into(),
            xsts: contract::XSTS_URL.into(),
            minecraft_login: contract::MINECRAFT_LOGIN_URL.into(),
            entitlements: contract::ENTITLEMENTS_URL.into(),
            profile: contract::PROFILE_URL.into(),
        }
    }

    /// Test-only endpoints on a local plain-HTTP server.
    #[cfg(test)]
    pub fn local(base: &str) -> Self {
        Self {
            authorize: format!("{base}/authorize"),
            token: format!("{base}/token"),
            xbox_user: format!("{base}/xbox/user"),
            xsts: format!("{base}/xsts"),
            minecraft_login: format!("{base}/launcher/login"),
            entitlements: format!("{base}/entitlements/license"),
            profile: format!("{base}/minecraft/profile"),
        }
    }
}

/// A response status and bounded body.
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Response {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

pub struct AuthHttp {
    client: reqwest::Client,
    pub endpoints: Endpoints,
}

fn builder(app_version: &str) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .user_agent(format!("KurasisLauncher/{app_version}"))
}

impl AuthHttp {
    pub fn production(app_version: &str) -> LauncherResult<Self> {
        let client = builder(app_version)
            .https_only(true)
            .build()
            .map_err(|_| LauncherError::internal("The HTTPS client could not be created."))?;
        Ok(Self {
            client,
            endpoints: Endpoints::production(),
        })
    }

    /// Test-only client that may talk plain HTTP to a local fixture server.
    #[cfg(test)]
    pub fn local(base: &str) -> Self {
        Self {
            client: builder("test").build().expect("client"),
            endpoints: Endpoints::local(base),
        }
    }

    pub async fn post_form(&self, url: &str, form: &[(&str, String)]) -> LauncherResult<Response> {
        let request = self
            .client
            .post(url)
            .header(ACCEPT, "application/json")
            .form(form);
        send(request).await
    }

    pub async fn post_json(
        &self,
        url: &str,
        body: &Value,
        xbl_contract: bool,
    ) -> LauncherResult<Response> {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if xbl_contract {
            headers.insert("x-xbl-contract-version", HeaderValue::from_static("1"));
        }
        send(self.client.post(url).headers(headers).json(body)).await
    }

    pub async fn get_bearer(&self, url: &str, token: &str) -> LauncherResult<Response> {
        let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| LauncherError::new(ErrorCode::AuthProtocolError, "Invalid token."))?;
        value.set_sensitive(true);
        send(
            self.client
                .get(url)
                .header(ACCEPT, "application/json")
                .header(AUTHORIZATION, value),
        )
        .await
    }
}

async fn send(request: reqwest::RequestBuilder) -> LauncherResult<Response> {
    let mut response = request.send().await.map_err(map_transport_error)?;
    let status = response.status().as_u16();
    if (300..400).contains(&status) {
        return Err(LauncherError::new(
            ErrorCode::AuthProtocolError,
            "The sign-in service answered with an unexpected redirect.",
        )
        .with_detail("httpStatus", i64::from(status)));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(map_transport_error)? {
        if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(LauncherError::new(
                ErrorCode::AuthProtocolError,
                "The sign-in service response was too large.",
            ));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Response { status, body })
}

fn map_transport_error(error: reqwest::Error) -> LauncherError {
    // The error's Display can include the request URL; only its category is kept.
    let kind = if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else if error.is_request() {
        "request"
    } else {
        "transport"
    };
    LauncherError::new(
        ErrorCode::NetworkError,
        "The sign-in service could not be reached. Check the internet connection and try again.",
    )
    .with_detail("transport", kind)
}
