use std::env;
use std::path::PathBuf;

fn main() {
    // The Microsoft public-client ID is non-secret build configuration (docs/AUTH-SETUP.md).
    println!("cargo:rerun-if-env-changed=KURASIS_MSA_CLIENT_ID");

    // Declaring the app's commands makes Tauri generate one permission per command,
    // so each window must be granted every command explicitly in capabilities/.
    let manifest = tauri_build::AppManifest::new().commands(&[
        "get_app_info",
        "get_app_snapshot",
        "create_instance",
        "update_instance_settings",
        "set_app_settings",
        "show_main_window",
        "request_quit",
        "begin_login",
        "cancel_login",
        "sign_out",
        "verify_account",
        "open_external",
        "list_supported_versions",
        "preview_install_plan",
        // Registered only in debug builds; release builds have no handler for them.
        "start_lifecycle_test",
        "stop_lifecycle_test",
    ]);
    let mut attributes = tauri_build::Attributes::new().app_manifest(manifest);

    if is_windows_msvc() {
        // tauri-build embeds its Windows manifest only into the binary target. Test
        // executables then load Common Controls v5 and abort with
        // STATUS_ENTRYPOINT_NOT_FOUND, so embed the same manifest for every target.
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        embed_windows_manifest();
    }

    tauri_build::try_build(attributes).expect("failed to run tauri-build");
}

fn is_windows_msvc() -> bool {
    env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
}

fn embed_windows_manifest() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"))
        .join("windows-app-manifest.xml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
}
