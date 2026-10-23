// The single typed entry point for Tauri IPC. UI code must not call `invoke` or
// `listen` directly, so every command name, payload shape and event subscription is
// declared in one place.
import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { askToStart } from './launchCheck';
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
  type MyReviews,
  type ReviewDraft,
  type ReviewFound,
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
  type Closet,
  type WardrobeChange,
  FEED_PROBLEMS,
  type FeedBlock,
  type FeedPage,
  type FeedProblem,
  type FeedStream,
  type FeedVideo,
  type FeedView,
  type ServerAdsView,
  type AdEvent,
  type StatsSent,
  type NoteView,
  type OwnChannel,
  type OwnStreamer,
  type ChannelMark,
  type FavoriteFeed,
  type FeedMarks,
  type MarkList,
  type LocalWorld,
  type WorldsCreatePayload,
  type WorldsDevice,
  type WorldsImportable,
  type WorldsImportPayload,
  type WorldsInstance,
  type WorldsModSource,
  type WorldsKeep,
  type WorldsUpdatePreview,
  type WorldsNewInvite,
  type WorldsPreview,
  type WorldsProblem,
  type WorldsRoom,
  type WorldsRoomPage,
  type WorldsState,
  type WorldsVersionChoice,
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
  type ModsOwnPack,
  type ModsPublishCheck,
  type ModsPublished,
  type ModsPublishForm,
  type ModsReportReason,
  type ModsPlan,
  type ModsSetProblem,
  type ModsImportCreatePayload,
  type ModsImportFound,
  type ModsImportReport,
  type ModsWorldPayload,
  type ModsWorldPlan,
  type ModsProgress,
  type ModsProject,
  type ModsSearchPage,
  type ModsSearchQuery,
  type ModsSource,
  type ModsVersion,
  type ModsCategoryCount,
  type ModsCollection,
  type ModsHit,
  type ModsKind,
  type ModsVersionPage,
  type ModsVersionsQuery,
  type ModsFix,
  type ModsInstalledMatch,
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
  /**
   * Starts the game; with `server` (a friend's invite or a server ad) straight into
   * that server. `ad` names the ad whose Play it was, for the statistics (D-071).
   * When the mods' own files say it may not start, the player is asked first
   * (D-066); `null` when they chose not to.
   */
  launchGame: async (
    instanceId: string,
    requestId?: string,
    server?: string,
    ad?: string,
  ): Promise<RunSummary | null> => {
    const found = await invoke<ModsSetProblem[] | null>('mods_check', { instanceId }).catch(
      () => null,
    );
    if (!(await askToStart(instanceId, Array.isArray(found) ? found : []))) return null;
    return invoke<RunSummary>('launch_game', {
      request: mutation(
        server === undefined
          ? { instanceId }
          : ad === undefined
            ? { instanceId, server }
            : { instanceId, server, ad },
        requestId,
      ),
    });
  },
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
  reviewsLogs: (instanceId: string | null, runId: string | null): Promise<ReviewFound> =>
    invoke<ReviewFound>('reviews_logs', { instanceId, runId }),
  reviewsTextFile: (name: string, text: string): Promise<ReviewFound> =>
    invoke<ReviewFound>('reviews_text_file', { name, text }),
  reviewsScreenshot: (instanceId: string, file: string): Promise<string> =>
    invoke<string>('reviews_screenshot', { instanceId, file }),
  reviewsSend: (draft: ReviewDraft): Promise<Delivery> =>
    invoke<Delivery>('reviews_send', { draft }),
  reviewsMine: (refresh = false): Promise<MyReviews> =>
    invoke<MyReviews>('reviews_mine', { refresh }),
  reviewsSeen: (): Promise<void> => invoke('reviews_seen'),
  reviewsDelete: (id: number): Promise<void> => invoke('reviews_delete', { id }),
  sendExplanationFeedback: (kind: string, signature: string, helpful: boolean): Promise<void> =>
    invoke('send_explanation_feedback', { kind, signature, helpful }),
  /** The last ping and ad counts as they went, and today's ping (D-071). */
  statsSent: (): Promise<StatsSent> => invoke<StatsSent>('stats_sent'),
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
  /** Other players' heads by nick (up to 200): a square PNG `data:` URL, or null
   * for a nick without a skin of ours. */
  skinsHeads: (nicks: string[]): Promise<Record<string, string | null>> =>
    invoke<Record<string, string | null>>('skins_heads', { nicks }),
  /** Takes the skin or cape another player wears, by nick, as a picked one. */
  /** The heads of the Microsoft accounts signed in here, by profile ID: the skin
   * Mojang serves for each, or null (draw the default). */
  skinsMicrosoftHeads: (): Promise<Record<string, string | null>> =>
    invoke<Record<string, string | null>>('skins_microsoft_heads'),
  skinsByNick: (kind: SkinKind, nick: string): Promise<PickedTexture> =>
    invoke<PickedTexture>('skins_by_nick', { kind, nick }),
  /** Picks a PNG, GIF or APNG in the native dialog; null if it was closed. */
  skinsPick: (kind: SkinKind, title?: string): Promise<PickedTexture | null> =>
    invoke<PickedTexture | null>('skins_pick', { kind, title }),
  /** A file dropped on the page: its name and its contents in base64. */
  skinsTake: (kind: SkinKind, name: string, data: string): Promise<PickedTexture> =>
    invoke<PickedTexture>('skins_take', { kind, name, data }),
  /** Saves a picture of the character (a PNG in base64) where the player chooses;
   * the file's name, or null if the dialog was closed. */
  skinsSavePicture: (data: string, name: string, title?: string): Promise<string | null> =>
    invoke<string | null>('skins_save_picture', { data, name, title }),
  /** Uploads what was picked for `kind`. */
  skinsUpload: (kind: SkinKind, slim: boolean, frameMs: number[] | null): Promise<Wardrobe> =>
    invoke<Wardrobe>('skins_upload', { kind, slim, frameMs }),
  skinsRemove: (kind: SkinKind): Promise<Wardrobe> => invoke<Wardrobe>('skins_remove', { kind }),
  /** The account's wardrobe (D-065). */
  skinsWardrobe: (): Promise<Closet> => invoke<Closet>('skins_wardrobe'),
  /** Uploads what was picked for `kind` into the wardrobe without wearing it
   * (counted as an upload); an empty name lets the interface name it. */
  skinsKeep: (
    kind: SkinKind,
    slim: boolean,
    frameMs: number[] | null,
    name: string,
  ): Promise<Closet> => invoke<Closet>('skins_keep', { kind, slim, frameMs, name }),
  skinsWardrobeChange: (change: WardrobeChange): Promise<Closet> =>
    invoke<Closet>('skins_wardrobe_change', { change }),
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
  /** A page of versions, newest first, with the game versions and loaders to filter by. */
  modsVersionsPage: (project: string, query: ModsVersionsQuery): Promise<ModsVersionPage> =>
    invoke<ModsVersionPage>('mods_versions_page', { project, query }),
  /** How many projects the search finds in each category of its kind. */
  modsCategories: (query: ModsSearchQuery): Promise<ModsCategoryCount[]> =>
    invoke<ModsCategoryCount[]>('mods_categories', { query }),
  /** Up to five projects like this one. */
  modsSimilar: (project: string, game: string | null, loader: string | null): Promise<ModsHit[]> =>
    invoke<ModsHit[]>('mods_similar', { project, game, loader }),
  /** The catalog's collections; each lists with `modsSearch` and its `query`. */
  modsCollections: (
    kind: ModsKind,
    game: string | null,
    loader: string | null,
  ): Promise<ModsCollection[]> =>
    invoke<ModsCollection[]>('mods_collections', { kind, game, loader }),
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
  /** What adding a map as a new world will do. */
  modsWorldPlan: (payload: ModsWorldPayload): Promise<ModsWorldPlan> =>
    invoke<ModsWorldPlan>('mods_world_plan', { payload }),
  /** Adds a map as a new world; its folder name. */
  modsInstallWorld: (payload: ModsWorldPayload, requestId?: string): Promise<string> =>
    invoke<string>('mods_install_world', { request: mutation(payload, requestId) }),
  /** Picks an instance, game folder or mods folder (or a zip of one); null when closed. */
  modsImportPick: (folder: boolean, title: string): Promise<ModsImportFound | null> =>
    invoke<ModsImportFound | null>('mods_import_pick', { folder, title }),
  /** Makes the installation for what was picked. */
  modsImportCreate: (
    payload: ModsImportCreatePayload,
    requestId?: string,
  ): Promise<InstanceSummary> =>
    invoke<InstanceSummary>('mods_import_create', { request: mutation(payload, requestId) }),
  /** Copies what was picked into the installation made for it. */
  modsImportApply: (instanceId: string, requestId?: string): Promise<ModsImportReport> =>
    invoke<ModsImportReport>('mods_import_apply', {
      request: mutation({ instanceId }, requestId),
    }),
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
  /** What publishing an installation as a pack would take, and the pack it was published as. */
  modsPublishCheck: (instanceId: string, choice: ModsExportChoice): Promise<ModsPublishCheck> =>
    invoke<ModsPublishCheck>('mods_publish_check', { instanceId, choice }),
  /** Picks the pack's icon; `null` when the dialog was closed. */
  modsPublishIcon: (title?: string): Promise<{ preview: string } | null> =>
    invoke<{ preview: string } | null>('mods_publish_icon', { title: title ?? null }),
  /** Publishes an installation as the player's own pack, or a new version of it. */
  modsPublish: (payload: {
    instanceId: string;
    form: ModsPublishForm;
    icon: boolean;
    asNew: boolean;
  }): Promise<ModsPublished> => invoke<ModsPublished>('mods_publish', { payload }),
  /** Hides the player's own pack from the catalog, or shows it again; returns its status. */
  modsPackHidden: (project: string, hidden: boolean): Promise<ModsOwnPack['status']> =>
    invoke<ModsOwnPack['status']>('mods_pack_hidden', { project, hidden }),
  /** Reports a player's own pack to the catalog's owner. */
  modsReport: (project: string, reason: ModsReportReason, text: string): Promise<void> =>
    invoke('mods_report', { project, reason, text }),
  modsCancel: (instanceId: string): Promise<boolean> =>
    invoke<boolean>('mods_cancel', { instanceId }),
  modsContent: (instanceId: string): Promise<ContentView> =>
    invoke<ContentView>('mods_content', { instanceId }),
  /**
   * Mods, packs and shaders of every installation (the last played first)
   * whose title or file name has all the words, or that are catalog project
   * `project`; at most 100.
   */
  modsFindInstalled: (text: string, project: string | null): Promise<ModsInstalledMatch[]> =>
    invoke<ModsInstalledMatch[]>('mods_find_installed', { text, project }),
  /** What puts in or updates mod `modId` (a crash's `dependencyId`); null: nothing better. */
  modsFix: (instanceId: string, modId: string): Promise<ModsFix | null> =>
    invoke<ModsFix | null>('mods_fix', { instanceId, modId }),
  /** Files of the installation the catalog holds back as harmful. */
  modsHarmful: (instanceId: string): Promise<string[]> =>
    invoke<string[]>('mods_harmful', { instanceId }),
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
  /** The server ads of a place showing now (D-069). */
  feedServerAds: (place: 'feed', refresh = false): Promise<ServerAdsView> =>
    invoke<ServerAdsView>('feed_server_ads', { place, refresh }),
  /** Counts a view or a press in an ad's tile for the statistics (D-071). */
  feedServerAdEvent: (ad: string, event: AdEvent): Promise<void> =>
    invoke('feed_server_ad_event', { ad, event }),
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
  /** The channels the player hid from the feed and the ones they put first. */
  feedMarks: (): Promise<FeedMarks> => invoke<FeedMarks>('feed_marks'),
  /** Puts a channel on a list, taking it off the other, or (null) off both; the marks after. */
  feedMark: (channel: ChannelMark, list: MarkList | null): Promise<FeedMarks> =>
    invoke<FeedMarks>('feed_mark', { ...channel, list }),
  /** The newest videos and live streams of the favourite channels. */
  feedFavorites: (): Promise<FavoriteFeed> => invoke<FavoriteFeed>('feed_favorites'),
  /** The videos the player opened from the feed lately, oldest first. */
  feedWatched: (): Promise<string[]> => invoke<string[]>('feed_watched'),
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
  /**
   * Versions a world can be made for, newest first; snapshots and the versions
   * of our own list (D-073) too when asked.
   */
  worldsVersions: (snapshots = false, special = false): Promise<WorldsVersionChoice[]> =>
    invoke<WorldsVersionChoice[]>('worlds_versions', { snapshots, special }),
  worldsLocal: (): Promise<LocalWorld[]> => invoke<LocalWorld[]>('worlds_local'),
  worldsRooms: (): Promise<WorldsRoom[]> => invoke<WorldsRoom[]>('worlds_rooms'),
  worldsRoom: (room: string): Promise<WorldsRoomPage> =>
    invoke<WorldsRoomPage>('worlds_room', { room }),
  /** A world on this computer and its room. */
  worldsCreate: (payload: WorldsCreatePayload): Promise<WorldsRoom> =>
    invoke<WorldsRoom>('worlds_create', { payload }),
  /** Installations a world with mods can be made from. */
  worldsModSources: (): Promise<WorldsModSource[]> =>
    invoke<WorldsModSource[]>('worlds_mod_sources'),
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
  /**
   * The installation the room plays, made on first use. A guest of a world
   * with mods is asked first; `accept` is the room pack they agreed to.
   */
  worldsInstance: (room: string, accept?: string): Promise<WorldsInstance> =>
    invoke<WorldsInstance>('worlds_instance', { room, accept: accept ?? null }),
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
  /** What updating a world's mods from an installation would change (W12). */
  worldsUpdatePreview: (world: string, instanceId: string): Promise<WorldsUpdatePreview> =>
    invoke<WorldsUpdatePreview>('worlds_update_preview', { world, instanceId }),
  /**
   * Updates a stopped world's mods: backed up, tried on a copy, kept with the
   * way back. `choices` answers each changed setting by its path.
   */
  worldsUpdate: (payload: {
    room: string;
    world: string;
    instanceId: string;
    fingerprint: string;
    choices: Record<string, WorldsKeep>;
  }): Promise<void> => invoke('worlds_update', { payload }),
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
