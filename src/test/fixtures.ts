// Synthetic, non-secret IPC fixtures for frontend tests only.
import type { AppInfo, AppSnapshot, InstanceSummary } from '../lib/contracts';

export const appInfoFixture: AppInfo = {
  productName: 'Kurasis Launcher',
  appVersion: '0.1.0',
  tauriVersion: '2.12.4',
  os: 'linux',
  arch: 'x86_64',
  buildProfile: 'debug',
  microsoftClientId: 'not_configured',
};

export function instanceFixture(overrides: Partial<InstanceSummary> = {}): InstanceSummary {
  return {
    id: '0f1e2d3c-4b5a-4978-8796-a5b4c3d2e1f0',
    displayName: 'Survival',
    versionId: '1.21.1',
    manifestSha1: null,
    createdAt: '2026-10-16T12:00:00Z',
    lastPlayedAt: null,
    maxHeapMib: 4096,
    resolution: { width: 1280, height: 720 },
    health: 'not_installed',
    lastResult: null,
    busy: false,
    ...overrides,
  };
}

export function snapshotFixture(overrides: Partial<AppSnapshot> = {}): AppSnapshot {
  return {
    apiSchemaVersion: 1,
    sessionEpoch: 'epoch-1',
    revision: 0,
    appVersion: '0.1.0',
    dataRoot: { status: 'ready', path: '/home/test/.local/share/launcher', error: null },
    settings: {
      theme: 'system',
      duringGameMode: 'unload_and_return',
      selectedInstanceId: null,
      defaultMaxHeapMib: null,
      defaultResolution: { width: 1280, height: 720 },
      writable: true,
    },
    account: {
      state: 'signed_out',
      profileName: null,
      profileId: null,
      error: null,
      sessionOnly: false,
      loginOperationId: null,
    },
    microsoftClientId: 'not_configured',
    instances: [],
    activeOperation: null,
    currentRun: null,
    lastRun: null,
    lifecycleHarnessAvailable: true,
    supportedVersions: [
      {
        id: '1.21.1',
        status: 'pending_verification',
        expectedJavaMajor: 21,
        acceptedDescriptorSha1: ['499680d659b7a0ccb2fa9a809c345c5baf2ace7d'],
      },
      {
        id: '26.3',
        status: 'pending_verification',
        expectedJavaMajor: 25,
        acceptedDescriptorSha1: ['11f1aefe473eefba0af62e3286c33b692807b2eb'],
      },
    ],
    memory: {
      policy: {
        physicalMib: 16384,
        minHeapMib: 1024,
        maxHeapMib: 13108,
        recommendedHeapMib: 4096,
        lowMemory: false,
      },
      error: null,
      resolution: { minWidth: 640, maxWidth: 7680, minHeight: 480, maxHeight: 4320 },
    },
    storageIssues: [],
    operationProgress: null,
    recentOperations: [],
    ...overrides,
  };
}
