use std::env;
use std::path::PathBuf;

fn main() {
    // The Microsoft public-client ID is non-secret build configuration (docs/AUTH-SETUP.md).
    println!("cargo:rerun-if-env-changed=LAUNCHER_MSA_CLIENT_ID");
    // The update public key (D-063); without it the launcher does not look for updates.
    println!("cargo:rerun-if-env-changed=LAUNCHER_UPDATE_PUBKEY");
    println!("cargo:rerun-if-env-changed=LAUNCHER_UPDATE_URL");
    println!("cargo:rerun-if-env-changed=LAUNCHER_ACCOUNT_URL");
    println!("cargo:rerun-if-env-changed=LAUNCHER_FRIENDS_URL");
    println!("cargo:rerun-if-env-changed=LAUNCHER_MODS_URL");
    println!("cargo:rerun-if-env-changed=LAUNCHER_SKINS_URL");

    // Declaring the app's commands makes Tauri generate one permission per command,
    // so each window must be granted every command explicitly in capabilities/.
    let manifest = tauri_build::AppManifest::new().commands(&[
        "get_app_info",
        "get_app_snapshot",
        "create_instance",
        "update_instance_settings",
        "set_instance_notes",
        "rename_instance",
        "set_app_settings",
        "show_main_window",
        "open_console",
        "get_data_location",
        "move_data_location",
        "pick_data_folder",
        "check_java",
        "set_instance_java",
        "set_instance_loader",
        "pick_java_file",
        "list_backups",
        "create_backup",
        "restore_backup",
        "delete_backup",
        "inspect_instance",
        "delete_instance",
        "reset_preview",
        "reset_launcher",
        "recommend_memory",
        "list_screenshots",
        "delete_screenshot",
        "open_screenshot",
        "read_console",
        "request_quit",
        "begin_login",
        "cancel_login",
        "sign_out",
        "verify_account",
        "add_notoken_profile",
        "remove_notoken_profile",
        "set_default_profile",
        "set_instance_profile",
        "open_external",
        "list_supported_versions",
        "preview_install_plan",
        "list_loader_game_versions",
        "list_loader_versions",
        "install_instance",
        "repair_instance",
        "cancel_operation",
        "answer_download",
        "check_network",
        "get_traffic",
        "reset_traffic",
        "launch_game",
        "stop_game",
        "open_instance_folder",
        "export_diagnostics",
        "report_frontend_error",
        "note_shown_error",
        "list_launcher_failures",
        "dismiss_launcher_failure",
        "preview_crash_report",
        "send_crash_report",
        "send_explanation_feedback",
        "check_for_update",
        "download_update",
        "restart_to_update",
        "dismiss_whats_new",
        "open_update_page",
        "interface_ready",
        "launcher_account_open",
        "launcher_account_sign_out",
        "launcher_account_set_nick",
        "launcher_account_resend_confirmation",
        "launcher_account_recheck",
        "friends_state",
        "friends_request",
        "friends_act",
        "friends_open_direct",
        "friends_create_group",
        "friends_change_room",
        "friends_messages",
        "friends_send",
        "friends_read",
        "friends_report",
        "friends_set_visible",
        "friends_decline_all",
        "friends_report_request",
        "launcher_account_export",
        "mods_search",
        "mods_project",
        "mods_versions",
        "mods_version",
        "mods_fetch_version",
        "mods_imports",
        "mods_import",
        "mods_import_state",
        "mods_choose",
        "mods_plan",
        "mods_install",
        "mods_pack",
        "mods_create_pack_instance",
        "mods_apply_pack",
        "mods_update_pack",
        "mods_export_pack",
        "mods_cancel",
        "mods_content",
        "mods_identify",
        "mods_updates",
        "mods_set_enabled",
        "mods_remove",
        "mods_open_link",
        "skins_mine",
        "skins_texture",
        "skins_pick",
        "skins_upload",
        "skins_remove",
        "feed_get",
        "feed_page",
        "feed_note",
        "feed_play",
        "feed_open_link",
        "feed_channels",
        "feed_add_channel",
        "feed_remove_channel",
        "feed_own_videos",
        "feed_streamers",
        "feed_add_streamer",
        "feed_remove_streamer",
        "feed_own_streams",
        "worlds_state",
        "worlds_versions",
        "worlds_local",
        "worlds_rooms",
        "worlds_room",
        "worlds_create",
        "worlds_accept_eula",
        "worlds_moderate",
        "worlds_leave_room",
        "worlds_invite",
        "worlds_preview",
        "worlds_redeem",
        "worlds_host",
        "worlds_stop_hosting",
        "worlds_join",
        "worlds_disconnect",
        "worlds_cancel",
        "worlds_instance",
        "worlds_play",
        "worlds_devices",
        "worlds_revoke_device",
        "worlds_importable",
        "worlds_import",
        "worlds_backups",
        "worlds_back_up",
        "worlds_restore",
        "worlds_export",
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
