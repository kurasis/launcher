import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import type { BackupInfo, BackupOverview } from '../../lib/contracts';
import { instanceFixture, snapshotFixture } from '../../test/fixtures';
import { BackupsView } from './BackupsView';

const instance = instanceFixture({ health: 'ready' });

function backup(overrides: Partial<BackupInfo> = {}): BackupInfo {
  return {
    id: '7d5c2a1e-0000-4000-8000-000000000001',
    instanceId: instance.id,
    instanceName: 'Survival',
    versionLabel: 'Minecraft 1.21.1',
    scope: { kind: 'world', name: 'My World' },
    reason: 'manual',
    createdAt: '2026-10-17T12:00:00Z',
    fileCount: 42,
    totalBytes: 5 * 1024 * 1024,
    newBytes: 1024 * 1024,
    ...overrides,
  };
}

function setup(overview: BackupOverview) {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === 'list_backups') return overview;
    if (cmd === 'create_backup') return backup({ id: 'new' });
    if (cmd === 'restore_backup') return { world: 'My World (backup 2026-10-17 12-00)' };
    if (cmd === 'delete_backup') return 1024 * 1024;
    if (cmd === 'get_app_snapshot') return snapshotFixture();
    return null;
  });
  render(<BackupsView snapshot={snapshotFixture({ instances: [instance] })} />);
  return calls;
}

const named = (cmd: string, calls: { cmd: string; args: unknown }[]) =>
  calls.filter((c) => c.cmd === cmd);

describe('BackupsView', () => {
  it('backs up a world or the whole installation', async () => {
    const calls = setup({
      backups: [],
      worlds: [{ instanceId: instance.id, worlds: ['My World'] }],
      storeBytes: 0,
    });
    expect(await screen.findByText('My World')).toBeVisible();
    expect(screen.getByText('Not backed up yet')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Back up' }));
    await waitFor(() => {
      expect(named('create_backup', calls)[0]?.args).toMatchObject({
        request: {
          payload: { instanceId: instance.id, scope: { kind: 'world', name: 'My World' } },
        },
      });
    });
    expect(await screen.findByText('World “My World” backed up.')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Back up the whole installation' }));
    await waitFor(() => {
      expect(named('create_backup', calls)[1]?.args).toMatchObject({
        request: { payload: { scope: { kind: 'installation' } } },
      });
    });
  });

  it('lists backups and restores or deletes them after confirmation', async () => {
    const calls = setup({
      backups: [backup()],
      worlds: [{ instanceId: instance.id, worlds: ['My World'] }],
      storeBytes: 5 * 1024 * 1024,
    });
    expect(await screen.findByText('Backups use 5.0 MiB')).toBeVisible();
    expect(screen.getByText('42 files · 5.0 MiB · 1.0 MiB new')).toBeVisible();
    expect(screen.getByText('Made by you')).toBeVisible();

    fireEvent.click(screen.getByRole('button', { name: 'Restore' }));
    expect(screen.getByText(/Its current state is backed up first/)).toBeVisible();
    expect(named('restore_backup', calls)).toHaveLength(0);
    fireEvent.click(screen.getAllByRole('button', { name: 'Restore' })[0] as HTMLElement);
    await waitFor(() => {
      expect(named('restore_backup', calls)[0]?.args).toMatchObject({
        request: { payload: { backupId: backup().id, asCopy: false } },
      });
    });

    fireEvent.click(screen.getByRole('button', { name: 'Restore as a copy' }));
    expect(
      await screen.findByText('Restored as the world “My World (backup 2026-10-17 12-00)”.'),
    ).toBeVisible();

    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    fireEvent.click(screen.getAllByRole('button', { name: 'Delete' })[0] as HTMLElement);
    expect(await screen.findByText('Deleted; 1.0 MiB freed.')).toBeVisible();
  });

  it('switches automatic backups after playing', async () => {
    const calls = setup({ backups: [], worlds: [], storeBytes: 0 });
    fireEvent.click(
      await screen.findByRole('checkbox', { name: /Back up changed worlds after playing/ }),
    );
    await waitFor(() => {
      expect(named('set_app_settings', calls)[0]?.args).toMatchObject({
        request: { payload: { backupAfterPlay: true } },
      });
    });
  });
});
