import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it, vi } from 'vitest';
import type { AppSnapshot, InstanceSummary, RunSummary, UserData } from '../../lib/contracts';
import { instanceFixture, snapshotFixture } from '../../test/fixtures';
import { InstancesView } from './InstancesView';

const nothingInside: UserData = {
  worlds: [],
  mods: 0,
  resourcePacks: 0,
  shaderPacks: 0,
  screenshots: 0,
  bytes: 0,
};

function backend() {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === 'set_app_settings') return snapshotFixture().settings;
    if (cmd === 'repair_instance') return { operationId: 'op-1' };
    if (cmd === 'inspect_instance') return nothingInside;
    return null;
  });
  return calls;
}

function instance(n: number, overrides: Partial<InstanceSummary> = {}): InstanceSummary {
  return instanceFixture({
    id: `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`,
    displayName: `Install ${String(n)}`,
    createdAt: `2026-10-${String(n).padStart(2, '0')}T12:00:00Z`,
    ...overrides,
  });
}

function snapshot(instances: InstanceSummary[], overrides: Partial<AppSnapshot> = {}) {
  const base = snapshotFixture();
  return snapshotFixture({
    instances,
    ...overrides,
    settings: {
      ...base.settings,
      selectedInstanceId: instances[0]?.id ?? null,
      ...overrides.settings,
    },
  });
}

/** The rows' names, in the order shown. */
function rowNames(): string[] {
  return within(screen.getByRole('list'))
    .getAllByRole('button')
    .map((row) => row.querySelector('span > span')?.textContent ?? '');
}

const many = [
  instance(1, { displayName: 'Survival', versionId: '1.21.1' }),
  instance(2, {
    displayName: 'Shaders',
    versionId: '1.21.1',
    loader: { kind: 'fabric', version: '0.16.9' },
    lastPlayedAt: '2026-10-20T19:12:00Z',
  }),
  instance(3, { displayName: 'Old server', versionId: '1.12.2' }),
  instance(4, { displayName: 'Creative', versionId: '26.3' }),
  instance(5, { displayName: 'Snapshot', versionId: '26.4-snapshot-3' }),
  instance(6, { displayName: 'Hardcore', versionId: '1.20.1' }),
  instance(7, {
    displayName: 'Skyblock',
    versionId: '1.16.5',
    lastPlayedAt: '2026-10-21T10:00:00Z',
  }),
];

