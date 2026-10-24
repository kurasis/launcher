import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type {
  AppInfo,
  AppSnapshot,
  ContentItem,
  ContentView,
  InstanceSummary,
  ModsHit,
  ModsSearchPage,
  ModsSearchQuery,
  MutationRequest,
} from '../lib/contracts';
import { setLanguage } from '../lib/strings';
import { appInfoFixture, instanceFixture, snapshotFixture } from '../test/fixtures';
import { App } from './App';

interface Options {
  info?: AppInfo;
  /** Mods folders by installation. */
  content?: Record<string, ContentItem[]>;
  /** The catalog's answer to a search text. */
  search?: (text: string) => ModsSearchPage | Promise<ModsSearchPage>;
}

// Test-only fake backend. Production code always talks to the real Rust core.
function backend(initial: AppSnapshot, options: Options = {}) {
  let snapshot = initial;
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC(
    (cmd, args) => {
      calls.push({ cmd, args });
      switch (cmd) {
        case 'get_app_snapshot':
          return snapshot;
        case 'get_app_info':
          return options.info ?? appInfoFixture;
        case 'set_app_settings': {
          const { request } = args as {
            request: MutationRequest<Partial<AppSnapshot['settings']>>;
          };
          snapshot = {
            ...snapshot,
            revision: snapshot.revision + 1,
            settings: { ...snapshot.settings, ...request.payload },
          };
          return snapshot.settings;
        }
        case 'mods_content': {
          const { instanceId } = args as { instanceId: string };
          const view: ContentView = {
            items: options.content?.[instanceId] ?? [],
            worlds: [],
            pack: null,
          };
          return view;
        }
        case 'mods_harmful':
        case 'mods_identify':
          return [];
        case 'mods_search': {
          const { query } = args as { query: ModsSearchQuery };
          return options.search?.(query.text) ?? { total: 0, hits: [] };
        }
        case 'install_instance':
        case 'repair_instance':
          return { operationId: 'op-1' };
        case 'launch_game':
          return { runId: 'run-1', kind: 'game', state: 'preflight' };
        case 'list_supported_versions':
          return {
            versions: [],
            fetchedAt: null,
            stale: false,
            error: null,
            latestRelease: null,
            latestSnapshot: null,
          };
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );
  return calls;
}

const SURVIVAL = instanceFixture();
const CREATIVE = instanceFixture({
  id: '1f1e2d3c-4b5a-4978-8796-a5b4c3d2e1f0',
  displayName: 'Creative',
  health: 'ready',
});

/** A snapshot with these installations, the first one chosen. */
function withInstances(
  instances: InstanceSummary[],
  rest: Partial<AppSnapshot> = {},
  settings: Partial<AppSnapshot['settings']> = {},
): AppSnapshot {
  return snapshotFixture({
    instances,
    settings: {
      ...snapshotFixture().settings,
      selectedInstanceId: instances[0]?.id ?? null,
      ...settings,
    },
    ...rest,
  });
}

function item(overrides: Partial<ContentItem>): ContentItem {
  return {
    path: 'mods/sodium.jar',
    folder: 'mods',
    name: 'sodium.jar',
    world: null,
    enabled: true,
    size: 1,
    directory: false,
    projectId: null,
    versionId: null,
    title: 'Sodium',
    versionNumber: null,
    icon: null,
    dependency: false,
    source: null,
    fromPack: false,
    ...overrides,
  };
}

function hit(id: string, title: string): ModsHit {
  return {
    id,
    kind: 'mod',
    slug: id,
    title,
    summary: '',
    icon: null,
    downloads: 1200,
    status: 'ready',
    updated: 0,
    author: null,
    sources: ['modrinth'],
    loaders: [],
    categoryIds: [],
    cover: null,
  };
}

/** The app with its snapshot shown. */
async function ready(): Promise<void> {
  render(<App />);
  await screen.findByRole('region', { name: 'Launch' });
}

function findButton() {
  return screen.getByRole('button', { name: 'Find' });
}

function palette() {
  return screen.queryByRole('dialog', { name: 'Find in the launcher' });
}

function field() {
  return screen.getByRole('combobox', { name: 'Find in the launcher' });
}

/** The palette's live line: how many rows, and why the catalog was not asked. */
function announced() {
  const dialog = palette();
  if (dialog === null) throw new Error('The palette is closed.');
  return within(dialog).getByRole('status');
}

function press(init: KeyboardEventInit) {
  fireEvent.keyDown(window, { key: 'k', code: 'KeyK', ...init });
}

function type(text: string) {
  fireEvent.change(field(), { target: { value: text } });
}

/** Lets promises (IPC answers) settle while timers are fake. */
async function settle() {
  await act(async () => {
    for (let i = 0; i < 20; i += 1) await Promise.resolve();
  });
}

describe('CommandPalette', () => {
  afterEach(() => {
    vi.useRealTimers();
    setLanguage('en');
  });

  it('opens with Ctrl+K only, and closes with it again', async () => {
    backend(withInstances([SURVIVAL]));
    await ready();
    press({ ctrlKey: true, shiftKey: true });
    press({ ctrlKey: true, altKey: true });
    press({ metaKey: true });
    press({ ctrlKey: true, repeat: true });
    expect(palette()).toBeNull();
    press({ ctrlKey: true });
    expect(palette()).toBeVisible();
    expect(palette()).toHaveAttribute('aria-modal', 'true');
    expect(field()).toHaveFocus();
    expect(field()).toHaveAttribute('maxLength', '200');
    expect(findButton()).toHaveAttribute('aria-expanded', 'true');
    // From inside the field too.
    fireEvent.keyDown(field(), { key: 'k', code: 'KeyK', ctrlKey: true });
    expect(palette()).toBeNull();
    expect(findButton()).toHaveAttribute('aria-expanded', 'false');
  });

  it('takes ⌘K on macOS and leaves Ctrl+K to the text fields there', async () => {
    backend(withInstances([SURVIVAL]), { info: { ...appInfoFixture, os: 'macos' } });
    await ready();
    await waitFor(() => {
      expect(findButton()).toHaveAttribute('aria-keyshortcuts', 'Meta+K');
    });
    expect(findButton()).toHaveAttribute('title', 'Find (⌘K)');
    press({ ctrlKey: true });
    expect(palette()).toBeNull();
    press({ metaKey: true });
    expect(palette()).toBeVisible();
    expect(within(palette() as HTMLElement).getAllByText('⌘').length).toBeGreaterThan(0);
  });

  it('never opens over another modal dialog', async () => {
    backend(withInstances([SURVIVAL]));
    await ready();
    const other = document.createElement('div');
    other.setAttribute('role', 'dialog');
    other.setAttribute('aria-modal', 'true');
    document.body.append(other);
    press({ ctrlKey: true });
    expect(palette()).toBeNull();
    other.remove();
    press({ ctrlKey: true });
    expect(palette()).toBeVisible();
  });

  it('opens from Find, and Escape closes it and gives focus back', async () => {
    backend(withInstances([SURVIVAL]));
    await ready();
    const find = findButton();
    expect(find).toHaveAttribute('aria-haspopup', 'dialog');
    expect(find).toHaveAttribute('aria-keyshortcuts', 'Control+K');
    expect(find).toHaveAttribute('title', 'Find (Ctrl+K)');
    fireEvent.click(find);
    expect(palette()).toBeVisible();
    expect(field()).toHaveFocus();
    // Tab stays in the field.
    fireEvent.keyDown(field(), { key: 'Tab' });
    expect(field()).toHaveFocus();
    const escapes = vi.fn();
    window.addEventListener('keydown', escapes);
    fireEvent.keyDown(field(), { key: 'Escape' });
    window.removeEventListener('keydown', escapes);
    expect(palette()).toBeNull();
    // Escape listeners of the screen behind never see it.
    expect(escapes).not.toHaveBeenCalled();
    expect(find).toHaveFocus();
  });

  it('moves through the rows with the arrows, around the ends', async () => {
    backend(withInstances([SURVIVAL, CREATIVE]));
    await ready();
    press({ ctrlKey: true });
    const options = screen.getAllByRole('option');
    // Two installations, then eight pages and actions.
    expect(options).toHaveLength(10);
    const listbox = screen.getByRole('listbox', { name: 'Results' });
    expect(field()).toHaveAttribute('aria-controls', listbox.id);
    expect(field()).toHaveAttribute('aria-expanded', 'true');
    expect(
      within(listbox)
        .getAllByRole('group')
        .map((group) => group.getAttribute('aria-labelledby') ?? ''),
    ).toHaveLength(2);
    expect(screen.getByRole('group', { name: 'Installations' })).toBeVisible();
    expect(screen.getByRole('group', { name: 'Pages and actions' })).toBeVisible();
    const active = () => field().getAttribute('aria-activedescendant');
    expect(active()).toBe(options[0]?.id);
    expect(options[0]).toHaveAttribute('aria-selected', 'true');
    expect(options[0]).toHaveAccessibleName('Survival Minecraft 1.21.1 · Not installed Install');
    fireEvent.keyDown(field(), { key: 'ArrowDown' });
    expect(active()).toBe(options[1]?.id);
    expect(options[1]).toHaveAttribute('aria-selected', 'true');
    expect(options[0]).toHaveAttribute('aria-selected', 'false');
    fireEvent.keyDown(field(), { key: 'ArrowUp' });
    fireEvent.keyDown(field(), { key: 'ArrowUp' });
    expect(active()).toBe(options[9]?.id);
    fireEvent.keyDown(field(), { key: 'ArrowDown' });
    expect(active()).toBe(options[0]?.id);
    expect(announced()).toHaveTextContent('10 results');
  });

  it('opens a page with Enter, and highlights what matched', async () => {
    backend(withInstances([SURVIVAL]));
    await ready();
    press({ ctrlKey: true });
    type('sett');
    const first = screen.getAllByRole('option')[0];
    expect(first).toHaveAccessibleName('Settings Page Go');
    expect(first?.querySelector('mark')).toHaveTextContent(/^Sett$/);
    fireEvent.keyDown(field(), { key: 'Enter' });
    expect(palette()).toBeNull();
    const nav = screen.getByRole('navigation', { name: 'Main' });
    expect(within(nav).getByRole('button', { name: 'Settings' })).toHaveAttribute(
      'aria-current',
      'page',
    );
  });

  it('opens the console, sections, and a new installation with a loader', async () => {
    const calls = backend(withInstances([SURVIVAL]));
    await ready();
    press({ ctrlKey: true });
    type('console');
    expect(screen.getAllByRole('option')[0]).toHaveAccessibleName('Open console Diagnostics');
    fireEvent.keyDown(field(), { key: 'Enter' });
    await waitFor(() => {
      expect(calls.some((c) => c.cmd === 'open_console')).toBe(true);
    });

    press({ ctrlKey: true });
    type('privacy');
    fireEvent.keyDown(field(), { key: 'Enter' });
    expect(await screen.findByRole('tab', { name: 'Privacy' })).toHaveAttribute(
      'aria-selected',
      'true',
    );

    press({ ctrlKey: true });
    type('with fabric');
    expect(screen.getAllByRole('option')[0]).toHaveAccessibleName(
      'New installation with Fabric Installations Create',
    );
    fireEvent.keyDown(field(), { key: 'Enter' });
    expect(await screen.findByRole('radio', { name: 'Fabric' })).toBeChecked();
  });

  it('chooses a row with a click, keeping focus in the field', async () => {
    backend(withInstances([SURVIVAL]));
    await ready();
    press({ ctrlKey: true });
    const mods = screen.getByRole('option', { name: 'Mods Page Go' });
    fireEvent.mouseMove(mods);
    expect(field()).toHaveAttribute('aria-activedescendant', mods.id);
    // The press itself does not move focus out of the field.
    expect(fireEvent.mouseDown(mods)).toBe(false);
    fireEvent.click(mods);
    expect(palette()).toBeNull();
    expect(
      within(screen.getByRole('navigation', { name: 'Main' })).getByRole('button', {
        name: 'Mods',
      }),
    ).toHaveAttribute('aria-current', 'page');
  });

  it('opens an installation with Enter, and runs its main action with Ctrl+Enter', async () => {
    const calls = backend(withInstances([SURVIVAL, CREATIVE]));
    await ready();
    press({ ctrlKey: true });
    type('surv');
    expect(screen.getAllByRole('option')[0]).toHaveAccessibleName(
      'Survival Minecraft 1.21.1 · Not installed Install',
    );
    fireEvent.keyDown(field(), { key: 'Enter', ctrlKey: true });
    expect(palette()).toBeNull();
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'install_instance')?.args).toMatchObject({
        request: { payload: { instanceId: SURVIVAL.id } },
      });
    });

    // Another installation is chosen first, then played in the launch bar.
    press({ ctrlKey: true });
    type('creat');
    fireEvent.keyDown(field(), { key: 'Enter', ctrlKey: true });
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'launch_game')?.args).toMatchObject({
        request: { payload: { instanceId: CREATIVE.id } },
      });
    });
    expect(calls.find((c) => c.cmd === 'set_app_settings')?.args).toMatchObject({
      request: { payload: { selectedInstanceId: CREATIVE.id } },
    });
  });

  it('opens an installation with plain Enter, leaving a new-installation form', async () => {
    const calls = backend(withInstances([SURVIVAL, CREATIVE]));
    await ready();
    fireEvent.click(screen.getByRole('button', { name: 'New installation' }));
    expect(screen.getByRole('heading', { name: 'New installation' })).toBeVisible();
    press({ ctrlKey: true });
    type('creat');
    fireEvent.keyDown(field(), { key: 'Enter' });
    expect(palette()).toBeNull();
    // The list with the installation asked for, not the form left open.
    expect(await screen.findByRole('heading', { level: 2, name: 'Installations' })).toBeVisible();
    expect(screen.queryByRole('heading', { name: 'New installation' })).toBeNull();
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'set_app_settings')?.args).toMatchObject({
        request: { payload: { selectedInstanceId: CREATIVE.id } },
      });
    });
    // Opening is not playing.
    expect(calls.some((c) => c.cmd === 'install_instance' || c.cmd === 'launch_game')).toBe(false);
  });

  it('keeps focus in the field when its frame is pressed, and gives it back from the scrim', async () => {
    backend(withInstances([SURVIVAL]));
    await ready();
    const find = findButton();
    fireEvent.click(find);
    const dialog = palette();
    if (dialog === null) throw new Error('The palette is closed.');
    // A press on a hint or a group heading keeps the keys working in the field.
    expect(fireEvent.mouseDown(within(dialog).getByText('main action'))).toBe(false);
    expect(fireEvent.mouseDown(within(dialog).getByText('Pages and actions'))).toBe(false);
    // The field itself takes presses as usual.
    expect(fireEvent.mouseDown(field())).toBe(true);
    // A press on the dimmed screen closes it, and the browser does not move focus after.
    const backdrop = dialog.parentElement;
    if (backdrop === null) throw new Error('The palette has no backdrop.');
    expect(fireEvent.mouseDown(backdrop)).toBe(false);
    expect(palette()).toBeNull();
    expect(find).toHaveFocus();
  });

  it('says when nothing is found', async () => {
    backend(withInstances([SURVIVAL]));
    await ready();
    press({ ctrlKey: true });
    type('zzqx <b>');
    expect(
      await screen.findByText('Nothing found. Try a shorter word or another spelling.'),
    ).toBeVisible();
    expect(screen.queryAllByRole('option')).toHaveLength(0);
    expect(field()).toHaveAttribute('aria-expanded', 'false');
    expect(field()).not.toHaveAttribute('aria-activedescendant');
    expect(announced()).toHaveTextContent('Nothing found');
    // Typed markup stays text.
    expect(document.querySelector('b')).toBeNull();
  });

  it('asks the catalog after a pause and drops answers to older queries', async () => {
    const asked: { text: string; answer: (page: ModsSearchPage) => void }[] = [];
    const calls = backend(withInstances([SURVIVAL]), {
      search: (text) =>
        new Promise<ModsSearchPage>((resolve) => {
          asked.push({ text, answer: resolve });
        }),
    });
    await ready();
    press({ ctrlKey: true });
    vi.useFakeTimers();
    const searches = () => calls.filter((c) => c.cmd === 'mods_search');
    type('s');
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    // One character never reaches the catalog.
    expect(searches()).toHaveLength(0);
    type('so');
    await act(async () => {
      await vi.advanceTimersByTimeAsync(200);
    });
    type('sod');
    await act(async () => {
      await vi.advanceTimersByTimeAsync(249);
    });
    expect(searches()).toHaveLength(0);
    expect(screen.getByText('Searching the catalog…')).toBeVisible();
    // Heard too, so "Nothing found" is never said alone while the catalog is asked.
    expect(announced()).toHaveTextContent(/Searching the catalog…$/);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(searches()).toHaveLength(1);
    expect(searches()[0]?.args).toEqual({
      query: {
        text: 'sod',
        kind: 'mod',
        game: null,
        loader: null,
        category: null,
        sort: null,
        offset: 0,
        limit: 5,
      },
    });

    type('sodium');
    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });
    expect(searches()).toHaveLength(2);
    asked[1]?.answer({ total: 1, hits: [hit('p-2', 'Sodium Extra')] });
    await settle();
    asked[0]?.answer({ total: 1, hits: [hit('p-1', 'Sodalite')] });
    await settle();
    expect(screen.getByRole('option', { name: /^Sodium Extra In the catalog/ })).toBeVisible();
    expect(screen.queryByText('Sodalite')).toBeNull();
    expect(screen.getByRole('group', { name: 'Mods' })).toBeVisible();
  });

  it('opens a catalog hit on its page in Mods', async () => {
    const calls = backend(withInstances([SURVIVAL]), {
      search: () => ({ total: 1, hits: [hit('p-9', 'Fabric API')] }),
    });
    await ready();
    press({ ctrlKey: true });
    type('fabric api');
    const found = await screen.findByRole('option', { name: /^Fabric API In the catalog/ });
    fireEvent.click(found);
    expect(palette()).toBeNull();
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'mods_project')?.args).toEqual({ project: 'p-9' });
    });
  });

  it('never asks the catalog offline, and says so', async () => {
    const calls = backend(withInstances([SURVIVAL], { network: 'offline' }));
    await ready();
    press({ ctrlKey: true });
    vi.useFakeTimers();
    type('sodium');
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    expect(calls.some((c) => c.cmd === 'mods_search')).toBe(false);
    expect(
      screen.getByText('The catalog is searched when the internet connection returns.'),
    ).toBeVisible();
    expect(announced()).toHaveTextContent(
      'The catalog is searched when the internet connection returns.',
    );
  });

  it('reads installed mods only of installations with a mod loader', async () => {
    const fabric = instanceFixture({
      id: 'a1a1a1a1-0000-4000-8000-000000000001',
      displayName: 'Fabric world',
      loader: { kind: 'fabric', version: '0.16.9' },
    });
    const forge = instanceFixture({
      id: 'a1a1a1a1-0000-4000-8000-000000000002',
      displayName: 'Forge world',
      loader: { kind: 'forge', version: '1.21.1-52.0.1' },
    });
    const optifine = instanceFixture({
      id: 'a1a1a1a1-0000-4000-8000-000000000003',
      displayName: 'OptiFine world',
      loader: { kind: 'optifine', version: '1.21.1_HD_U_J1' },
    });
    const calls = backend(withInstances([SURVIVAL, fabric, forge, optifine]), {
      content: {
        [fabric.id]: [
          item({}),
          item({ path: 'shaderpacks/x.zip', folder: 'shaderpacks', title: 'Sodium shaders' }),
        ],
      },
    });
    await ready();
    press({ ctrlKey: true });
    await waitFor(() => {
      expect(
        calls
          .filter((c) => c.cmd === 'mods_content')
          .map((c) => (c.args as { instanceId: string }).instanceId)
          .sort(),
      ).toEqual([fabric.id, forge.id]);
    });
    type('sodi');
    const sodium = await screen.findByRole('option', {
      name: 'Sodium Installed in “Fabric world” Open',
    });
    expect(screen.queryByText('Sodium shaders')).toBeNull();
    fireEvent.click(sodium);
    expect(await screen.findByRole('tab', { name: 'Installed' })).toHaveAttribute(
      'aria-selected',
      'true',
    );
    expect(screen.getByLabelText('Installation')).toHaveValue(fabric.id);
  });

  it('ignores the Enter that ends an IME composition', async () => {
    backend(withInstances([SURVIVAL]));
    await ready();
    press({ ctrlKey: true });
    type('sett');
    fireEvent.keyDown(field(), { key: 'Enter', isComposing: true });
    expect(palette()).toBeVisible();
    // WebKit sends the Enter after the composition has ended.
    fireEvent.compositionEnd(field());
    fireEvent.keyDown(field(), { key: 'Enter' });
    expect(palette()).toBeVisible();
    fireEvent.keyUp(field(), { key: 'Enter' });
    fireEvent.keyDown(field(), { key: 'Enter' });
    expect(palette()).toBeNull();
  });

  it('takes an Enter after a candidate picked with the mouse', async () => {
    backend(withInstances([SURVIVAL]));
    await ready();
    press({ ctrlKey: true });
    type('sett');
    const now = Date.now();
    const clock = vi.spyOn(Date, 'now').mockReturnValue(now);
    fireEvent.compositionEnd(field());
    // No key was released; a moment later the Enter is the player's own.
    clock.mockReturnValue(now + 1000);
    fireEvent.keyDown(field(), { key: 'Enter' });
    expect(palette()).toBeNull();
  });

  it('speaks Arabic, keeping key caps left to right', async () => {
    backend(withInstances([SURVIVAL], {}, { language: 'ar' }));
    render(<App />);
    const find = await screen.findByRole('button', { name: 'بحث' }, { timeout: 5000 });
    fireEvent.click(find);
    const dialog = screen.getByRole('dialog', { name: 'البحث في المشغّل' });
    const input = within(dialog).getByRole('combobox');
    expect(input).toHaveAttribute('placeholder', 'عمّ تبحث؟');
    expect(input).toHaveAttribute('dir', 'auto');
    expect(within(dialog).getByRole('group', { name: 'الصفحات والإجراءات' })).toBeVisible();
    for (const key of within(dialog).getAllByText('Esc')) {
      expect(key.closest('[dir]')).toHaveAttribute('dir', 'ltr');
    }
  });
});
