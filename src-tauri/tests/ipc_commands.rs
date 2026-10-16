//! Exercises command registration and serialization through Tauri's mock runtime.

use kurasis_launcher_lib::commands;
use serde_json::Value;
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{INVOKE_KEY, get_ipc_response, mock_builder, mock_context, noop_assets};
use tauri::webview::InvokeRequest;

#[test]
fn get_app_info_returns_the_frontend_contract() {
    let app = mock_builder()
        .invoke_handler(tauri::generate_handler![commands::get_app_info])
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("mock window");

    let response = get_ipc_response(
        &webview,
        InvokeRequest {
            cmd: "get_app_info".into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: "tauri://localhost".parse().expect("url"),
            body: InvokeBody::default(),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .expect("command succeeds");
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
            "tauriVersion"
        ]
    );
    assert_eq!(info["os"], std::env::consts::OS);
    assert_eq!(info["arch"], std::env::consts::ARCH);
    assert_eq!(info["tauriVersion"], tauri::VERSION);
    assert_eq!(info["buildProfile"], "debug");
    assert!(
        ["not_configured", "invalid", "configured_unverified"]
            .contains(&info["microsoftClientId"].as_str().expect("status")),
    );
}
