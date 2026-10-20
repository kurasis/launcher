// The single typed entry point for Tauri IPC. UI code must not call `invoke` or
// `listen` directly, so every command name, payload shape and event subscription is
// declared in one place.
import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { localizeError } from './strings';
import { strings } from './strings';
import {
  BACKEND_EVENT_NAMES,
  type AppInfo,
  type AppSnapshot,
  type BackendEvent,
  type CancelAck,
  type CatalogView,
  type ConsolePage,
  type CreateInstancePayload,
  type Delivery,
  type FailureSummary,
  type ReportPreview,
  type ReportSubject,
  type DataLocation,
  type ExportResult,
  type ExternalLink,
  type GameFolder,
  type InstanceSummary,
  type LauncherError,
  type LifecycleScenario,
  type LocationTarget,
  type LoaderChoice,
  type LoaderKind,
  type LoaderVersion,
  type MutationRequest,
  type OperationStarted,
  type PlanSummary,
  type QuitAction,
  type QuitResult,
  type RunSummary,
  type SettingsPatch,
  type BackupInfo,
  type BackupOverview,
  type Gallery,
  type TrafficSummary,
  type UpdateView,
  type LauncherAccountProblem,
  FRIENDS_PROBLEMS,
  LAUNCHER_ACCOUNT_PROBLEMS,
  type ChatMessage,
  type FriendAction,
  type FriendRequestResult,
  type FriendsProblem,
  type FriendsView,
  SKINS_PROBLEMS,
  type PickedTexture,
  type SkinKind,
  type SkinsProblem,
  type Wardrobe,
  FEED_PROBLEMS,
  type FeedBlock,
  type FeedPage,
  type FeedProblem,
  type FeedStream,
  type FeedVideo,
  type FeedView,
  type NoteView,
  type OwnChannel,
  type OwnStreamer,
  type LocalWorld,
  type WorldsCreatePayload,
  type WorldsDevice,
  type WorldsImportable,
  type WorldsImportPayload,
  type WorldsInstance,
  type WorldsNewInvite,
  type WorldsPreview,
  type WorldsProblem,
  type WorldsRoom,
  type WorldsRoomPage,
  type WorldsState,
  WORLDS_PROBLEMS,
  type BackupScope,
  type JavaCheck,
  type NoTokenProfile,
  type ProfileRef,
  type SetInstanceJavaPayload,
  type SetInstanceLoaderPayload,
  type MemoryRecommendation,
  type ResetChoice,
  type ResetPreview,
  type ResetReport,
  type UserData,
  type RenameInstancePayload,
  type SetInstanceNotesPayload,
  type SettingsView,
  type UpdateInstanceSettingsPayload,
  type ContentIdentified,
  type ContentUpdate,
  type ContentView,
  type ModsImportRequest,
  type ModsInstallPayload,
  type ModsNeededBy,
  type ModsPack,
  type ModsPackReport,
  type ModsExportChoice,
  type ModsExportResult,
  type ModsPlan,
  type ModsProgress,
  type ModsProject,
  type ModsSearchPage,
  type ModsSearchQuery,
  type ModsSource,
  type ModsVersion,
} from './contracts';

function newRequestId(): string {
  return crypto.randomUUID();
}

function mutation<T>(payload: T, requestId: string = newRequestId()): MutationRequest<T> {
  return { requestId, payload };
}

