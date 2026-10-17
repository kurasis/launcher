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
  type ExportResult,
  type ExternalLink,
  type InstanceSummary,
  type LauncherError,
  type LifecycleScenario,
  type MutationRequest,
  type OperationStarted,
  type PlanSummary,
  type QuitAction,
  type QuitResult,
  type RunSummary,
  type SettingsPatch,
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
  signOut: (requestId?: string): Promise<void> =>
    invoke('sign_out', { request: mutation({}, requestId) }),
  verifyAccount: (requestId?: string): Promise<void> =>
    invoke('verify_account', { request: mutation({}, requestId) }),
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
  openInstanceFolder: (instanceId: string, requestId?: string): Promise<void> =>
    invoke('open_instance_folder', { request: mutation({ instanceId }, requestId) }),
  exportDiagnostics: (runId: string | null, requestId?: string): Promise<ExportResult> =>
    invoke<ExportResult>('export_diagnostics', { request: mutation({ runId }, requestId) }),
  /** `demo` starts Minecraft's demo without an account (D-033). */
  launchGame: (instanceId: string, requestId?: string, demo = false): Promise<RunSummary> =>
    invoke<RunSummary>('launch_game', { request: mutation({ instanceId, demo }, requestId) }),
  stopGame: (runId: string, requestId?: string): Promise<boolean> =>
    invoke<boolean>('stop_game', { request: mutation({ runId }, requestId) }),
  listSupportedVersions: (refresh: boolean): Promise<CatalogView> =>
    invoke<CatalogView>('list_supported_versions', { refresh }),
  previewInstallPlan: (versionId: string): Promise<PlanSummary> =>
    invoke<PlanSummary>('preview_install_plan', { versionId }),
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
