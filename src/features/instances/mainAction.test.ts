import { describe, expect, it } from 'vitest';
import type { AppSnapshot, InstanceSummary, OperationResult } from '../../lib/contracts';
import { instanceFixture, snapshotFixture } from '../../test/fixtures';
import { mainActionOf } from './mainAction';

const ID = instanceFixture().id;

function of(instance: InstanceSummary, rest: Partial<AppSnapshot> = {}) {
  const action = mainActionOf(instance, snapshotFixture({ instances: [instance], ...rest }));
  return { kind: action.kind, label: action.label, enabled: action.enabled };
}

const otherBusy = { kind: 'install' as const, instanceId: 'other', operationId: 'op-9' };

function lastInstall(outcome: OperationResult['outcome']): OperationResult {
  return {
    operationId: 'op-0',
    instanceId: ID,
    kind: 'install',
    outcome,
    error: null,
    finishedAt: '2026-10-17T00:00:00Z',
    downloadedFiles: 1,
    networkBytes: 1,
    reusedFiles: 0,
  };
}

describe('mainActionOf', () => {
  it('plays an installed version, unless another operation runs or a start is pending', () => {
    const ready = instanceFixture({ health: 'ready' });
    expect(of(ready)).toEqual({ kind: 'play', label: 'Play', enabled: true });
    expect(of(ready, { activeOperation: otherBusy })).toEqual({
      kind: 'play',
      label: 'Play',
      enabled: false,
    });
    // Offline, installed versions still start.
    expect(of(ready, { network: 'offline' }).enabled).toBe(true);
    const snapshot = snapshotFixture({ instances: [ready] });
    expect(
      mainActionOf(ready, snapshot, { play: true, install: false, repair: false }),
    ).toMatchObject({ kind: 'play', label: 'Starting…', enabled: false, command: 'launch' });
  });

  it('installs, retries an unfinished install, and queues while busy or offline', () => {
    const fresh = instanceFixture();
    expect(of(fresh)).toEqual({ kind: 'install', label: 'Install', enabled: true });
    expect(of(fresh, { recentOperations: [lastInstall('cancelled')] })).toEqual({
      kind: 'retry',
      label: 'Retry install',
      enabled: true,
    });
    expect(of(fresh, { recentOperations: [lastInstall('succeeded')] }).kind).toBe('install');
    expect(of(fresh, { activeOperation: otherBusy })).toEqual({
      kind: 'queue',
      label: 'Add to queue',
      enabled: true,
    });
    expect(of(fresh, { network: 'offline' }).kind).toBe('queue');
    const queued = mainActionOf(
      fresh,
      snapshotFixture({
        instances: [fresh],
        queue: [
          { queueId: 'q', instanceId: ID, kind: 'install', queuedAt: '2026-10-17T00:00:00Z' },
        ],
      }),
    );
    expect(queued).toMatchObject({ kind: 'install', enabled: false, queued: true });
  });

  it('repairs damaged or unverified files, and has nothing for a newer launcher’s', () => {
    expect(of(instanceFixture({ health: 'repair_required' }))).toEqual({
      kind: 'repair',
      label: 'Repair',
      enabled: true,
    });
    expect(of(instanceFixture({ health: 'needs_verification' })).kind).toBe('repair');
    expect(of(instanceFixture({ health: 'repair_required' }), { network: 'offline' }).kind).toBe(
      'queue',
    );
    expect(of(instanceFixture({ health: 'unsupported' }))).toEqual({
      kind: 'none',
      label: '',
      enabled: false,
    });
  });

  it('shows the running game and its own operation as a status', () => {
    const ready = instanceFixture({ health: 'ready' });
    expect(
      of(ready, { activeOperation: { kind: 'launch', instanceId: ID, operationId: 'op-1' } }),
    ).toEqual({ kind: 'running', label: 'The game is running.', enabled: false });
    const running = {
      ...snapshotFixture().currentRun,
      instanceId: ID,
    } as AppSnapshot['currentRun'];
    expect(of(ready, { currentRun: running }).kind).toBe('running');
    const install = { kind: 'install' as const, instanceId: ID, operationId: 'op-2' };
    expect(of(instanceFixture(), { activeOperation: install })).toEqual({
      kind: 'busy',
      label: 'Installing',
      enabled: false,
    });
    const progress = {
      sessionEpoch: 'epoch-1',
      operationId: 'op-2',
      instanceId: ID,
      kind: 'install' as const,
      sequence: 1,
      revision: 1,
      phase: 'downloading' as const,
      downloadedBytes: 42,
      totalDownloadBytes: 100,
      verifiedFiles: 0,
      totalFiles: null,
      bytesPerSecond: null,
      currentFiles: [],
      approval: null,
      timestamp: '2026-10-17T00:00:00Z',
    };
    expect(
      of(instanceFixture(), { activeOperation: install, operationProgress: progress }).label,
    ).toBe('Installing 42%');
  });
});