export const ipc = {
  getAppInfo: (): Promise<AppInfo> => invoke<AppInfo>('get_app_info'),
  getAppSnapshot: (): Promise<AppSnapshot> => invoke<AppSnapshot>('get_app_snapshot'),
  createInstance: (payload: CreateInstancePayload, requestId?: string): Promise<InstanceSummary> =>
    invoke<InstanceSummary>('create_instance', { request: mutation(payload, requestId) }),
  getDataLocation: (): Promise<DataLocation> => invoke<DataLocation>('get_data_location'),
  pickDataFolder: (title?: string): Promise<string | null> =>
    invoke<string | null>('pick_data_folder', { title }),
  moveDataLocation: (
    target: LocationTarget,
    path: string | null,
    requestId?: string,
  ): Promise<DataLocation> =>
    invoke<DataLocation>('move_data_location', {
      request: mutation({ target, path }, requestId),
    }),
  listBackups: (): Promise<BackupOverview> => invoke<BackupOverview>('list_backups'),
  createBackup: (instanceId: string, scope: BackupScope, requestId?: string): Promise<BackupInfo> =>
    invoke<BackupInfo>('create_backup', { request: mutation({ instanceId, scope }, requestId) }),
  restoreBackup: (
    backupId: string,
    asCopy: boolean,
    requestId?: string,
  ): Promise<{ world: string | null }> =>
    invoke<{ world: string | null }>('restore_backup', {
      request: mutation({ backupId, asCopy }, requestId),
    }),
  deleteBackup: (backupId: string, requestId?: string): Promise<number> =>
    invoke<number>('delete_backup', { request: mutation({ backupId }, requestId) }),
  /** For an installation (counting its mods) or a new one of a version (D-058). */
  recommendMemory: (
    target: { instanceId: string } | { versionId: string; modded: boolean },
  ): Promise<MemoryRecommendation> => invoke<MemoryRecommendation>('recommend_memory', target),
  resetPreview: (): Promise<ResetPreview> => invoke<ResetPreview>('reset_preview'),
  resetLauncher: (choice: ResetChoice, requestId?: string): Promise<ResetReport> =>
    invoke<ResetReport>('reset_launcher', { request: mutation(choice, requestId) }),
  inspectInstance: (instanceId: string): Promise<UserData> =>
    invoke<UserData>('inspect_instance', { instanceId }),
  /** Deletes an installation, after a backup when `backup`; returns that backup. */
  deleteInstance: (
    instanceId: string,
    backup: boolean,
    requestId?: string,
  ): Promise<BackupInfo | null> =>
    invoke<BackupInfo | null>('delete_instance', {
      request: mutation({ instanceId, backup }, requestId),
    }),
  checkJava: (instanceId: string, path: string): Promise<JavaCheck> =>
    invoke<JavaCheck>('check_java', { payload: { instanceId, path } }),
  pickJavaFile: (title?: string): Promise<string | null> =>
    invoke<string | null>('pick_java_file', { title }),
  setInstanceJava: (
    payload: SetInstanceJavaPayload,
    requestId?: string,
  ): Promise<InstanceSummary> =>
    invoke<InstanceSummary>('set_instance_java', { request: mutation(payload, requestId) }),
  setInstanceLoader: (
    payload: SetInstanceLoaderPayload,
    requestId?: string,
  ): Promise<InstanceSummary> =>
    invoke<InstanceSummary>('set_instance_loader', { request: mutation(payload, requestId) }),
  setInstanceNotes: (
    payload: SetInstanceNotesPayload,
    requestId?: string,
  ): Promise<InstanceSummary> =>
    invoke<InstanceSummary>('set_instance_notes', { request: mutation(payload, requestId) }),
  renameInstance: (payload: RenameInstancePayload, requestId?: string): Promise<InstanceSummary> =>
    invoke<InstanceSummary>('rename_instance', { request: mutation(payload, requestId) }),
  updateInstanceSettings: (
    payload: UpdateInstanceSettingsPayload,
    requestId?: string,
  ): Promise<InstanceSummary> =>
    invoke<InstanceSummary>('update_instance_settings', { request: mutation(payload, requestId) }),
  setAppSettings: (patch: SettingsPatch, requestId?: string): Promise<SettingsView> =>
    invoke<SettingsView>('set_app_settings', { request: mutation(patch, requestId) }),
  showMainWindow: (): Promise<void> => invoke('show_main_window'),
  /** Opens or focuses the console window (D-037). */
  openConsole: (): Promise<void> => invoke('open_console'),
  /** Console entries after `after`; only the console window may call this. */
  readConsole: (after: number): Promise<ConsolePage> =>
    invoke<ConsolePage>('read_console', { after }),
  beginLogin: (requestId?: string): Promise<{ operationId: string }> =>
    invoke<{ operationId: string }>('begin_login', { request: mutation({}, requestId) }),
  cancelLogin: (operationId: string, requestId?: string): Promise<void> =>
    invoke('cancel_login', { request: mutation({ operationId }, requestId) }),
  /** Removes one Microsoft account (or retries a pending removal). */
  signOut: (profileId: string, requestId?: string): Promise<void> =>
    invoke('sign_out', { request: mutation({ profileId }, requestId) }),
  verifyAccount: (profileId: string, requestId?: string): Promise<void> =>
    invoke('verify_account', { request: mutation({ profileId }, requestId) }),
  addNoTokenProfile: (name: string, requestId?: string): Promise<NoTokenProfile> =>
    invoke<NoTokenProfile>('add_notoken_profile', { request: mutation({ name }, requestId) }),
  removeNoTokenProfile: (profileId: string, requestId?: string): Promise<void> =>
    invoke('remove_notoken_profile', { request: mutation({ profileId }, requestId) }),
  setDefaultProfile: (profile: ProfileRef | null, requestId?: string): Promise<void> =>
    invoke('set_default_profile', { request: mutation({ profile }, requestId) }),
  setInstanceProfile: (
    instanceId: string,
    profile: ProfileRef | null,
    requestId?: string,
  ): Promise<InstanceSummary> =>
    invoke<InstanceSummary>('set_instance_profile', {
      request: mutation({ instanceId, profile }, requestId),
    }),
  openExternal: (link: ExternalLink): Promise<void> => invoke('open_external', { link }),
  installInstance: (instanceId: string, requestId?: string): Promise<OperationStarted> =>
    invoke<OperationStarted>('install_instance', {
      request: mutation({ instanceId }, requestId),
    }),
  repairInstance: (instanceId: string, requestId?: string): Promise<OperationStarted> =>
    invoke<OperationStarted>('repair_instance', {
      request: mutation({ instanceId }, requestId),
    }),
  cancelOperation: (operationId: string, requestId?: string): Promise<CancelAck> =>
    invoke<CancelAck>('cancel_operation', { request: mutation({ operationId }, requestId) }),
  answerDownload: (operationId: string, allow: boolean, requestId?: string): Promise<true> =>
    invoke('answer_download', { request: mutation({ operationId, allow }, requestId) }).then(
      () => true as const,
    ),
  checkNetwork: (): Promise<void> => invoke('check_network'),
  getTraffic: (): Promise<TrafficSummary> => invoke<TrafficSummary>('get_traffic'),
  resetTraffic: (requestId?: string): Promise<TrafficSummary> =>
    invoke<TrafficSummary>('reset_traffic', { request: mutation({}, requestId) }),
  listScreenshots: (): Promise<Gallery> => invoke<Gallery>('list_screenshots'),
  deleteScreenshot: (instanceId: string, file: string, requestId?: string): Promise<true> =>
    invoke('delete_screenshot', { request: mutation({ instanceId, file }, requestId) }).then(
      () => true as const,
    ),
  openScreenshot: (
    instanceId: string,
    file: string,
    reveal: boolean,
    requestId?: string,
  ): Promise<void> =>
    invoke('open_screenshot', { request: mutation({ instanceId, file, reveal }, requestId) }),
  /** The game folder, or one of its known folders. */
  openInstanceFolder: (
    instanceId: string,
    requestId?: string,
    folder: GameFolder | null = null,
  ): Promise<void> =>
    invoke('open_instance_folder', { request: mutation({ instanceId, folder }, requestId) }),
  exportDiagnostics: (
    runId: string | null,
    requestId?: string,
    title?: string,
  ): Promise<ExportResult> =>
    invoke<ExportResult>('export_diagnostics', {
      request: mutation({ runId, title }, requestId),
    }),
  /** Plays as the installation's profile: a Microsoft account, or the demo (D-045). */
  /** Starts the game; with `server` (a friend's invite) straight into that server. */
  launchGame: (instanceId: string, requestId?: string, server?: string): Promise<RunSummary> =>
    invoke<RunSummary>('launch_game', {
      request: mutation(server === undefined ? { instanceId } : { instanceId, server }, requestId),
    }),
  stopGame: (runId: string, requestId?: string): Promise<boolean> =>
    invoke<boolean>('stop_game', { request: mutation({ runId }, requestId) }),
  listSupportedVersions: (refresh: boolean): Promise<CatalogView> =>
    invoke<CatalogView>('list_supported_versions', { refresh }),
  previewInstallPlan: (versionId: string, loader: LoaderChoice | null): Promise<PlanSummary> =>
    invoke<PlanSummary>('preview_install_plan', { versionId, loader }),
  listLoaderGameVersions: (kind: LoaderKind): Promise<string[]> =>
    invoke<string[]>('list_loader_game_versions', { kind }),
  listLoaderVersions: (kind: LoaderKind, gameVersion: string): Promise<LoaderVersion[]> =>
    invoke<LoaderVersion[]>('list_loader_versions', { kind, gameVersion }),
  requestQuit: (action: QuitAction, requestId?: string): Promise<QuitResult> =>
    invoke<QuitResult>('request_quit', { request: mutation({ action }, requestId) }),
  listLauncherFailures: (): Promise<FailureSummary[]> =>
    invoke<FailureSummary[]>('list_launcher_failures'),
  dismissLauncherFailure: (id: string): Promise<void> => invoke('dismiss_launcher_failure', { id }),
  /** What a report would send, scrubbed; nothing leaves the computer yet. */
  previewCrashReport: (subject: ReportSubject): Promise<ReportPreview> =>
    invoke<ReportPreview>('preview_crash_report', { subject }),
  sendCrashReport: (previewId: string, files: string[], note: string): Promise<Delivery> =>
    invoke<Delivery>('send_crash_report', { previewId, files, note }),
  sendExplanationFeedback: (kind: string, signature: string, helpful: boolean): Promise<void> =>
    invoke('send_explanation_feedback', { kind, signature, helpful }),
  checkForUpdate: (): Promise<UpdateView> => invoke<UpdateView>('check_for_update'),
  downloadUpdate: (): Promise<UpdateView> => invoke<UpdateView>('download_update'),
  /** Installs the downloaded update and starts the new version. */
  restartToUpdate: (): Promise<void> => invoke('restart_to_update'),
  dismissWhatsNew: (): Promise<void> => invoke('dismiss_whats_new'),
  openUpdatePage: (): Promise<void> => invoke('open_update_page'),
  /** Opens the launcher account's window on its sign-in or registration page. */
  launcherAccountOpen: (screen: 'login' | 'register' | 'delete', title?: string): Promise<void> =>
    invoke('launcher_account_open', { screen, title }),
  launcherAccountSignOut: (): Promise<void> => invoke('launcher_account_sign_out'),
  /** Null when the nick changed, else what went wrong. */
  launcherAccountSetNick: (nick: string): Promise<LauncherAccountProblem | null> =>
    invoke<LauncherAccountProblem | null>('launcher_account_set_nick', { nick }),
  launcherAccountResendConfirmation: (): Promise<LauncherAccountProblem | null> =>
    invoke<LauncherAccountProblem | null>('launcher_account_resend_confirmation'),
  /** Asks the server again; whether the email is confirmed now. Rejects with a
   * LauncherAccountProblem string. */
  launcherAccountRecheck: (): Promise<boolean> => invoke<boolean>('launcher_account_recheck'),
  // Friends and chats (D-064). These reject with a FriendsProblem string;
  // `friendsProblem` reads it.
  friendsState: (): Promise<FriendsView> => invoke<FriendsView>('friends_state'),
  friendsRequest: (nick: string): Promise<FriendRequestResult> =>
    invoke<FriendRequestResult>('friends_request', { nick }),
  friendsAct: (action: FriendAction, id: string): Promise<void> =>
    invoke('friends_act', { action, id }),
  /** The direct chat with a friend (made when there is none); its room id. */
  friendsOpenDirect: (id: string): Promise<string> => invoke<string>('friends_open_direct', { id }),
  friendsCreateGroup: (name: string, members: string[]): Promise<string> =>
    invoke<string>('friends_create_group', { name, members }),
  /** Owner: add, remove or rename (`name`); anyone: remove themselves (leave). */
  friendsChangeRoom: (
    room: string,
    action: 'add' | 'remove' | 'name',
    value: string,
  ): Promise<void> => invoke('friends_change_room', { room, action, value }),
  /** Up to 50 messages before `before`, oldest first. */
  friendsMessages: (room: string, before: number | null): Promise<ChatMessage[]> =>
    invoke<ChatMessage[]>('friends_messages', { room, before }),
  friendsSend: (room: string, body: string): Promise<ChatMessage> =>
    invoke<ChatMessage>('friends_send', { room, body }),
  friendsRead: (room: string, upto: number): Promise<void> =>
    invoke('friends_read', { room, upto }),
  friendsReport: (message: number, reason: string): Promise<void> =>
    invoke('friends_report', { message, reason }),
  friendsDeclineAll: (): Promise<void> => invoke('friends_decline_all'),
  /** Reports a waiting request as spam; it goes. */
  friendsReportRequest: (id: string): Promise<void> => invoke('friends_report_request', { id }),
  friendsSetVisible: (visible: boolean): Promise<void> =>
    invoke('friends_set_visible', { visible }),
  // The launcher account's skin and cape (D-065). These reject with a
  // SkinsProblem string; `skinsProblem` reads it.
  skinsMine: (): Promise<Wardrobe> => invoke<Wardrobe>('skins_mine'),
  /** A worn texture by its hash, as a `data:` URL. */
  skinsTexture: (hash: string): Promise<string> => invoke<string>('skins_texture', { hash }),
  /** Picks a PNG in the native dialog; null if it was closed. */
  skinsPick: (kind: SkinKind, title?: string): Promise<PickedTexture | null> =>
    invoke<PickedTexture | null>('skins_pick', { kind, title }),
  /** Uploads what was picked for `kind`. */
  skinsUpload: (kind: SkinKind, slim: boolean, frameMs: number[] | null): Promise<Wardrobe> =>
    invoke<Wardrobe>('skins_upload', { kind, slim, frameMs }),
  skinsRemove: (kind: SkinKind): Promise<Wardrobe> => invoke<Wardrobe>('skins_remove', { kind }),
  /** Saves the account's data as a file; its name, or null if the dialog was closed. */
  launcherAccountExport: (title?: string): Promise<string | null> =>
    invoke<string | null>('launcher_account_export', { title }),
  // The catalog of mods (D-066).
  modsSearch: (query: ModsSearchQuery): Promise<ModsSearchPage> =>
    invoke<ModsSearchPage>('mods_search', { query }),
  /** By our ID or the slug. */
  modsProject: (project: string): Promise<ModsProject> =>
    invoke<ModsProject>('mods_project', { project }),
  /** Newest first, without changelogs. */
  modsVersions: (
    project: string,
    game: string | null,
    loader: string | null,
  ): Promise<ModsVersion[]> => invoke<ModsVersion[]>('mods_versions', { project, game, loader }),
  modsVersion: (version: string): Promise<ModsVersion> =>
    invoke<ModsVersion>('mods_version', { version }),
  /** Asks the catalog to keep a version's files. */
  modsFetchVersion: (version: string): Promise<void> => invoke('mods_fetch_version', { version }),
  modsImports: (): Promise<ModsImportRequest[]> => invoke<ModsImportRequest[]>('mods_imports'),
  modsImport: (input: string): Promise<ModsImportRequest> =>
    invoke<ModsImportRequest>('mods_import', { input }),
  modsImportState: (request: string): Promise<ModsImportRequest> =>
    invoke<ModsImportRequest>('mods_import_state', { request }),
  modsChoose: (request: string, source: ModsSource, id: string): Promise<ModsImportRequest> =>
    invoke<ModsImportRequest>('mods_choose', { request, source, id }),
  modsPlan: (payload: ModsInstallPayload): Promise<ModsPlan> =>
    invoke<ModsPlan>('mods_plan', { payload }),
  /** How many files are in place; progress comes as `onModsProgress`. */
  modsInstall: (payload: ModsInstallPayload, requestId?: string): Promise<number> =>
    invoke<number>('mods_install', { request: mutation(payload, requestId) }),
  /** A modpack version; `prepare` asks the catalog to keep its files (once). */
  modsPack: (version: string, prepare: boolean): Promise<ModsPack> =>
    invoke<ModsPack>('mods_pack', { version, prepare }),
  /** A new installation for a ready modpack (its files come with `modsApplyPack`). */
  modsCreatePackInstance: (
    payload: { version: string; displayName: string },
    requestId?: string,
  ): Promise<InstanceSummary> =>
    invoke<InstanceSummary>('mods_create_pack_instance', {
      request: mutation(payload, requestId),
    }),
  /** Puts a modpack's files into its installation; progress comes as `onModsProgress`. */
  modsApplyPack: (
    payload: { instanceId: string; version: string },
    requestId?: string,
  ): Promise<ModsPackReport> =>
    invoke<ModsPackReport>('mods_apply_pack', { request: mutation(payload, requestId) }),
  /** Updates an installation made from a modpack, after a backup of it (D-047). */
  modsUpdatePack: (
    payload: { instanceId: string; version: string },
    requestId?: string,
  ): Promise<ModsPackReport> =>
    invoke<ModsPackReport>('mods_update_pack', { request: mutation(payload, requestId) }),
  /** Saves an installation as a `.mrpack` where the player chooses. */
  modsExportPack: (
    instanceId: string,
    choice: ModsExportChoice,
    title?: string,
  ): Promise<ModsExportResult> =>
    invoke<ModsExportResult>('mods_export_pack', {
      payload: { instanceId, choice, title: title ?? null },
    }),
  modsCancel: (instanceId: string): Promise<boolean> =>
    invoke<boolean>('mods_cancel', { instanceId }),
  modsContent: (instanceId: string): Promise<ContentView> =>
    invoke<ContentView>('mods_content', { instanceId }),
  modsIdentify: (instanceId: string): Promise<ContentIdentified[]> =>
    invoke<ContentIdentified[]>('mods_identify', { instanceId }),
  modsUpdates: (instanceId: string): Promise<ContentUpdate[]> =>
    invoke<ContentUpdate[]>('mods_updates', { instanceId }),
  modsSetEnabled: (
    instanceId: string,
    path: string,
    enabled: boolean,
    requestId?: string,
  ): Promise<void> =>
    invoke('mods_set_enabled', { request: mutation({ instanceId, path, enabled }, requestId) }),
  modsNeededBy: (instanceId: string, path: string): Promise<ModsNeededBy> =>
    invoke<ModsNeededBy>('mods_needed_by', { instanceId, path }),
  modsRemove: (instanceId: string, path: string, requestId?: string): Promise<void> =>
    invoke('mods_remove', { request: mutation({ instanceId, path }, requestId) }),
  /** Opens an https link in the system browser; ask the player first. */
  modsOpenLink: (url: string): Promise<void> => invoke('mods_open_link', { url }),
  // The start page feed (D-067). These reject with a FeedProblem string;
  // `feedProblem` reads it.
  /** `refresh` asks the service again even when it answered a moment ago. */
  feedGet: (refresh = false): Promise<FeedView> => invoke<FeedView>('feed_get', { refresh }),
  /** A whole note; `original` asks for the English text instead of a translation. */
  feedNote: (id: string, original = false): Promise<NoteView> =>
    invoke<NoteView>('feed_note', { id, original }),
  /** Plays a video (`yt`) or stream (`twitch`) where the settings say. */
  feedPlay: (kind: 'yt' | 'twitch', id: string, title?: string): Promise<void> =>
    invoke('feed_play', { kind, id, title }),
  /** Opens a link of the feed in the browser (only addresses the core allows). */
  feedOpenLink: (url: string): Promise<void> => invoke('feed_open_link', { url }),
  feedChannels: (): Promise<OwnChannel[]> => invoke<OwnChannel[]>('feed_channels'),
  /** Looks a channel up by its link, @handle or id and adds it; the list after. */
  feedAddChannel: (input: string): Promise<OwnChannel[]> =>
    invoke<OwnChannel[]>('feed_add_channel', { input }),
  feedRemoveChannel: (channelId: string): Promise<OwnChannel[]> =>
    invoke<OwnChannel[]>('feed_remove_channel', { channelId }),
  feedOwnVideos: (): Promise<FeedVideo[]> => invoke<FeedVideo[]>('feed_own_videos'),
  feedStreamers: (): Promise<OwnStreamer[]> => invoke<OwnStreamer[]>('feed_streamers'),
  /** Looks a Twitch channel up by its link or name and adds it; the list after. */
  feedAddStreamer: (input: string): Promise<OwnStreamer[]> =>
    invoke<OwnStreamer[]>('feed_add_streamer', { input }),
  feedRemoveStreamer: (login: string): Promise<OwnStreamer[]> =>
    invoke<OwnStreamer[]>('feed_remove_streamer', { login }),
  /** The live streams of the player's own Twitch channels. */
  feedOwnStreams: (): Promise<FeedStream[]> => invoke<FeedStream[]>('feed_own_streams'),
  /**
   * A page of the endless list: one part or all (`null`), or the Shorts alone,
   * after the cursor `before`.
   */
  feedPage: (
    filter: FeedBlock | null,
    before: string | null,
    shortsOnly = false,
  ): Promise<FeedPage> => invoke<FeedPage>('feed_page', { filter, shortsOnly, before }),
  // Worlds with friends (D-068). Failures are LauncherErrors; `worldsProblemOf`
  // reads the service's problem from one.
  worldsState: (): Promise<WorldsState> => invoke<WorldsState>('worlds_state'),
  worldsVersions: (): Promise<string[]> => invoke<string[]>('worlds_versions'),
  worldsLocal: (): Promise<LocalWorld[]> => invoke<LocalWorld[]>('worlds_local'),
  worldsRooms: (): Promise<WorldsRoom[]> => invoke<WorldsRoom[]>('worlds_rooms'),
  worldsRoom: (room: string): Promise<WorldsRoomPage> =>
    invoke<WorldsRoomPage>('worlds_room', { room }),
  /** A world on this computer and its room. */
  worldsCreate: (payload: WorldsCreatePayload): Promise<WorldsRoom> =>
    invoke<WorldsRoom>('worlds_create', { payload }),
  worldsAcceptEula: (world: string): Promise<LocalWorld> =>
    invoke<LocalWorld>('worlds_accept_eula', { world }),
  worldsModerate: (
    room: string,
    account: string,
    action: 'remove' | 'ban' | 'unban',
  ): Promise<void> => invoke('worlds_moderate', { room, account, action }),
  worldsLeaveRoom: (room: string): Promise<void> => invoke('worlds_leave_room', { room }),
  worldsInvite: (room: string, hours: number, uses: number): Promise<WorldsNewInvite> =>
    invoke<WorldsNewInvite>('worlds_invite', { room, hours, uses }),
  worldsPreview: (code: string): Promise<WorldsPreview> =>
    invoke<WorldsPreview>('worlds_preview', { code }),
  worldsRedeem: (code: string): Promise<WorldsRoom> =>
    invoke<WorldsRoom>('worlds_redeem', { code }),
  /** Resolves once friends can join; stages come in `onWorldsState`. */
  worldsHost: (room: string): Promise<void> => invoke('worlds_host', { room }),
  worldsStopHosting: (): Promise<void> => invoke('worlds_stop_hosting'),
  worldsJoin: (room: string): Promise<void> => invoke('worlds_join', { room }),
  worldsDisconnect: (): Promise<void> => invoke('worlds_disconnect'),
  worldsCancel: (): Promise<void> => invoke('worlds_cancel'),
  /** The installation the room plays, made on first use. */
  worldsInstance: (room: string): Promise<WorldsInstance> =>
    invoke<WorldsInstance>('worlds_instance', { room }),
  /** Starts the game into the room as the launcher account's nick. */
  worldsPlay: (room: string, requestId?: string): Promise<RunSummary> =>
    invoke<RunSummary>('worlds_play', { request: mutation({ room }, requestId) }),
  /** The account's computers for worlds, this one first. */
  worldsDevices: (): Promise<WorldsDevice[]> => invoke<WorldsDevice[]>('worlds_devices'),
  /** Signs another computer out of worlds; the worlds it hosts close. */
  worldsRevokeDevice: (device: string): Promise<void> => invoke('worlds_revoke_device', { device }),
  /** Installations whose single-player worlds can be imported (W9). */
  worldsImportable: (): Promise<WorldsImportable[]> =>
    invoke<WorldsImportable[]>('worlds_importable'),
  /** A new world from a copy of a single-player world, and its room. */
  worldsImport: (payload: WorldsImportPayload): Promise<WorldsRoom> =>
    invoke<WorldsRoom>('worlds_import', { payload }),
  /** A world's backups, newest first. */
  worldsBackups: (world: string): Promise<BackupInfo[]> =>
    invoke<BackupInfo[]>('worlds_backups', { world }),
  worldsBackUp: (world: string): Promise<BackupInfo> =>
    invoke<BackupInfo>('worlds_back_up', { world }),
  /** Puts a backup in place of the stopped world; it is backed up first. */
  worldsRestore: (world: string, backup: string): Promise<void> =>
    invoke('worlds_restore', { world, backup }),
  /** Saves the stopped world as a zip where the player chooses. */
  worldsExport: (world: string, title?: string): Promise<ExportResult> =>
    invoke<ExportResult>('worlds_export', { world, title: title ?? null }),
  /** The interface has shown: a new version's start can count as good. */
  interfaceReady: (): Promise<void> => invoke('interface_ready'),
  /** Debug builds only; release builds have no handler for this command. */
  startLifecycleTest: (scenario: LifecycleScenario, requestId?: string): Promise<RunSummary> =>
    invoke<RunSummary>('start_lifecycle_test', { request: mutation(scenario, requestId) }),
  /** Debug builds only. */
  stopLifecycleTest: (runId: string): Promise<boolean> =>
    invoke<boolean>('stop_lifecycle_test', { runId }),
};

