import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it, vi } from 'vitest';
import type { UserData } from '../../lib/contracts';
import { instanceFixture, snapshotFixture } from '../../test/fixtures';
import { DeleteInstance } from './DeleteInstance';

const instance = instanceFixture({ displayName: 'Modpack', health: 'ready' });
const empty: UserData = {
  worlds: [],
  mods: 0,
  resourcePacks: 0,
  shaderPacks: 0,
  screenshots: 0,
  bytes: 1024,
};

function backend(data: UserData) {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === 'inspect_instance') return data;
    if (cmd === 'delete_instance') {
      const { request } = args as { request: { payload: { backup: boolean } } };
      return request.payload.backup ? { id: 'b-1' } : null;
    }
    if (cmd === 'get_app_snapshot') return snapshotFixture();
    return null;
  });
  return calls;
}

const deleted = (calls: { cmd: string; args: unknown }[]) =>
  calls.find((c) => c.cmd === 'delete_instance')?.args;

describe('DeleteInstance', () => {
  it('lists what the folder holds and backs it up before deleting', async () => {
    const calls = backend({
      ...empty,
      worlds: ['Home', 'Nether trip'],
      mods: 12,
      screenshots: 1,
      bytes: 3 * 1024 * 1024,
    });
    const onDeleted = vi.fn();
    render(<DeleteInstance instance={instance} busy={false} onDeleted={onDeleted} />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete installation' }));
    expect(await screen.findByText('Delete "Modpack"?')).toBeVisible();
    expect(screen.getByText('2 worlds (Home, Nether trip)')).toBeVisible();
    expect(screen.getByText('12 mods')).toBeVisible();
    expect(screen.getByText('1 screenshot')).toBeVisible();
    expect(screen.getByText(/3\.0 MiB/)).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Back up and delete' }));
    await waitFor(() => {
      expect(onDeleted).toHaveBeenCalledWith(
        '"Modpack" was backed up and deleted. Backups can bring it back.',
      );
    });
    expect(deleted(calls)).toMatchObject({
      request: { payload: { instanceId: instance.id, backup: true } },
    });
  });

  it('can delete without a backup or cancel', async () => {
    const calls = backend({ ...empty, mods: 1 });
    const onDeleted = vi.fn();
    render(<DeleteInstance instance={instance} busy={false} onDeleted={onDeleted} />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete installation' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Cancel' }));
    expect(screen.queryByText('Delete "Modpack"?')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Delete installation' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Delete without a backup' }));
    await waitFor(() => {
      expect(onDeleted).toHaveBeenCalledWith('"Modpack" was deleted.');
    });
    expect(deleted(calls)).toMatchObject({ request: { payload: { backup: false } } });
  });

  it('offers no backup when there is nothing of the player', async () => {
    backend(empty);
    render(<DeleteInstance instance={instance} busy={false} onDeleted={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete installation' }));
    expect(
      await screen.findByText('It holds no worlds, mods, packs or screenshots.'),
    ).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Back up and delete' })).toBeNull();
    expect(screen.getByRole('button', { name: 'Delete installation' })).toBeEnabled();
  });

  it('waits while something runs', () => {
    backend(empty);
    render(<DeleteInstance instance={instance} busy onDeleted={vi.fn()} />);
    expect(screen.getByRole('button', { name: 'Delete installation' })).toBeDisabled();
  });
});
