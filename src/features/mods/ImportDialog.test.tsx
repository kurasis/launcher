import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it, vi } from 'vitest';
import type { ModsImportFound } from '../../lib/contracts';
import { instanceFixture } from '../../test/fixtures';
import { ImportDialog } from './ImportDialog';

const found: ModsImportFound = {
  format: 'folder',
  name: 'My mods',
  game: '1.20.1',
  loader: 'fabric',
  guessed: true,
  files: 3,
  bytes: 2048,
  mods: 2,
  leftOut: 1,
};

type Answers = Record<string, (args: Record<string, unknown>) => unknown>;

function setup(answers: Answers = {}) {
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  const instance = instanceFixture({ displayName: 'Brought' });
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: payload });
    const answer = answers[cmd];
    if (answer !== undefined) return answer(payload);
    switch (cmd) {
      case 'list_supported_versions':
        return {
          versions: [
            {
              id: '1.21.1',
              versionType: 'release',
              releaseTime: null,
              verified: true,
              availability: 'available',
            },
            {
              id: '24w14a',
              versionType: 'snapshot',
              releaseTime: null,
              verified: false,
              availability: 'available',
            },
          ],
          fetchedAt: null,
          stale: false,
          error: null,
          latestRelease: '1.21.1',
          latestSnapshot: null,
        };
      case 'mods_import_pick':
        return found;
      case 'mods_import_create':
        return instance;
      case 'mods_import_apply':
        return { files: 3, kept: 0, leftOut: 1 };
      case 'plugin:event|listen':
        return 1;
      default:
        return null;
    }
  });
  const onDone = vi.fn();
  render(<ImportDialog onClose={() => undefined} onDone={onDone} />);
  return { calls, onDone, instance };
}

const named = (cmd: string, calls: { cmd: string; args: Record<string, unknown> }[]) =>
  calls.filter((c) => c.cmd === cmd);

describe('ImportDialog', () => {
  it('brings a folder of mods into a new installation', async () => {
    const { calls, onDone, instance } = setup();
    fireEvent.click(screen.getByRole('button', { name: 'Choose a folder…' }));
    expect(await screen.findByText('Folder of mods')).toBeVisible();
    expect(screen.getByText(/3 files, 2.0 KiB · 2 mods/)).toBeVisible();
    expect(screen.getByText(/guessed from the mods/)).toBeVisible();
    expect(screen.getByText(/1 file is left out/)).toBeVisible();
    expect(named('mods_import_pick', calls)[0]?.args).toMatchObject({ folder: true });
    // The version found is offered though the list lacks it; snapshots are not.
    const game = screen.getByLabelText('Minecraft version');
    expect(game).toHaveValue('1.20.1');
    expect(screen.queryByRole('option', { name: '24w14a' })).toBeNull();
    expect(screen.getByLabelText('Mod loader')).toHaveValue('fabric');
    fireEvent.change(screen.getByLabelText('Installation name'), { target: { value: ' Pack ' } });
    fireEvent.change(screen.getByLabelText('Mod loader'), { target: { value: 'quilt' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create the installation' }));
    expect(await screen.findByText(/“Brought” is ready with 3 files/)).toBeVisible();
    const create = named('mods_import_create', calls)[0]?.args.request as {
      payload: Record<string, unknown>;
    };
    expect(create.payload).toEqual({ displayName: 'Pack', gameVersion: '1.20.1', loader: 'quilt' });
    const apply = named('mods_import_apply', calls)[0]?.args.request as {
      payload: Record<string, unknown>;
    };
    expect(apply.payload).toEqual({ instanceId: instance.id });
    expect(onDone).toHaveBeenCalledWith(instance.id);
  });

  it('asks for the version it could not tell, and stays when the dialog is closed', async () => {
    const { calls } = setup({
      mods_import_pick: (args) => (args.folder === true ? null : { ...found, game: null }),
    });
    fireEvent.click(screen.getByRole('button', { name: 'Choose a folder…' }));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Choose a zip…' })).toBeEnabled();
    });
    fireEvent.click(screen.getByRole('button', { name: 'Choose a zip…' }));
    expect(await screen.findByText(/could not tell the Minecraft version/)).toBeVisible();
    const create = screen.getByRole('button', { name: 'Create the installation' });
    expect(create).toBeDisabled();
    await screen.findByRole('option', { name: '1.21.1' });
    fireEvent.change(screen.getByLabelText('Minecraft version'), { target: { value: '1.21.1' } });
    expect(create).toBeEnabled();
    expect(named('mods_import_create', calls)).toHaveLength(0);
  });

  it('offers to finish when copying stopped', async () => {
    let tries = 0;
    setup({
      mods_import_apply: () => {
        tries += 1;
        return tries === 1
          ? Promise.reject(new Error('The download was cancelled.'))
          : { files: 1, kept: 2, leftOut: 0 };
      },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Choose a folder…' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Create the installation' }));
    expect(await screen.findByText(/not every file is in it yet/)).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
    expect(await screen.findByText(/is ready with 3 files/)).toBeVisible();
  });
});
