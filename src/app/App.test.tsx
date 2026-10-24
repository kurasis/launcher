import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { afterEach, describe, expect, it } from 'vitest';
import type { AppInfo, AppSnapshot, InstanceSummary, MutationRequest } from '../lib/contracts';
import { appInfoFixture, instanceFixture, snapshotFixture } from '../test/fixtures';
import { setLanguage } from '../lib/strings';
import { App } from './App';

// Test-only fake backend. Production code always talks to the real Rust core.
function fakeBackend(
  initial: AppSnapshot,
  info: AppInfo = appInfoFixture,
  overrides: Record<string, () => unknown> = {},
) {
  let snapshot = initial;
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC(
    (cmd, args) => {
      calls.push({ cmd, args });
      const override = overrides[cmd];
      if (override !== undefined) return override();
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
        case 'get_data_location':
          return {
            mode: 'default',
            path: '/home/test/.local/share/launcher',
            defaultPath: '/home/test/.local/share/launcher',
            portablePath: '/opt/launcher/launcher-data',
            pendingPath: null,
            pendingReused: false,
          };
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
        case 'feed_marks':
          return { hidden: [], favorite: [] };
        case 'feed_favorites':
          return { videos: [], streams: [] };
        case 'feed_watched':
          return [];
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );
  return calls;
}

/** The More menu button, whatever screen it names. */
function moreButton() {
  return screen.getByRole('button', { name: /^More/ });
}

/** Opens the More menu with a click and chooses `name`. */
function chooseFromMore(name: string) {
  fireEvent.click(moreButton());
  fireEvent.click(within(screen.getByRole('menu')).getByRole('menuitem', { name }));
}

/** The element keys go to. */
function focused(): Element {
  const element = document.activeElement;
  if (element === null) throw new Error('Nothing has focus.');
  return element;
}

function settingsWith(settings: Partial<AppSnapshot['settings']>) {
  return { ...snapshotFixture().settings, ...settings };
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
    expect(
      await screen.findByRole('button', { name: 'Настройки' }, { timeout: 5000 }),
    ).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Настройки установки' }));
    fireEvent.change(await screen.findByLabelText('Максимум памяти (МиБ)'), {
      target: { value: '2048' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Сохранить' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Максимум памяти на этом компьютере — от 1024 до 13108 МиБ.',
    );
  });

  it('says when it is offline and checks again on request', async () => {
    const calls = fakeBackend(snapshotFixture({ network: 'offline' }));
    render(<App />);
    expect(await screen.findByText('Offline')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Check again' }));
    await waitFor(() => {
      expect(calls.some((c) => c.cmd === 'check_network')).toBe(true);
    });
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
    await screen.findByText(/No installations yet/);
    chooseFromMore('About');
    // The About view shows the same core info once it has loaded.
    expect(await screen.findByText('Release')).toBeVisible();
    expect(screen.getByText('Version 0.1.0, release build')).toBeVisible();
    expect(screen.queryByText('Development build')).toBeNull();
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
    expect(screen.getByText('Player')).toBeVisible();
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
    // An installation's settings open in place of its details, on the same screen.
    expect(await screen.findByText('4096 MiB')).toBeVisible();
    expect(screen.getByText('1280×720')).toBeVisible();
    expect(screen.queryByLabelText('Maximum memory (MiB)')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Installation settings' }));
    expect(
      screen.getByRole('heading', { level: 3, name: /^Installation settings: / }),
    ).toBeInTheDocument();
    expect(screen.queryByRole('tab', { name: 'Installations' })).toBeNull();
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

  it('keeps Quit in the rail when the core cannot be reached', async () => {
    fakeBackend(snapshotFixture(), appInfoFixture, {
      get_app_snapshot: () => {
        // eslint-disable-next-line @typescript-eslint/only-throw-error -- mirrors the IPC error envelope
        throw { code: 'INTERNAL', message: 'Core failed.', retryable: true, diagnosticId: 'd' };
      },
    });
    render(<App />);
    expect(await screen.findByRole('button', { name: 'Try again' })).toBeVisible();
    expect(screen.getByRole('button', { name: 'Quit' })).toBeVisible();
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
    expect(await screen.findByText('/home/test/.local/share/launcher')).toBeVisible();
    expect(screen.getByRole('button', { name: 'Make portable' })).toBeEnabled();

    chooseFromMore('About');
    expect(screen.getByRole('button', { name: 'More: About' })).toHaveAttribute(
      'aria-current',
      'page',
    );
    expect(screen.getByRole('button', { name: 'Settings' })).not.toHaveAttribute('aria-current');
    expect(await screen.findByText('linux / x86_64')).toBeVisible();
    expect(screen.queryByRole('region', { name: 'Microsoft sign-in setup' })).toBeNull();
    // The bundled fonts show their OFL notice.
    fireEvent.click(screen.getByText(/^Fonts: Golos Text and Noto Sans Arabic/));
    expect(screen.getByText(/Copyright 2019 The Golos Text Project Authors/)).toBeVisible();
    expect(screen.getByText(/Copyright 2022 The Noto Project Authors/)).toBeVisible();
  });

  it('opens diagnostics and the project source link from the UI only', async () => {
    const calls = fakeBackend(snapshotFixture());
    render(<App />);
    await screen.findByText(/No installations yet/);
    chooseFromMore('Diagnostics');
    expect(await screen.findByRole('region', { name: 'Export diagnostics' })).toBeVisible();
    chooseFromMore('About');
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
          currentFiles: [],
          approval: null,
          timestamp: '',
        },
      }),
    );
    render(<App />);
    // The Downloads row and the launch bar carry it; no separate chip in the rail.
    const header = await screen.findByRole('banner');
    expect(await within(header).findByRole('button', { name: 'Downloads' })).toHaveTextContent(
      'Downloads 45%',
    );
    const bar = screen.getByRole('region', { name: 'Launch' });
    expect(within(bar).getByRole('status')).toHaveTextContent('Installation: downloading');
    expect(within(bar).getByRole('progressbar', { name: 'Download progress' })).toHaveAttribute(
      'value',
      '45',
    );
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    expect(within(header).getByRole('button', { name: 'Downloads' })).toHaveTextContent(
      'Downloads 45%',
    );
    expect(within(header).queryByText('Installing')).toBeNull();
  });

  it('asks before a large download on a metered connection, by itself', async () => {
    fakeBackend(
      snapshotFixture({
        instances: [instanceFixture({ busy: true })],
        activeOperation: { kind: 'install', instanceId: instanceFixture().id, operationId: 'op-m' },
        operationProgress: {
          sessionEpoch: 'epoch-1',
          operationId: 'op-m',
          instanceId: instanceFixture().id,
          kind: 'install',
          sequence: 1,
          revision: 0,
          phase: 'awaiting_approval',
          downloadedBytes: 0,
          totalDownloadBytes: null,
          verifiedFiles: 0,
          totalFiles: null,
          bytesPerSecond: null,
          currentFiles: [],
          approval: {
            bytes: 550 * 1024 * 1024,
            files: 3955,
            reason: 'large',
            monthBytes: 0,
            monthlyLimitMib: 0,
          },
          timestamp: '',
        },
      }),
    );
    render(<App />);
    const dialog = await screen.findByRole('dialog', { name: 'Download 550.0 MiB?' });
    expect(within(dialog).getByRole('button', { name: 'Download' })).toBeVisible();
    expect(dialog).toHaveFocus();
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

  it('shows the main tabs in the rail and the other screens in the More menu', async () => {
    fakeBackend(snapshotFixture());
    render(<App />);
    await screen.findByText(/No installations yet/);
    const nav = screen.getByRole('navigation', { name: 'Main' });
    const tabs = within(nav)
      .getAllByRole('button')
      .map((button) => button.textContent);
    expect(tabs).toEqual([
      'Feed',
      'Installations',
      'Mods',
      'Worlds',
      'Skins',
      'Accounts',
      'Settings',
    ]);
    const tools = screen.getByRole('navigation', { name: 'Friends, downloads and more' });
    expect(within(tools).getByRole('button', { name: /^More/ })).toBeVisible();
    expect(screen.getByRole('button', { name: 'Quit' })).toBeVisible();
    const more = moreButton();
    expect(more).toHaveAttribute('aria-haspopup', 'menu');
    expect(more).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByRole('menu')).toBeNull();

    fireEvent.click(more);
    expect(more).toHaveAttribute('aria-expanded', 'true');
    const menu = screen.getByRole('menu', { name: 'More' });
    expect(more).toHaveAttribute('aria-controls', menu.id);
    expect(
      Array.from(menu.children, (item) =>
        item.getAttribute('role') === 'separator' ? '-' : item.textContent,
      ),
    ).toEqual([
      'Backups',
      'Screenshots',
      '-',
      'Diagnostics',
      'Console',
      '-',
      'Leave a review',
      'About',
    ]);
    expect(within(menu).getByRole('menuitem', { name: 'Backups' })).toHaveFocus();
    // A second click closes it.
    fireEvent.click(more);
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('moves through the More menu with the keyboard', async () => {
    fakeBackend(snapshotFixture());
    render(<App />);
    await screen.findByText(/No installations yet/);
    const more = moreButton();
    more.focus();
    fireEvent.keyDown(more, { key: 'ArrowDown' });
    const item = (name: string) => screen.getByRole('menuitem', { name });
    expect(item('Backups')).toHaveFocus();
    fireEvent.keyDown(focused(), { key: 'ArrowDown' });
    expect(item('Screenshots')).toHaveFocus();
    // The separator is skipped.
    fireEvent.keyDown(focused(), { key: 'ArrowDown' });
    expect(item('Diagnostics')).toHaveFocus();
    fireEvent.keyDown(focused(), { key: 'End' });
    expect(item('About')).toHaveFocus();
    fireEvent.keyDown(focused(), { key: 'ArrowDown' });
    expect(item('Backups')).toHaveFocus();
    fireEvent.keyDown(focused(), { key: 'ArrowUp' });
    expect(item('About')).toHaveFocus();
    fireEvent.keyDown(focused(), { key: 'Home' });
    expect(item('Backups')).toHaveFocus();
    fireEvent.keyDown(focused(), { key: 'ArrowDown' });
    fireEvent.keyDown(focused(), { key: 'Enter' });

    expect(screen.queryByRole('menu')).toBeNull();
    const current = screen.getByRole('button', { name: 'More: Screenshots' });
    expect(current).toHaveFocus();
    expect(current).toHaveAttribute('aria-current', 'page');
    expect(current).toHaveAttribute('aria-expanded', 'false');
    expect(screen.getByRole('button', { name: 'Installations' })).not.toHaveAttribute(
      'aria-current',
    );

    // ArrowUp opens on the last item, Space chooses.
    fireEvent.keyDown(current, { key: 'ArrowUp' });
    expect(item('About')).toHaveFocus();
    expect(item('Screenshots')).toHaveAttribute('aria-current', 'page');
    fireEvent.keyDown(focused(), { key: ' ' });
    expect(screen.getByRole('button', { name: 'More: About' })).toHaveFocus();
    expect(await screen.findByText('linux / x86_64')).toBeVisible();

    // Enter and Space on the button open the menu too.
    fireEvent.keyDown(moreButton(), { key: 'Enter' });
    expect(item('Backups')).toHaveFocus();
    fireEvent.keyDown(focused(), { key: 'Escape' });
    fireEvent.keyDown(moreButton(), { key: ' ' });
    expect(item('Backups')).toHaveFocus();

    // A row tab takes the current mark back from More.
    fireEvent.keyDown(focused(), { key: 'Escape' });
    fireEvent.click(screen.getByRole('button', { name: 'Accounts' }));
    expect(moreButton()).toHaveAccessibleName('More');
    expect(moreButton()).not.toHaveAttribute('aria-current');
  });

  it('closes the More menu with Escape, Tab or a click elsewhere', async () => {
    fakeBackend(snapshotFixture());
    render(<App />);
    await screen.findByText(/No installations yet/);
    const more = moreButton();

    fireEvent.keyDown(more, { key: 'ArrowDown' });
    fireEvent.keyDown(screen.getByRole('menuitem', { name: 'Backups' }), { key: 'ArrowDown' });
    fireEvent.keyDown(focused(), { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();
    expect(more).toHaveFocus();
    expect(more).toHaveAttribute('aria-expanded', 'false');
    // Nothing was chosen.
    expect(screen.getByRole('button', { name: 'Installations' })).toHaveAttribute(
      'aria-current',
      'page',
    );

    fireEvent.click(more);
    expect(screen.getByRole('menu')).toBeVisible();
    fireEvent.pointerDown(screen.getByRole('contentinfo'));
    expect(screen.queryByRole('menu')).toBeNull();
    expect(more).toHaveFocus();

    fireEvent.click(more);
    fireEvent.keyDown(focused(), { key: 'Tab' });
    expect(screen.queryByRole('menu')).toBeNull();
    expect(more).toHaveFocus();

    // A press inside the menu does not close it.
    fireEvent.click(more);
    fireEvent.pointerDown(screen.getByRole('menuitem', { name: 'Diagnostics' }));
    expect(screen.getByRole('menu')).toBeVisible();
  });

  it('opens the console window from the More menu', async () => {
    let fail = false;
    const calls = fakeBackend(snapshotFixture(), appInfoFixture, {
      open_console: () => {
        if (fail) {
          // The Rust core rejects with a plain error envelope.
          // eslint-disable-next-line @typescript-eslint/only-throw-error
          throw {
            code: 'INTERNAL',
            message: 'The console window could not be opened.',
            retryable: false,
            diagnosticId: 'diag-2',
          };
        }
        return null;
      },
    });
    render(<App />);
    await screen.findByText(/No installations yet/);
    chooseFromMore('Console');
    await waitFor(() => {
      expect(calls.filter((c) => c.cmd === 'open_console')).toHaveLength(1);
    });
    // The console is a window, not a screen: the current screen stays.
    expect(moreButton()).toHaveAccessibleName('More');
    expect(screen.getByRole('button', { name: 'Installations' })).toHaveAttribute(
      'aria-current',
      'page',
    );

    fail = true;
    fireEvent.keyDown(moreButton(), { key: 'ArrowUp' });
    fireEvent.keyDown(focused(), { key: 'ArrowUp' });
    fireEvent.keyDown(focused(), { key: 'ArrowUp' });
    expect(screen.getByRole('menuitem', { name: 'Console' })).toHaveFocus();
    fireEvent.keyDown(focused(), { key: 'Enter' });
    expect(await screen.findByText('The console window could not be opened.')).toBeVisible();
    expect(calls.filter((c) => c.cmd === 'open_console')).toHaveLength(2);
  });

  it('opens the start screen chosen in settings', async () => {
    fakeBackend(snapshotFixture({ settings: settingsWith({ startView: 'account' }) }));
    render(<App />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Accounts' })).toHaveAttribute(
        'aria-current',
        'page',
      );
    });
    expect(screen.queryByText(/No installations yet/)).toBeNull();
  });

  it('opens the last screen when asked to, and remembers the next one', async () => {
    const calls = fakeBackend(
      snapshotFixture({ settings: settingsWith({ startView: 'last', lastView: 'account' }) }),
    );
    render(<App />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Accounts' })).toHaveAttribute(
        'aria-current',
        'page',
      );
    });
    fireEvent.click(screen.getByRole('button', { name: 'Installations' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'set_app_settings')?.args).toMatchObject({
        request: { payload: { lastView: 'home' } },
      });
    });
    // Settings and the More menu's screens are never remembered.
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    chooseFromMore('Backups');
    chooseFromMore('Diagnostics');
    await act(async () => {
      await Promise.resolve();
    });
    expect(calls.filter((c) => c.cmd === 'set_app_settings')).toHaveLength(1);
  });

  it('remembers nothing unless the last screen is the start screen', async () => {
    const calls = fakeBackend(snapshotFixture({ settings: settingsWith({ startView: 'home' }) }));
    render(<App />);
    await screen.findByText(/No installations yet/);
    fireEvent.click(screen.getByRole('button', { name: 'Accounts' }));
    await act(async () => {
      await Promise.resolve();
    });
    expect(calls.some((c) => c.cmd === 'set_app_settings')).toBe(false);
  });

  it('opens the feed at start, and hides it while it is switched off', async () => {
    const calls = fakeBackend(snapshotFixture({ settings: settingsWith({ startView: 'feed' }) }));
    const { unmount } = render(<App />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Feed' })).toHaveAttribute('aria-current', 'page');
    });
    await waitFor(() => {
      expect(calls.some((c) => c.cmd === 'feed_get')).toBe(true);
    });
    unmount();

    const off = settingsWith({
      startView: 'feed',
      feed: { ...snapshotFixture().settings.feed, enabled: false },
    });
    const quiet = fakeBackend(snapshotFixture({ settings: off }));
    render(<App />);
    expect(await screen.findByText(/No installations yet/)).toBeVisible();
    const nav = screen.getByRole('navigation', { name: 'Main' });
    expect(within(nav).queryByRole('button', { name: 'Feed' })).toBeNull();
    expect(screen.getByRole('button', { name: 'Installations' })).toHaveAttribute(
      'aria-current',
      'page',
    );
    // Off means the launcher asks the feed for nothing.
    expect(quiet.some((c) => c.cmd.startsWith('feed_'))).toBe(false);
  });

  it('says under the feed that statistics are sent, and opens Privacy from there', async () => {
    const unseen = { stats: true, statsNoticeSeen: false };
    const calls = fakeBackend(
      snapshotFixture({ settings: settingsWith({ ...unseen, startView: 'feed' }) }),
    );
    const { unmount } = render(<App />);
    const line = await screen.findByText(/sends anonymous statistics without any ID/);
    // At the bottom of the feed.
    expect(screen.getByRole('main').lastElementChild?.contains(line)).toBe(true);
    fireEvent.click(screen.getByRole('button', { name: 'Privacy settings' }));
    expect(await screen.findByRole('tab', { name: 'Privacy' })).toHaveAttribute(
      'aria-selected',
      'true',
    );
    expect(screen.getByRole('switch', { name: /Anonymous statistics/ })).toBeChecked();
    expect(screen.queryByText(/sends anonymous statistics without any ID/)).toBeNull();
    await waitFor(() => {
      expect(
        calls.some(
          (c) =>
            c.cmd === 'set_app_settings' &&
            JSON.stringify(c.args).includes('"statsNoticeSeen":true'),
        ),
      ).toBe(true);
    });
    unmount();

    // With the feed off, under the installations.
    const off = settingsWith({
      ...unseen,
      feed: { ...snapshotFixture().settings.feed, enabled: false },
    });
    fakeBackend(snapshotFixture({ settings: off }));
    render(<App />);
    const under = await screen.findByText(/sends anonymous statistics without any ID/);
    expect(screen.getByRole('main').lastElementChild?.contains(under)).toBe(true);
    fireEvent.click(screen.getByRole('button', { name: 'Got it' }));
    expect(screen.queryByText(/sends anonymous statistics without any ID/)).toBeNull();
  });

  it("installs the feed's newest version through the new-installation form", async () => {
    const feed = {
      format: 1,
      generated: '2026-10-19T12:00:00Z',
      lang: 'en',
      versions: [
        {
          id: 'j-1-21-1',
          title: 'Minecraft 1.21.1',
          date: '2026-10-14T15:00:00Z',
          summary: '',
          translated: false,
          version: '1.21.1',
          kind: 'release',
          image: null,
          imageWidth: null,
          imageHeight: null,
          url: null,
        },
      ],
      news: [],
      videos: [],
      streams: [],
      cycle: null,
      latest: { release: null, snapshot: null },
      events: [],
      announcements: [],
      tip: null,
      fetched: {},
    };
    fakeBackend(
      snapshotFixture({ settings: settingsWith({ startView: 'feed' }) }),
      appInfoFixture,
      {
        feed_get: () => ({ feed, stale: false, problem: null }),
        feed_own_videos: () => [],
        feed_own_streams: () => [],
      },
    );
    render(<App />);
    fireEvent.click(await screen.findByRole('button', { name: 'Install 1.21.1' }));
    expect(screen.getByRole('button', { name: 'Installations' })).toHaveAttribute(
      'aria-current',
      'page',
    );
    // The latest release would be 26.3; the feed's version is chosen instead.
    await waitFor(() => {
      expect(screen.getByLabelText('Name')).toHaveValue('1.21.1');
    });
    // Settings has the feed's own section.
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    fireEvent.click(screen.getByRole('tab', { name: 'Feed' }));
    expect(screen.getByRole('switch', { name: /Show the feed/ })).toBeChecked();
  });

  it('opens the skins screen when it is the start screen', async () => {
    for (const settings of [
      settingsWith({ startView: 'skins' }),
      settingsWith({ startView: 'last', lastView: 'skins' }),
    ]) {
      fakeBackend(snapshotFixture({ settings }));
      const { unmount } = render(<App />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Skins' })).toHaveAttribute(
          'aria-current',
          'page',
        );
      });
      // Without a launcher account there is nothing to wear yet.
      fireEvent.click(screen.getByRole('button', { name: 'Open Accounts' }));
      expect(screen.getByRole('button', { name: 'Accounts' })).toHaveAttribute(
        'aria-current',
        'page',
      );
      unmount();
    }
  });

  it('opens Installations while something runs, whatever the start screen', async () => {
    fakeBackend(
      snapshotFixture({
        settings: settingsWith({ startView: 'account' }),
        instances: [instanceFixture({ busy: true })],
        activeOperation: { kind: 'install', instanceId: instanceFixture().id, operationId: 'op-1' },
      }),
    );
    render(<App />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Installations' })).toHaveAttribute(
        'aria-current',
        'page',
      );
    });
  });

  it('saves the start screen chosen in settings', async () => {
    const calls = fakeBackend(snapshotFixture());
    render(<App />);
    fireEvent.click(await screen.findByRole('button', { name: 'Settings' }));
    const picker = screen.getByRole('combobox', { name: 'Open at start' });
    expect(picker).toHaveValue('home');
    expect(
      within(picker)
        .getAllByRole('option')
        .map((o) => o.textContent),
    ).toEqual(['Feed', 'Installations', 'Mods', 'Skins', 'Accounts', 'Last opened']);
    fireEvent.change(picker, { target: { value: 'last' } });
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'set_app_settings')?.args).toMatchObject({
        request: { payload: { startView: 'last' } },
      });
    });
  });

  it('names the More menu in Arabic', async () => {
    fakeBackend(snapshotFixture({ settings: settingsWith({ language: 'ar' }) }));
    render(<App />);
    const more = await screen.findByRole('button', { name: 'المزيد' }, { timeout: 5000 });
    expect(document.documentElement.dir).toBe('rtl');
    fireEvent.click(more);
    fireEvent.click(screen.getByRole('menuitem', { name: 'لقطات الشاشة' }));
    expect(screen.getByRole('button', { name: 'المزيد: لقطات الشاشة' })).toHaveAttribute(
      'aria-current',
      'page',
    );
  });

  it('always shows the independent-project notice', () => {
    fakeBackend(snapshotFixture());
    render(<App />);
    expect(screen.getByRole('contentinfo')).toHaveTextContent(
      'not affiliated with or endorsed by Mojang Studios or Microsoft',
    );
  });

  it('puts Find in the rail, after the brand and outside the main tabs', async () => {
    fakeBackend(snapshotFixture());
    render(<App />);
    await screen.findByText(/No installations yet/);
    const find = screen.getByRole('button', { name: 'Find' });
    const nav = screen.getByRole('navigation', { name: 'Main' });
    expect(nav).not.toContainElement(find);
    // The rail: the header that holds the main tabs (a screen's own header is no banner
    // in a browser, but the test library counts every header as one).
    expect(find.closest('header')).toContainElement(nav);
    expect(find.compareDocumentPosition(nav) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(find).toHaveAttribute('aria-expanded', 'false');
    // The key caps are for the eye; the name is only the word.
    expect(find).toHaveTextContent('FindCtrlK');
    expect(find).toHaveAccessibleName('Find');
    // The closed palette leaves nothing behind that screens' names could collide with.
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(screen.queryByRole('option')).toBeNull();
    expect(screen.queryByRole('combobox', { name: 'Find in the launcher' })).toBeNull();
    // Focus can land on the screen itself when what the palette was opened from is gone.
    expect(screen.getByRole('main')).toHaveAttribute('tabindex', '-1');
  });
  it('closes the More menu when focus moves on, not when the window loses it', async () => {
    fakeBackend(snapshotFixture());
    render(<App />);
    await screen.findByText(/No installations yet/);
    fireEvent.click(moreButton());
    const menu = screen.getByRole('menu');
    const first = within(menu).getAllByRole('menuitem')[0];
    if (first === undefined) throw new Error('The More menu is empty.');
    // The window losing focus: no element gets it.
    fireEvent.blur(first, { relatedTarget: null });
    expect(screen.getByRole('menu')).toBeVisible();
    // Ctrl+K: the palette's field takes focus and the menu closes behind it.
    fireEvent.keyDown(window, { key: 'k', code: 'KeyK', ctrlKey: true });
    const field = await screen.findByRole('combobox', { name: 'Find in the launcher' });
    expect(field).toHaveFocus();
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('keeps the More menu working with the mouse where presses move focus', async () => {
    fakeBackend(snapshotFixture());
    render(<App />);
    await screen.findByText(/No installations yet/);
    fireEvent.click(moreButton());
    const about = within(screen.getByRole('menu')).getByRole('menuitem', { name: 'About' });
    // A press in the menu, on an item or its padding, leaves focus where it is.
    expect(fireEvent.mouseDown(screen.getByRole('menu'))).toBe(false);
    expect(fireEvent.mouseDown(about)).toBe(false);

    // WebKit on macOS: a press on the button takes focus off the item to the screen;
    // the click then closes the menu instead of finding it closed and opening it.
    fireEvent.pointerDown(moreButton(), { button: 0 });
    fireEvent.blur(focused(), { relatedTarget: screen.getByRole('main') });
    expect(screen.getByRole('menu')).toBeVisible();
    fireEvent.pointerUp(moreButton(), { button: 0 });
    fireEvent.click(moreButton());
    expect(screen.queryByRole('menu')).toBeNull();

    // The same press dragged off the button clicks nothing: the menu, with focus
    // outside it, closes.
    fireEvent.click(moreButton());
    expect(screen.getByRole('menu')).toBeVisible();
    fireEvent.pointerDown(moreButton(), { button: 0 });
    fireEvent.blur(focused(), { relatedTarget: screen.getByRole('main') });
    screen.getByRole('main').focus();
    fireEvent.pointerUp(document.body, { button: 0 });
    await waitFor(() => {
      expect(screen.queryByRole('menu')).toBeNull();
    });

    // Items still choose.
    chooseFromMore('About');
    expect(screen.getByRole('button', { name: 'More: About' })).toHaveAttribute(
      'aria-current',
      'page',
    );
  });
});
