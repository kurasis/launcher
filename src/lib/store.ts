// Client-side mirror of the backend snapshot (docs/spec/06 "Bootstrap and ownership").
//
// On start the store subscribes to events first, buffers them, fetches the snapshot
// and then applies only newer revisions in order. An event from another backend
// epoch or a revision gap triggers a fresh snapshot. React never changes this state
// itself; it only renders it and sends commands.
import { useSyncExternalStore } from 'react';
import type { AppSnapshot, BackendEvent } from './contracts';
import { describeIpcError, ipc, subscribeBackendEvents } from './ipc';

export type Reconciliation =
  { kind: 'apply'; snapshot: AppSnapshot } | { kind: 'ignore' } | { kind: 'refetch' };

/** Pure reconciliation of one event against the current snapshot. */
export function reconcile(snapshot: AppSnapshot, event: BackendEvent): Reconciliation {
  const { payload } = event;
  if (payload.sessionEpoch !== snapshot.sessionEpoch) return { kind: 'refetch' };
  if (event.name === 'operation_progress') {
    // Progress carries the current revision without advancing it; apply it when
    // it is newer for its operation and the operation has not finished yet.
    const progress = event.payload;
    const current = snapshot.operationProgress;
    const finished = snapshot.recentOperations.some((r) => r.operationId === progress.operationId);
    const stale =
      current !== null &&
      current.operationId === progress.operationId &&
      current.sequence >= progress.sequence;
    if (finished || stale) return { kind: 'ignore' };
    return { kind: 'apply', snapshot: { ...snapshot, operationProgress: progress } };
  }
  if (payload.revision <= snapshot.revision) return { kind: 'ignore' };
  if (payload.revision !== snapshot.revision + 1) return { kind: 'refetch' };

  const base = { ...snapshot, revision: payload.revision };
  switch (event.name) {
    case 'instance_changed': {
      const incoming = event.payload.instance;
      const exists = snapshot.instances.some((i) => i.id === incoming.id);
      const instances = exists
        ? snapshot.instances.map((i) => (i.id === incoming.id ? incoming : i))
        : [...snapshot.instances, incoming];
      return { kind: 'apply', snapshot: { ...base, instances } };
    }
    case 'instance_removed': {
      const removed = event.payload.instanceId;
      const instances = snapshot.instances.filter((i) => i.id !== removed);
      return { kind: 'apply', snapshot: { ...base, instances } };
    }
    case 'settings_changed':
      return { kind: 'apply', snapshot: { ...base, settings: event.payload.settings } };
    case 'account_state_changed':
      return { kind: 'apply', snapshot: { ...base, account: event.payload.account } };
    case 'operation_finished': {
      const { result, activeOperation } = event.payload;
      const busyId = activeOperation?.instanceId ?? null;
      const instances = snapshot.instances.map((i) => ({ ...i, busy: i.id === busyId }));
      const operationProgress =
        snapshot.operationProgress?.operationId === result.operationId
          ? null
          : snapshot.operationProgress;
      const recentOperations = [
        result,
        ...snapshot.recentOperations.filter((r) => r.operationId !== result.operationId),
      ].slice(0, 10);
      return {
        kind: 'apply',
        snapshot: { ...base, activeOperation, instances, operationProgress, recentOperations },
      };
    }
    case 'game_state_changed': {
      const { currentRun, lastRun, activeOperation, queue, network, mirroredHosts, update } =
        event.payload;
      const busyId = activeOperation?.instanceId ?? null;
      const instances = snapshot.instances.map((i) => ({ ...i, busy: i.id === busyId }));
      return {
        kind: 'apply',
        snapshot: {
          ...base,
          currentRun,
          lastRun,
          activeOperation,
          queue,
          network,
          mirroredHosts,
          update,
          instances,
        },
      };
    }
  }
}

export type StoreState =
  | { status: 'loading' }
  | { status: 'error'; message: string }
  | { status: 'ready'; snapshot: AppSnapshot };

type Listener = () => void;

export class SnapshotStore {
  private state: StoreState = { status: 'loading' };
  private readonly listeners = new Set<Listener>();
  private buffered: BackendEvent[] = [];
  private fetching = false;
  private refetchQueued = false;
  private unsubscribeEvents: (() => void) | null = null;

  getState = (): StoreState => this.state;

  subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener);
    if (this.listeners.size === 1) this.start();
    return () => {
      this.listeners.delete(listener);
      if (this.listeners.size === 0) this.stop();
    };
  };

  /** Fetches a fresh snapshot, for example after a failed command. After a
   * failed start it subscribes to the events again first. */
  refresh = (): void => {
    if (this.unsubscribeEvents === null && this.listeners.size > 0) {
      this.start();
      return;
    }
    void this.fetchSnapshot();
  };

  private setState(state: StoreState): void {
    this.state = state;
    for (const listener of this.listeners) listener();
  }

  private start(): void {
    const subscription = subscribeBackendEvents((event) => {
      this.onEvent(event);
    });
    this.unsubscribeEvents = subscription.unsubscribe;
    void subscription.ready.then(
      () => this.fetchSnapshot(),
      (error: unknown) => {
        // Without events the store would go stale: Retry starts over.
        subscription.unsubscribe();
        if (this.unsubscribeEvents === subscription.unsubscribe) this.unsubscribeEvents = null;
        this.setState({ status: 'error', message: describeIpcError(error) });
      },
    );
  }

  private stop(): void {
    this.unsubscribeEvents?.();
    this.unsubscribeEvents = null;
    this.buffered = [];
    // A later subscriber bootstraps from a fresh snapshot.
    this.state = { status: 'loading' };
  }

  private onEvent(event: BackendEvent): void {
    if (this.fetching || this.state.status !== 'ready') {
      this.buffered.push(event);
      return;
    }
    this.apply(this.state.snapshot, event);
  }

  private apply(snapshot: AppSnapshot, event: BackendEvent): AppSnapshot | null {
    const result = reconcile(snapshot, event);
    if (result.kind === 'apply') {
      this.setState({ status: 'ready', snapshot: result.snapshot });
      return result.snapshot;
    }
    if (result.kind === 'refetch') {
      void this.fetchSnapshot();
      return null;
    }
    return snapshot;
  }

  private async fetchSnapshot(): Promise<void> {
    if (this.fetching) {
      this.refetchQueued = true;
      return;
    }
    this.fetching = true;
    try {
      const snapshot = await ipc.getAppSnapshot();
      this.fetching = false;
      this.setState({ status: 'ready', snapshot });
      const pending = this.buffered;
      this.buffered = [];
      let current: AppSnapshot | null = snapshot;
      for (const event of pending) {
        if (current === null) break;
        current = this.apply(current, event);
      }
    } catch (error) {
      this.fetching = false;
      if (this.state.status !== 'ready') {
        this.setState({ status: 'error', message: describeIpcError(error) });
      }
    }
    if (this.refetchQueued) {
      this.refetchQueued = false;
      await this.fetchSnapshot();
    }
  }
}

export const appStore = new SnapshotStore();

export function useAppStore(): StoreState {
  return useSyncExternalStore(appStore.subscribe, appStore.getState);
}
