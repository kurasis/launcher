fn main() {
    // The Microsoft public-client ID is non-secret build configuration (docs/AUTH-SETUP.md).
    println!("cargo:rerun-if-env-changed=KURASIS_MSA_CLIENT_ID");

    // Declaring the app's commands makes Tauri generate one permission per command,
    // so each window must be granted every command explicitly in capabilities/.
    let manifest = tauri_build::AppManifest::new().commands(&["get_app_info"]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("failed to run tauri-build");
}