/** Commands whose failures are not noted again (they are the noting). */
const UNNOTED = new Set(['note_shown_error', 'report_frontend_error']);

/**
 * Calls a command; a failure's code and diagnostic ID also go to the
 * launcher log (D-061), so a report shows what the player saw.
 */
function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return tauriInvoke<T>(command, args).catch((error: unknown) => {
    if (!UNNOTED.has(command) && isLauncherError(error)) {
      void tauriInvoke('note_shown_error', {
        command,
        code: error.code,
        diagnosticId: error.diagnosticId,
      }).catch(() => undefined);
    }
    throw error;
  });
}

/** Records an error in the window's code in the launcher's failure records. */
export function reportFrontendError(message: string, stack?: string): void {
  void tauriInvoke('report_frontend_error', { message, stack: stack ?? null }).catch(
    () => undefined,
  );
}

/** True when a rejected IPC value is the backend's typed error envelope. */
export function isLauncherError(error: unknown): error is LauncherError {
  return (
    typeof error === 'object' &&
    error !== null &&
    typeof (error as { code?: unknown }).code === 'string' &&
    typeof (error as { message?: unknown }).message === 'string'
  );
}

/** Converts an unknown IPC rejection into a message that is safe to show. */
export function describeIpcError(error: unknown): string {
  if (isLauncherError(error) && error.message.length > 0) {
    return localizeError(error);
  }
  if (typeof error === 'string' && error.length > 0) {
    return error;
  }
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return strings.bootstrap.noResponse;
}

