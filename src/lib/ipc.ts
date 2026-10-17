// The single typed entry point for Tauri IPC. UI code must not call `invoke` or
// `listen` directly, so every command name, payload shape and event subscription is
// declared in one place.
import { invoke } from '@tauri-apps/api/core';
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
  type DataLocation,
  type ExportResult,
  type ExternalLink,
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
  type BackupScope,
  type JavaCheck,
  type NoTokenProfile,
  type ProfileRef,
  type SetInstanceJavaPayload,
  type SetInstanceNotesPayload,
  type SettingsView,
  type UpdateInstanceSettingsPayload,
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
  pickDataFolder: (): Promise<string | null> => invoke<string | null>('pick_data_folder'),
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
  checkJava: (instanceId: string, path: string): Promise<JavaCheck> =>
    invoke<JavaCheck>('check_java', { payload: { instanceId, path } }),
  pickJavaFile: (): Promise<string | null> => invoke<string | null>('pick_java_file'),
  setInstanceJava: (
    payload: SetInstanceJavaPayload,
    requestId?: string,
  ): Promise<InstanceSummary> =>
    invoke<InstanceSummary>('set_instance_java', { request: mutation(payload, requestId) }),
  setInstanceNotes: (
    payload: SetInstanceNotesPayload,
    requestId?: string,
  ): Promise<InstanceSummary> =>
    invoke<InstanceSummary>('set_instance_notes', { request: mutation(payload, requestId) }),
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
  openInstanceFolder: (instanceId: string, requestId?: string): Promise<void> =>
    invoke('open_instance_folder', { request: mutation({ instanceId }, requestId) }),
  exportDiagnostics: (runId: string | null, requestId?: string): Promise<ExportResult> =>
    invoke<ExportResult>('export_diagnostics', { request: mutation({ runId }, requestId) }),
  /** Plays as the installation's profile: a Microsoft account, or the demo (D-045). */
  launchGame: (instanceId: string, requestId?: string): Promise<RunSummary> =>
    invoke<RunSummary>('launch_game', { request: mutation({ instanceId }, requestId) }),
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
  /** Debug builds only; release builds have no handler for this command. */
  startLifecycleTest: (scenario: LifecycleScenario, requestId?: string): Promise<RunSummary> =>
    invoke<RunSummary>('start_lifecycle_test', { request: mutation(scenario, requestId) }),
  /** Debug builds only. */
  stopLifecycleTest: (runId: string): Promise<boolean> =>
    invoke<boolean>('stop_lifecycle_test', { runId }),
};

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
  let active = true;
  return {
    ready: current.then(() => undefined),
    unsubscribe: () => {
      if (!active) return;
      active = false;
      handlers.delete(handler);
      if (handlers.size === 0 && registration === current) {
        registration = null;
        void current.then((unlisteners) => {
          for (const unlisten of unlisteners) {
            // UnlistenFn returns a promise in Tauri 2. It rejects when the webview
            // is being torn down, in which case its listeners are gone already.
            void Promise.resolve()
              .then(unlisten)
              .catch(() => undefined);
          }
        });
      }
    },
  };
}

/** Fires when the operating system or app menu asked to quit during a run. */
export function onQuitRequested(handler: () => void): () => void {
  const pending = listen('quit_requested', () => {
    handler();
  });
  return () => {
    void pending.then((unlisten) => Promise.resolve().then(unlisten)).catch(() => undefined);
  };
}
