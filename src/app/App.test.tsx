import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import type { AppSnapshot, InstanceSummary, MutationRequest } from '../lib/contracts';
import { appInfoFixture, instanceFixture, snapshotFixture } from '../test/fixtures';
import { App } from './App';

// Test-only fake backend. Production code always talks to the real Rust core.
function fakeBackend(initial: AppSnapshot) {
  let snapshot = initial;
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC(
    (cmd, args) => {
      calls.push({ cmd, args });
      switch (cmd) {
        case 'get_app_snapshot':
          return snapshot;
        case 'get_app_info':
          return appInfoFixture;
        case 'create_instance': {
          const { request } = args as {
            request: MutationRequest<{ displayName: string; versionId: string }>;
          };
          const created: InstanceSummary = instanceFixture({
            id: '11111111-2222-4333-8444-555555555555',
            displayName: request.payload.displayName.trim(),
            versionId: request.payload.versionId,
          });
          snapshot = {
            ...snapshot,
            revision: snapshot.revision + 2,
            instances: [...snapshot.instances, created],
            settings: { ...snapshot.settings, selectedInstanceId: created.id },
          };
          return created;
        }
        case 'update_instance_settings':
          // The Rust core rejects with a plain error envelope, not an Error instance.
          // eslint-disable-next-line @typescript-eslint/only-throw-error
          throw {
            code: 'INVALID_INPUT',
            message: 'Maximum memory must be between 1024 and 13108 MiB on this computer.',
            retryable: false,
            diagnosticId: 'diag-1',
          };
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );
  return calls;
}

describe('App', () => {
  it('shows the empty state and the account status', async () => {
    fakeBackend(snapshotFixture());
    render(<App />);
    expect(screen.getByRole('heading', { level: 1, name: 'Kurasis Launcher' })).toBeVisible();
    expect(await screen.findByText(/No installations yet/)).toBeVisible();
    expect(screen.getByText('Not signed in')).toBeVisible();
    expect(screen.queryByRole('button', { name: /play/i })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /sign in/i })).not.toBeInTheDocument();
  });

  it('creates an installation through create_instance with a request id', async () => {
    const calls = fakeBackend(snapshotFixture());
    render(<App />);
    fireEvent.click(await screen.findByRole('button', { name: 'New installation' }));
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: '  Creative  ' } });
    fireEvent.change(screen.getByLabelText('Minecraft version'), { target: { value: '26.3' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create installation' }));

    const item = await screen.findByRole('button', { name: /Creative/ });
    expect(item).toHaveAttribute('aria-current', 'true');
    expect(within(item).getByText('Minecraft 26.3')).toBeVisible();
    const create = calls.find((c) => c.cmd === 'create_instance');
    expect(create?.args).toMatchObject({
      request: { payload: { displayName: '  Creative  ', versionId: '26.3' } },
    });
    expect((create?.args as { request: { requestId: string } }).request.requestId).toMatch(
      /^[0-9a-f-]{36}$/,
    );
  });

  it('shows backend validation errors for instance settings', async () => {
    const instance = instanceFixture();
    fakeBackend(
      snapshotFixture({
        instances: [instance],
        settings: { ...snapshotFixture().settings, selectedInstanceId: instance.id },
      }),
    );
    render(<App />);
    const memory = await screen.findByLabelText('Maximum memory (MiB)');
    expect(memory).toHaveValue(4096);
    expect(screen.getByText(/Between 1024 and 13108 MiB/)).toBeVisible();
    fireEvent.change(memory, { target: { value: '2048' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Maximum memory must be between');
  });

  it('explains an unavailable data directory', async () => {
    fakeBackend(
      snapshotFixture({
        dataRoot: {
          status: 'unavailable',
          path: null,
          error: {
            code: 'DATA_ROOT_LOCKED',
            message: 'Another launcher process is already using the launcher data directory.',
            retryable: false,
            diagnosticId: 'd',
          },
        },
      }),
    );
    render(<App />);
    expect(await screen.findByRole('alert')).toHaveTextContent('Another launcher process');
  });

  it('navigates to settings and about', async () => {
    fakeBackend(snapshotFixture());
    render(<App />);
    await screen.findByText(/No installations yet/);
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    expect(screen.getByRole('button', { name: 'Settings' })).toHaveAttribute(
      'aria-current',
      'page',
    );
    expect(screen.getByRole('radio', { name: 'Match system' })).toBeChecked();
    expect(screen.getByText('/home/test/.local/share/launcher')).toBeVisible();

    fireEvent.click(screen.getByRole('button', { name: 'About' }));
    const setup = await screen.findByRole('region', { name: 'Microsoft sign-in setup' });
    expect(setup).toHaveTextContent('Not configured');
    expect(screen.getByText('linux / x86_64')).toBeVisible();
  });

  it('applies the theme chosen in settings', async () => {
    fakeBackend(snapshotFixture({ settings: { ...snapshotFixture().settings, theme: 'dark' } }));
    render(<App />);
    await waitFor(() => {
      expect(document.documentElement.dataset.theme).toBe('dark');
    });
    await act(async () => {
      await Promise.resolve();
    });
  });

  it('always shows the independent-project notice', () => {
    fakeBackend(snapshotFixture());
    render(<App />);
    expect(screen.getByRole('contentinfo')).toHaveTextContent(
      'not affiliated with or endorsed by Mojang Studios or Microsoft',
    );
  });
});