type EventHandler = (event: BackendEvent) => void;

const handlers = new Set<EventHandler>();
let registration: Promise<UnlistenFn[]> | null = null;

function dispatch(event: BackendEvent): void {
  for (const handler of handlers) {
    handler(event);
  }
}

/**
 * Subscribes to all backend events. However many components subscribe (including
 * React development double-mounts), exactly one Tauri listener exists per event
 * name; it is removed when the last subscriber unsubscribes. Returns the
 * unsubscribe function and a promise that resolves once listening has started.
 */
export function subscribeBackendEvents(handler: EventHandler): {
  unsubscribe: () => void;
  ready: Promise<void>;
} {
  handlers.add(handler);
  registration ??= Promise.all(
    BACKEND_EVENT_NAMES.map((name) =>
      listen(name, (event) => {
        dispatch({ name, payload: event.payload } as BackendEvent);
      }),
    ),
  );
  const current = registration;
  // A registration that failed is not kept: the next subscriber tries again.
  current.catch(() => {
    if (registration === current) registration = null;
  });
  let active = true;
  return {
    ready: current.then(() => undefined),
    unsubscribe: () => {
      if (!active) return;
      active = false;
      handlers.delete(handler);
      if (handlers.size === 0 && registration === current) {
        registration = null;
        void current
          .then((unlisteners) => {
            for (const unlisten of unlisteners) {
              // UnlistenFn returns a promise in Tauri 2. It rejects when the webview
              // is being torn down, in which case its listeners are gone already.
              void Promise.resolve()
                .then(unlisten)
                .catch(() => undefined);
            }
          })
          .catch(() => undefined);
      }
    },
  };
}