describe('InstancesView', () => {
  it('shows search and order only for more than six installations', () => {
    backend();
    const { rerender } = render(
      <InstancesView snapshot={snapshot(many.slice(0, 6))} onEditSettings={() => undefined} />,
    );
    expect(screen.getByRole('heading', { level: 2, name: 'Installations' })).toBeVisible();
    expect(screen.getByText('6 installations')).toBeInTheDocument();
    expect(screen.queryByRole('searchbox')).toBeNull();
    expect(screen.queryByRole('group', { name: 'Order' })).toBeNull();
    rerender(<InstancesView snapshot={snapshot(many)} onEditSettings={() => undefined} />);
    const search = screen.getByRole('searchbox', { name: 'Find in the list' });
    expect(search).toHaveAttribute('maxLength', '64');
    expect(screen.getByRole('group', { name: 'Order' })).toBeVisible();
    // The search is not a "Name" field (the rename and create forms own that label).
    expect(screen.queryByLabelText('Name')).toBeNull();
  });

  it('finds and orders the installations, keeping the chosen one in the panel', () => {
    backend();
    render(<InstancesView snapshot={snapshot(many)} onEditSettings={() => undefined} />);
    // Recent first: the last game, then the newest installation.
    expect(rowNames()).toEqual([
      'Skyblock',
      'Shaders',
      'Hardcore',
      'Snapshot',
      'Creative',
      'Old server',
      'Survival',
    ]);
    expect(screen.getByRole('button', { name: 'Recent' })).toHaveAttribute('aria-pressed', 'true');
    fireEvent.click(screen.getByRole('button', { name: 'Name' }));
    expect(screen.getByRole('button', { name: 'Name' })).toHaveAttribute('aria-pressed', 'true');
    expect(rowNames()).toEqual([
      'Creative',
      'Hardcore',
      'Old server',
      'Shaders',
      'Skyblock',
      'Snapshot',
      'Survival',
    ]);
    fireEvent.click(screen.getByRole('button', { name: 'Version' }));
    expect(rowNames()).toEqual([
      'Snapshot',
      'Creative',
      'Shaders',
      'Survival',
      'Hardcore',
      'Skyblock',
      'Old server',
    ]);

    const search = screen.getByRole('searchbox', { name: 'Find in the list' });
    fireEvent.change(search, { target: { value: '  FABRIC ' } });
    expect(rowNames()).toEqual(['Shaders']);
    // Survival is still the chosen one, though the search hides its row.
    expect(screen.getByRole('heading', { level: 3, name: 'Survival' })).toBeVisible();
    fireEvent.change(search, { target: { value: '<img src=x onerror=alert(1)>' } });
    expect(screen.queryByRole('list')).toBeNull();
    expect(screen.getByText('Nothing found.')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Clear search' }));
    expect(search).toHaveValue('');
    expect(rowNames()).toHaveLength(many.length);
  });

  it('chooses an installation through the settings', async () => {
    const calls = backend();
    render(
      <InstancesView snapshot={snapshot(many.slice(0, 3))} onEditSettings={() => undefined} />,
    );
    const row = screen.getByRole('button', { name: /^Old server/ });
    expect(row).not.toHaveAttribute('aria-current');
    expect(within(row).getByText('Minecraft 1.12.2')).toBeVisible();
    expect(screen.getByRole('button', { name: /^Survival/ })).toHaveAttribute(
      'aria-current',
      'true',
    );
    fireEvent.click(row);
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'set_app_settings')?.args).toMatchObject({
        request: { payload: { selectedInstanceId: many[2]?.id } },
      });
    });
  });

  it('keeps finding usable while the settings are read-only', () => {
    const calls = backend();
    const base = snapshotFixture().settings;
    render(
      <InstancesView
        snapshot={snapshot(many, {
          settings: { ...base, selectedInstanceId: many[0]?.id ?? null, writable: false },
        })}
        onEditSettings={() => undefined}
      />,
    );
    for (const row of within(screen.getByRole('list')).getAllByRole('button')) {
      expect(row).toBeDisabled();
    }
    expect(screen.getByRole('button', { name: 'Bring in from another launcher' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'New installation' })).toBeEnabled();
    fireEvent.change(screen.getByRole('searchbox', { name: 'Find in the list' }), {
      target: { value: 'sky' },
    });
    expect(rowNames()).toEqual(['Skyblock']);
    fireEvent.click(screen.getByRole('button', { name: 'Name' }));
    expect(screen.getByRole('button', { name: 'Name' })).toHaveAttribute('aria-pressed', 'true');
    expect(calls.some((c) => c.cmd === 'set_app_settings')).toBe(false);
  });

  it('reaches Rename and Delete through the installation menu', async () => {
    const calls = backend();
    render(
      <InstancesView snapshot={snapshot(many.slice(0, 2))} onEditSettings={() => undefined} />,
    );
    const menuButton = screen.getByRole('button', { name: 'Installation actions' });
    // The menu's name leaves the installation out, so a search for it finds the row only.
    expect(screen.getAllByRole('button', { name: /Survival/ })).toHaveLength(1);
    expect(menuButton).toHaveAttribute('title', 'Installation actions');
    fireEvent.click(menuButton);
    const menu = screen.getByRole('menu');
    expect(
      within(menu)
        .getAllByRole('menuitem')
        .map((item) => item.textContent),
    ).toEqual(['Rename', 'Delete installation']);
    fireEvent.click(within(menu).getByRole('menuitem', { name: 'Rename' }));
    expect(screen.getByLabelText('Name')).toHaveValue('Survival');
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByLabelText('Name')).toBeNull();
    expect(screen.getByRole('heading', { level: 3, name: 'Survival' })).toHaveFocus();

    fireEvent.click(menuButton);
    fireEvent.click(screen.getByRole('menuitem', { name: 'Delete installation' }));
    expect(await screen.findByText('Delete "Survival"?')).toBeVisible();
    expect(calls.some((c) => c.cmd === 'inspect_instance')).toBe(true);
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByText('Delete "Survival"?')).toBeNull();
    fireEvent.click(menuButton);
    fireEvent.click(screen.getByRole('menuitem', { name: 'Delete installation' }));
    // Nothing of the player's inside: one plain Delete, no backup offer.
    expect(await screen.findByText('Delete "Survival"?')).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Back up and delete' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Delete installation' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'delete_instance')?.args).toMatchObject({
        request: { payload: { instanceId: many[0]?.id, backup: false } },
      });
    });
    expect(await screen.findByText('"Survival" was deleted.')).toBeVisible();
  });

  it('reaches settings, content, the folder, backups and Verify from the toolbar', async () => {
    const calls = backend();
    const onEditSettings = vi.fn();
    const onOpenContent = vi.fn();
    const onOpenBackups = vi.fn();
    const ready = instance(1, { displayName: 'Survival', health: 'ready' });
    render(
      <InstancesView
        snapshot={snapshot([ready])}
        onEditSettings={onEditSettings}
        onOpenContent={onOpenContent}
        onOpenBackups={onOpenBackups}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Installation settings' }));
    expect(onEditSettings).toHaveBeenCalledWith(ready.id);
    fireEvent.click(screen.getByRole('button', { name: 'Mods and content' }));
    expect(onOpenContent).toHaveBeenCalledWith(ready.id);
    fireEvent.click(screen.getByRole('button', { name: 'Backups' }));
    expect(onOpenBackups).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole('button', { name: 'Game folder' }));
    fireEvent.click(screen.getByRole('button', { name: 'Verify files' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'repair_instance')).toBeDefined();
    });
    expect(calls.find((c) => c.cmd === 'open_instance_folder')?.args).toMatchObject({
      request: { payload: { instanceId: ready.id, folder: null } },
    });
  });

  it('tells about the last game only when the data is about that game', () => {
    backend();
    const played = instance(1, {
      displayName: 'Survival',
      health: 'ready',
      lastPlayedAt: '2026-10-20T19:12:00Z',
      lastResult: { kind: 'launch', outcome: 'succeeded', finishedAt: '2026-10-20T19:54:00Z' },
    });
    const run: RunSummary = {
      runId: 'run-1',
      kind: 'game',
      instanceId: played.id,
      state: 'exited',
      startedAt: '2026-10-20T19:11:58Z',
      endedAt: '2026-10-20T19:54:00Z',
      exitCode: 0,
      durationMs: 42 * 60_000,
      drainTimedOut: false,
      outputTail: [],
      outputLinesDropped: 0,
      error: null,
      loggingDegraded: false,
      signal: null,
      diagnosis: null,
      server: null,
    };
    const { rerender } = render(
      <InstancesView
        snapshot={snapshot([instance(1, { displayName: 'Survival' })])}
        onEditSettings={() => undefined}
      />,
    );
    expect(screen.queryByText(/Last played/)).toBeNull();

    rerender(
      <InstancesView
        snapshot={snapshot([played], { lastRun: run })}
        onEditSettings={() => undefined}
      />,
    );
    expect(
      screen.getByText(/^Last played .+, 42 minutes\. The game ended without errors\.$/),
    ).toBeVisible();

    // Without this session's run there is no length, only the start and the ending.
    rerender(<InstancesView snapshot={snapshot([played])} onEditSettings={() => undefined} />);
    expect(screen.getByText(/^Last played .+\. The game ended without errors\.$/)).toBeVisible();
    expect(screen.queryByText(/minutes/)).toBeNull();

    // A repair since then replaced the result: the date moves to the details.
    const repaired = {
      ...played,
      lastResult: {
        kind: 'repair' as const,
        outcome: 'succeeded' as const,
        finishedAt: '2026-10-21T09:00:00Z',
      },
    };
    rerender(<InstancesView snapshot={snapshot([repaired])} onEditSettings={() => undefined} />);
    expect(screen.queryByText(/^Last played .+\./)).toBeNull();
    expect(screen.getByText('Last played')).toBeVisible();
    expect(screen.getByText('Last result')).toBeVisible();

    // The game runs again: the result is the previous game's.
    const running = { ...played, lastPlayedAt: '2026-10-22T08:00:00Z' };
    rerender(<InstancesView snapshot={snapshot([running])} onEditSettings={() => undefined} />);
    expect(screen.queryByText(/^Last played .+\./)).toBeNull();
    expect(screen.getByText('Last played')).toBeVisible();
  });

  it('shows only the new-installation form while creating', () => {
    backend();
    render(
      <InstancesView
        snapshot={snapshot(many)}
        onEditSettings={() => undefined}
        createRequest={{ version: null, loader: null }}
      />,
    );
    expect(screen.queryByRole('heading', { name: 'Installations' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'New installation' })).toBeNull();
    expect(screen.queryByRole('button', { name: /^Survival/ })).toBeNull();
    expect(screen.queryByRole('heading', { level: 3, name: 'Survival' })).toBeNull();
  });

  it('says how to start when there is no installation', () => {
    backend();
    render(<InstancesView snapshot={snapshot([])} onEditSettings={() => undefined} />);
    expect(screen.getByText(/No installations yet/)).toBeVisible();
    expect(screen.getByRole('button', { name: 'New installation' })).toBeVisible();
    expect(screen.queryByRole('list')).toBeNull();
  });
});
