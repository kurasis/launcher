import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import { BACKEND_EVENT_NAMES } from './contracts';
import { describeIpcError, ipc, isLauncherError, subscribeBackendEvents } from './ipc';

describe('describeIpcError', () => {
  it('uses the message of a typed backend error', () => {
    const error = { code: 'APP_BUSY', message: 'Busy.', retryable: true, diagnosticId: 'd' };
    expect(isLauncherError(error)).toBe(true);
    expect(describeIpcError(error)).toBe('Busy.');
  });

  it('passes through string rejections and Error messages', () => {
    expect(describeIpcError('not allowed')).toBe('not allowed');
    expect(describeIpcError(new Error('boom'))).toBe('boom');
  });

  it('falls back to a generic message for unknown values', () => {
    expect(describeIpcError(undefined)).toBe('The launcher core did not respond.');
    expect(describeIpcError({})).toBe('The launcher core did not respond.');
    expect(describeIpcError('')).toBe('The launcher core did not respond.');
  });
});

describe('mutating commands', () => {
  it('wrap payloads in a MutationRequest with a request id', async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      return null;
    });
    await ipc.createInstance({ displayName: 'A', versionId: '1.21.1' }, 'req-1');
    await ipc.setAppSettings({ theme: 'dark' });
    expect(calls[0]).toEqual({
      cmd: 'create_instance',
      args: { request: { requestId: 'req-1', payload: { displayName: 'A', versionId: '1.21.1' } } },
    });
    const second = calls[1]?.args as { request: { requestId: string } };
    expect(second.request.requestId).toMatch(/^[0-9a-f-]{36}$/);
  });
});

describe('subscribeBackendEvents', () => {
  it('registers one listener per event and removes them with the last subscriber', async () => {
    const commands: string[] = [];
    mockIPC((cmd) => {
      commands.push(cmd);
      return commands.length;
    });
    const first = subscribeBackendEvents(() => undefined);
    const second = subscribeBackendEvents(() => undefined);
    await Promise.all([first.ready, second.ready]);
    const listens = () => commands.filter((c) => c === 'plugin:event|listen').length;
    const unlistens = () => commands.filter((c) => c === 'plugin:event|unlisten').length;
    expect(listens()).toBe(BACKEND_EVENT_NAMES.length);

    first.unsubscribe();
    first.unsubscribe();
    await Promise.resolve();
    expect(unlistens()).toBe(0);

    second.unsubscribe();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(unlistens()).toBe(BACKEND_EVENT_NAMES.length);
  });
});
