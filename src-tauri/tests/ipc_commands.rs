//! Exercises command registration and serialization through Tauri's mock runtime.

use launcher_lib::commands;
use serde_json::Value;
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{INVOKE_KEY, get_ipc_response, mock_builder, mock_context, noop_assets};
use tauri::webview::InvokeRequest;

// Packaged assets are served from this origin; any other origin counts as remote and
// receives no capabilities.
#[cfg(windows)]
const LOCAL_ORIGIN: &str = "http://tauri.localhost";
#[cfg(not(windows))]
const LOCAL_ORIGIN: &str = "tauri://localhost";

fn invoke_get_app_info(origin: &str) -> Result<tauri::ipc::InvokeResponseBody, Value> {
    let app = mock_builder()
        .invoke_handler(tauri::generate_handler![commands::get_app_info])
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("mock window");

    get_ipc_response(
        &webview,
        InvokeRequest {
            cmd: "get_app_info".into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: origin.parse().expect("url"),
            body: InvokeBody::default(),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
}

#[test]
fn remote_origin_cannot_invoke_commands() {
    let error = invoke_get_app_info("https://example.com").expect_err("remote origin denied");
    assert!(error.to_string().contains("not allowed"), "{error}");
}

#[test]
fn get_app_info_returns_the_frontend_contract() {
    let response = invoke_get_app_info(LOCAL_ORIGIN).expect("command succeeds");
    let info: Value = response.deserialize().expect("JSON response");

    let object = info.as_object().expect("object");
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "appVersion",
            "arch",
            "buildProfile",
            "microsoftClientId",
            "os",
            "productName",
            "tauriVersion",
            "testProfile"
        ]
    );
    assert_eq!(info["os"], std::env::consts::OS);
    assert_eq!(info["arch"], std::env::consts::ARCH);
    assert_eq!(info["tauriVersion"], tauri::VERSION);
    assert_eq!(info["buildProfile"], "debug");
    // Set only when the debug build runs with LAUNCHER_NOTOKEN_PROFILE.
    assert!(info["testProfile"].is_null() || info["testProfile"].is_string());
    assert!(
        ["not_configured", "invalid", "configured_unverified"]
            .contains(&info["microsoftClientId"].as_str().expect("status")),
    );
}
