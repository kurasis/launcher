import { describe, expect, it } from 'vitest';
import type { OperationProgress } from './contracts';
import { progressPercent, toInt } from './format';

const base: OperationProgress = {
  sessionEpoch: 'e',
  operationId: 'op',
  instanceId: 'i',
  kind: 'install',
  sequence: 1,
  revision: 1,
  phase: 'downloading',
  downloadedBytes: 250,
  totalDownloadBytes: 1000,
  verifiedFiles: 9,
  totalFiles: 10,
  bytesPerSecond: null,
  currentFiles: [],
  approval: null,
  timestamp: '2026-10-19T00:00:00Z',
};

describe('progressPercent', () => {
  it('counts bytes while downloading and files while checking', () => {
    expect(progressPercent(base)).toBe(25);
    expect(progressPercent({ ...base, phase: 'checking_files' })).toBe(90);
  });

  it('is unknown without a total or progress', () => {
    expect(progressPercent(null)).toBeNull();
    expect(progressPercent({ ...base, totalDownloadBytes: null })).toBeNull();
    expect(progressPercent({ ...base, totalDownloadBytes: 0 })).toBeNull();
  });
});

describe('toInt', () => {
  it('reads whole numbers only', () => {
    expect(toInt(' 4096 ')).toBe(4096);
    expect(toInt('4.5')).toBeNaN();
    expect(toInt('')).toBeNaN();
  });
});
