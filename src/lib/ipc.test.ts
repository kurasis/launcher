import { describe, expect, it } from 'vitest';
import { describeIpcError } from './ipc';

describe('describeIpcError', () => {
  it('passes through string rejections from Rust commands', () => {
    expect(describeIpcError('APP_BUSY')).toBe('APP_BUSY');
  });

  it('uses Error messages', () => {
    expect(describeIpcError(new Error('boom'))).toBe('boom');
  });

  it('falls back to a generic message for unknown values', () => {
    expect(describeIpcError(undefined)).toBe('The launcher core did not respond.');
    expect(describeIpcError({})).toBe('The launcher core did not respond.');
    expect(describeIpcError('')).toBe('The launcher core did not respond.');
  });
});
