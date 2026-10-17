import { emit } from '@tauri-apps/api/event';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import { instanceFixture, snapshotFixture } from '../test/fixtures';
import type { BackendEvent } from './contracts';
import { reconcile, SnapshotStore, type StoreState } from './store';

function instanceEvent(revision: number, epoch = 'epoch-1', name = 'A'): BackendEvent {
  return {
    name: 'instance_changed',
    payload: {
      sessionEpoch: epoch,
      revision,
      timestamp: '2026-10-16T12:00:00Z',
      instance: instanceFixture({ displayName: name }),
    },
  };
}

describe('reconcile', () => {
  const base = snapshotFixture({ revision: 4 });

  it('applies the next revision', () => {
    const result = reconcile(base, instanceEvent(5));
    expect(result.kind).toBe('apply');
    if (result.kind === 'apply') {
      expect(result.snapshot.revision).toBe(5);
      expect(result.snapshot.instances).toHaveLength(1);
    }
  });

  it('replaces an existing instance instead of duplicating it', () => {
    const withInstance = snapshotFixture({ revision: 4, instances: [instanceFixture()] });
    const result = reconcile(withInstance, instanceEvent(5, 'epoch-1', 'Renamed'));
    expect(result.kind === 'apply' && result.snapshot.instances).toEqual([
      instanceFixture({ displayName: 'Renamed' }),
    ]);
  });

  it('ignores revisions already represented', () => {
    expect(reconcile(base, instanceEvent(4)).kind).toBe('ignore');
    expect(reconcile(base, instanceEvent(1)).kind).toBe('ignore');
  });

  it('applies newer progress without advancing the revision', () => {
    const progress = (sequence: number, operationId = 'op-1') => ({
      name: 'operation_progress' as const,
      payload: {
        sessionEpoch: 'epoch-1',
        operationId,
        instanceId: null,
        kind: 'install' as const,
        sequence,
        revision: 4,
        phase: 'downloading' as const,
        downloadedBytes: sequence * 10,
        totalDownloadBytes: 100,
        verifiedFiles: 0,
        totalFiles: 5,
        bytesPerSecond: null,
        timestamp: '',
      },
    });
    const first = reconcile(base, progress(2));
    expect(first.kind).toBe('apply');
    if (first.kind !== 'apply') return;
    expect(first.snapshot.revision).toBe(4);
    expect(reconcile(first.snapshot, progress(1)).kind).toBe('ignore');
    expect(reconcile(first.snapshot, progress(3)).kind).toBe('apply');

    // After the terminal event, late progress for that operation is dropped.
    const finished = reconcile(first.snapshot, {
      name: 'operation_finished',
      payload: {
        sessionEpoch: 'epoch-1',
        revision: 5,
        timestamp: '',
        activeOperation: null,
        result: {
          operationId: 'op-1',
          instanceId: null,
          kind: 'install',
          outcome: 'succeeded',
          error: null,
          finishedAt: '',
          downloadedFiles: 5,
          networkBytes: 100,
          reusedFiles: 0,
        },
      },
    });
    expect(finished.kind).toBe('apply');
    if (finished.kind !== 'apply') return;
    expect(finished.snapshot.operationProgress).toBeNull();
    expect(finished.snapshot.recentOperations[0]?.outcome).toBe('succeeded');
    expect(reconcile(finished.snapshot, progress(9)).kind).toBe('ignore');
  });

  it('refetches on a gap or another backend epoch', () => {
    expect(reconcile(base, instanceEvent(7)).kind).toBe('refetch');
    expect(reconcile(base, instanceEvent(5, 'epoch-2')).kind).toBe('refetch');
  });

  it('applies run state and marks the busy instance', () => {
    const instance = instanceFixture();
    const withInstance = snapshotFixture({ revision: 4, instances: [instance] });
    const result = reconcile(withInstance, {
      name: 'game_state_changed',
      payload: {
        sessionEpoch: 'epoch-1',
        revision: 5,
        timestamp: '',
        currentRun: null,
        lastRun: null,
        activeOperation: { kind: 'install', instanceId: instance.id, operationId: 'op' },
      },
    });
    expect(result.kind === 'apply' && result.snapshot.instances[0]?.busy).toBe(true);
    expect(result.kind === 'apply' && result.snapshot.activeOperation?.operationId).toBe('op');
  });

  it('applies settings changes', () => {
    const settings = { ...base.settings, theme: 'dark' as const };
    const result = reconcile(base, {
      name: 'settings_changed',
      payload: { sessionEpoch: 'epoch-1', revision: 5, timestamp: '', settings },
    });
    expect(result.kind === 'apply' && result.snapshot.settings.theme).toBe('dark');
  });
});

describe('SnapshotStore', () => {
  function waitFor(store: SnapshotStore, predicate: (s: StoreState) => boolean): Promise<void> {
    return new Promise((resolve) => {
      const check = () => {
        if (predicate(store.getState())) {
          unsubscribe();
          resolve();
        }
      };
      const unsubscribe = store.subscribe(check);
      check();
    });
  }

  it('bootstraps, applies live events and refetches after a gap', async () => {
    let snapshotCalls = 0;
    mockIPC(
      (cmd) => {
        if (cmd === 'get_app_snapshot') {
          snapshotCalls += 1;
          return snapshotFixture({ revision: snapshotCalls === 1 ? 0 : 9 });
        }
        return null;
      },
      { shouldMockEvents: true },
    );
    const store = new SnapshotStore();
    const stop = store.subscribe(() => undefined);
    await waitFor(store, (s) => s.status === 'ready');

    await emit('instance_changed', instanceEvent(1).payload);
    await waitFor(store, (s) => s.status === 'ready' && s.snapshot.revision === 1);

    await emit('instance_changed', instanceEvent(5).payload);
    await waitFor(store, (s) => s.status === 'ready' && s.snapshot.revision === 9);
    expect(snapshotCalls).toBe(2);
    stop();
  });

  it('reports an unreachable core', async () => {
    mockIPC(
      (cmd) => {
        if (cmd === 'get_app_snapshot') throw new Error('core down');
        return null;
      },
      { shouldMockEvents: true },
    );
    const store = new SnapshotStore();
    const stop = store.subscribe(() => undefined);
    await waitFor(store, (s) => s.status === 'error');
    expect(store.getState()).toEqual({ status: 'error', message: 'core down' });
    stop();
  });
});
