import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
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
    render(
      <DeleteInstance instance={instance} busy={false} onDeleted={onDeleted} onClose={vi.fn()} />,
    );
    const dialog = screen.getByRole('dialog', { name: 'Delete "Modpack"?' });
    // Cancel holds the focus while the folder is looked at, and after.
    // What it says is a live region: what the folder holds is read out when it comes.
    const said = within(dialog).getByText('Looking at what the folder holds…').parentElement;
    expect(said).toHaveAttribute('aria-live', 'polite');
    expect(dialog).toHaveAccessibleDescription('Looking at what the folder holds…');
    expect(within(dialog).getByRole('button', { name: 'Cancel' })).toHaveFocus();
    expect(await within(dialog).findByText('2 worlds (Home, Nether trip)')).toBeVisible();
    expect(within(dialog).getByText('12 mods')).toBeVisible();
    expect(within(dialog).getByText('1 screenshot')).toBeVisible();
    expect(within(dialog).getByText(/3\.0 MiB/)).toBeVisible();
    expect(within(dialog).getByRole('button', { name: 'Cancel' })).toHaveFocus();
    expect(dialog).toHaveAccessibleDescription(/3\.0 MiB/);
    fireEvent.click(within(dialog).getByRole('button', { name: 'Back up and delete' }));
    await waitFor(() => {
      expect(onDeleted).toHaveBeenCalledWith(
        '"Modpack" was backed up and deleted. Backups can bring it back.',
      );
    });
    expect(deleted(calls)).toMatchObject({
      request: { payload: { instanceId: instance.id, backup: true } },
    });
  });

  it('can delete without a backup', async () => {
    const calls = backend({ ...empty, mods: 1 });
    const onDeleted = vi.fn();
    render(
      <DeleteInstance instance={instance} busy={false} onDeleted={onDeleted} onClose={vi.fn()} />,
    );
    fireEvent.click(await screen.findByRole('button', { name: 'Delete without a backup' }));
    await waitFor(() => {
      expect(onDeleted).toHaveBeenCalledWith('"Modpack" was deleted.');
    });
    expect(deleted(calls)).toMatchObject({ request: { payload: { backup: false } } });
  });

  it('deletes nothing on Cancel or Escape', async () => {
    const calls = backend({ ...empty, mods: 1 });
    const onClose = vi.fn();
    const { unmount } = render(
      <DeleteInstance instance={instance} busy={false} onDeleted={vi.fn()} onClose={onClose} />,
    );
    await screen.findByRole('button', { name: 'Delete without a backup' });
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onClose).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(2);
    unmount();
    expect(deleted(calls)).toBeUndefined();
  });

  it('cannot be left by Escape while it deletes', async () => {
    let finish: (value: null) => void = () => undefined;
    mockIPC((cmd) => {
      if (cmd === 'inspect_instance') return empty;
      if (cmd === 'delete_instance')
        return new Promise((resolve) => {
          finish = resolve;
        });
      return null;
    });
    const onClose = vi.fn();
    const onDeleted = vi.fn();
    render(
      <DeleteInstance instance={instance} busy={false} onDeleted={onDeleted} onClose={onClose} />,
    );
    fireEvent.click(await screen.findByRole('button', { name: 'Delete installation' }));
    expect(await screen.findByText('Deleting…')).toBeVisible();
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeDisabled();
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    expect(onClose).not.toHaveBeenCalled();
    finish(null);
    await waitFor(() => {
      expect(onDeleted).toHaveBeenCalledWith('"Modpack" was deleted.');
    });
  });

  it('offers no backup when there is nothing of the player', async () => {
    backend(empty);
    render(
      <DeleteInstance instance={instance} busy={false} onDeleted={vi.fn()} onClose={vi.fn()} />,
    );
    expect(
      await screen.findByText('It holds no worlds, mods, packs or screenshots.'),
    ).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Back up and delete' })).toBeNull();
    // Deleting is drawn as such, never as the dialog's main answer.
    expect(screen.getByRole('button', { name: 'Delete installation' })).toBeEnabled();
    expect(screen.getByRole('button', { name: 'Cancel' })).toHaveFocus();
  });

  it('says when the folder could not be looked at, and looks again', async () => {
    let fail = true;
    mockIPC((cmd) => {
      if (cmd === 'inspect_instance') {
        if (fail) throw new Error('no folder');
        return empty;
      }
      return null;
    });
    render(
      <DeleteInstance instance={instance} busy={false} onDeleted={vi.fn()} onClose={vi.fn()} />,
    );
    const dialog = screen.getByRole('dialog', { name: 'Delete "Modpack"?' });
    expect(await within(dialog).findByRole('alert')).toBeVisible();
    expect(within(dialog).queryByRole('button', { name: 'Delete installation' })).toBeNull();
    fail = false;
    fireEvent.click(within(dialog).getByRole('button', { name: 'Try again' }));
    expect(
      await within(dialog).findByText('It holds no worlds, mods, packs or screenshots.'),
    ).toBeVisible();
    expect(within(dialog).queryByRole('alert')).toBeNull();
  });

  it('waits while something runs', async () => {
    backend(empty);
    render(<DeleteInstance instance={instance} busy onDeleted={vi.fn()} onClose={vi.fn()} />);
    expect(await screen.findByRole('button', { name: 'Delete installation' })).toBeDisabled();
    expect(screen.getByText('Wait until the running operation or game ends.')).toBeVisible();
  });
});
