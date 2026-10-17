import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { afterEach, describe, expect, it } from 'vitest';
import type { AppInfo, AppSnapshot, InstanceSummary, MutationRequest } from '../lib/contracts';
import { appInfoFixture, instanceFixture, snapshotFixture } from '../test/fixtures';
import { setLanguage } from '../lib/strings';
import { App } from './App';

// Test-only fake backend. Production code always talks to the real Rust core.
function fakeBackend(initial: AppSnapshot, info: AppInfo = appInfoFixture) {
  let snapshot = initial;
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC(
    (cmd, args) => {
      calls.push({ cmd, args });
      switch (cmd) {
        case 'get_app_snapshot':
          return snapshot;
        case 'list_supported_versions':
          return {
            versions: ['26.3', '1.21.1'].map((id) => ({
              id,
              versionType: 'release',
              releaseTime: null,
              verified: true,
              availability: 'available',
            })),
            fetchedAt: null,
            stale: false,
            error: null,
            latestRelease: '26.3',
            latestSnapshot: null,
          };
        case 'get_app_info':
          return info;
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
            template: 'Maximum memory must be between {min} and {max} MiB on this computer.',
            params: { min: 1024, max: 13108 },
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
  afterEach(() => {
    setLanguage('en');
  });

  it('shows the interface and core errors in the chosen language', async () => {
    const instance = instanceFixture();
    fakeBackend(
      snapshotFixture({
        instances: [instance],
        settings: {
          ...snapshotFixture().settings,
          language: 'ru',
          selectedInstanceId: instance.id,
        },
      }),
    );
    render(<App />);
    expect(await screen.findByRole('button', { name: 'Настройки' })).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Изменить настройки' }));
    fireEvent.change(await screen.findByLabelText('Максимум памяти (МиБ)'), {
      target: { value: '2048' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Сохранить' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Максимум памяти на этом компьютере — от 1024 до 13108 МиБ.',
    );
  });

  it('saves the language chosen in settings', async () => {
    const calls = fakeBackend(snapshotFixture());
    render(<App />);
    fireEvent.click(await screen.findByRole('button', { name: 'Settings' }));
    const picker = screen.getByRole('combobox', { name: 'Language' });
    expect(picker).toHaveValue('en');
    expect(within(picker).getByRole('option', { name: 'العربية' })).toBeInTheDocument();
    fireEvent.change(picker, { target: { value: 'ru' } });
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'set_app_settings')?.args).toMatchObject({
        request: { payload: { language: 'ru' } },
      });
    });
  });

  it('marks a debug build running with the test profile', async () => {
    fakeBackend(snapshotFixture(), { ...appInfoFixture, testProfile: 'TestPlayer' });
    render(<App />);
    expect(await screen.findByText('Test profile TestPlayer · demo mode')).toBeVisible();
    expect(screen.getByText('Development build')).toBeVisible();
  });

  it('labels only development builds', async () => {
    fakeBackend(snapshotFixture(), { ...appInfoFixture, buildProfile: 'release' });
    render(<App />);
    fireEvent.click(await screen.findByRole('button', { name: 'About' }));
    // The About view shows the same core info once it has loaded.
    expect(await screen.findByText('Release')).toBeVisible();
    expect(screen.queryByText(/build$/)).toBeNull();
  });

  it('shows no test profile mark normally', async () => {
    fakeBackend(snapshotFixture());
    render(<App />);
    expect(await screen.findByText('Development build')).toBeVisible();
    expect(screen.queryByText(/Test profile/)).toBeNull();
  });

  it('shows the empty state and the account status', async () => {
    fakeBackend(snapshotFixture());
    render(<App />);
    expect(screen.getByRole('heading', { level: 1, name: 'Sferum Launcher' })).toBeVisible();
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
    fireEvent.click(await screen.findByRole('button', { name: /^26\.3/ }));
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
    // Installation settings live on the Settings screen.
    expect(await screen.findByText('4096 MiB · 1280×720')).toBeVisible();
    expect(screen.queryByLabelText('Maximum memory (MiB)')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Change settings' }));
    expect(screen.getByRole('tab', { name: 'Installations' })).toHaveAttribute(
      'aria-selected',
      'true',
    );
    expect(screen.getByLabelText('Installation')).toHaveValue(instance.id);
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

  it('opens diagnostics and the project source link from the UI only', async () => {
    const calls = fakeBackend(snapshotFixture());
    render(<App />);
    await screen.findByText(/No installations yet/);
    fireEvent.click(screen.getByRole('button', { name: 'Diagnostics' }));
    expect(await screen.findByRole('region', { name: 'Export diagnostics' })).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'About' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Source code' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'open_external')?.args).toEqual({
        link: 'source_code',
      });
    });
    expect(screen.getByText(/License: not chosen yet/)).toBeVisible();
  });

  it('shows the running operation in the header on every screen', async () => {
    fakeBackend(
      snapshotFixture({
        instances: [instanceFixture({ busy: true })],
        activeOperation: { kind: 'install', instanceId: instanceFixture().id, operationId: 'op-1' },
        operationProgress: {
          sessionEpoch: 'epoch-1',
          operationId: 'op-1',
          instanceId: instanceFixture().id,
          kind: 'install',
          sequence: 3,
          revision: 0,
          phase: 'downloading',
          downloadedBytes: 450,
          totalDownloadBytes: 1000,
          verifiedFiles: 1,
          totalFiles: 10,
          bytesPerSecond: null,
          timestamp: '',
        },
      }),
    );
    render(<App />);
    const header = await screen.findByRole('banner');
    await waitFor(() => {
      expect(within(header).getByRole('status')).toHaveTextContent('Installing45%');
    });
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    expect(within(header).getByRole('status')).toHaveTextContent('Installing45%');
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