/** Listens to one event; returns the function that stops listening. */
// The payload's type is the event's, which the caller names.
// eslint-disable-next-line @typescript-eslint/no-unnecessary-type-parameters
function listenTo<T>(name: string, handler: (payload: T) => void): () => void {
  const pending = listen<T>(name, (event) => {
    handler(event.payload);
  });
  return () => {
    void pending.then((unlisten) => Promise.resolve().then(unlisten)).catch(() => undefined);
  };
}

/** The OS or the app menu asked to quit while the core supervises something. */
export function onQuitRequested(handler: (hosting: boolean) => void): () => void {
  return listenTo<{ hosting?: boolean } | null>('quit_requested', (payload) => {
    handler(payload?.hosting === true);
  });
}

/** The FriendsProblem a friends command rejected with (anything else: `server`). */
/** What a launcher account command rejected with; `server` for anything else. */
export function accountProblem(error: unknown): LauncherAccountProblem {
  return typeof error === 'string' &&
    (LAUNCHER_ACCOUNT_PROBLEMS as readonly string[]).includes(error)
    ? (error as LauncherAccountProblem)
    : 'server';
}

export function friendsProblem(error: unknown): FriendsProblem {
  return typeof error === 'string' && (FRIENDS_PROBLEMS as readonly string[]).includes(error)
    ? (error as FriendsProblem)
    : 'server';
}

export function skinsProblem(error: unknown): SkinsProblem {
  return typeof error === 'string' && (SKINS_PROBLEMS as readonly string[]).includes(error)
    ? (error as SkinsProblem)
    : 'server';
}

/** The FeedProblem a feed command rejected with (anything else: `server`). */
export function feedProblem(error: unknown): FeedProblem {
  return typeof error === 'string' && (FEED_PROBLEMS as readonly string[]).includes(error)
    ? (error as FeedProblem)
    : 'server';
}

/** Friends and chats changed (lists, rooms, unread counts, connection). */
export function onFriendsState(handler: (view: FriendsView) => void): () => void {
  return listenTo('friends://state', handler);
}

/** A new message in one of the account's rooms (its own included). */
export function onFriendsMessage(handler: (message: ChatMessage) => void): () => void {
  return listenTo('friends://message', handler);
}

/** A moderator removed a message. */
export function onFriendsRemoved(
  handler: (removed: { room: string; message: number }) => void,
): () => void {
  return listenTo('friends://removed', handler);
}

/** What this computer does for a room of worlds with friends changed. */
export function onWorldsState(handler: (state: WorldsState) => void): () => void {
  return listenTo('worlds://state', handler);
}

/** The worlds.lapi.sh problem a worlds command failed with, if it was one. */
export function worldsProblemOf(error: unknown): WorldsProblem | null {
  if (!isLauncherError(error)) return null;
  const name = error.safeDetails?.worldsProblem;
  return typeof name === 'string' && (WORLDS_PROBLEMS as readonly string[]).includes(name)
    ? (name as WorldsProblem)
    : null;
}

/** Progress of an install from the catalog of mods (D-066). */
export function onModsProgress(handler: (progress: ModsProgress) => void): () => void {
  return listenTo('mods://progress', handler);
}
