import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import type { ResetPreview } from '../../lib/contracts';
import { snapshotFixture } from '../../test/fixtures';
import { ResetSettings } from './ResetSettings';

const empty = { worlds: [], mods: 0, resourcePacks: 0, shaderPacks: 0, screenshots: 0, bytes: 0 };
const preview: ResetPreview = {
  installations: [
    {
      instanceId: 'a',
      displayName: 'Modpack',
      versionLabel: 'Minecraft 1.20.1 · Forge 47.4.10',
      data: { ...empty, worlds: ['Base'], mods: 3, bytes: 1024 },
    },
    { instanceId: 'b', displayName: 'Plain', versionLabel: 'Minecraft 1.21.1', data: empty },
  ],
  gameFilesBytes: 2 * 1024 * 1024 * 1024,
  backupCount: 4,
  backupBytes: 300 * 1024 * 1024,
  microsoftAccounts: 1,
  notokenProfiles: 2,
  temporaryBytes: 10 * 1024 * 1024,
};

function backend(gate?: Promise<void>) {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC(async (cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === 'reset_preview') return preview;
    if (cmd === 'reset_launcher') {
      await gate;
      return {
        backedUp: 1,
        deletedInstallations: 2,
        deletedBackups: 4,
        freedBytes: 1024,
        signedOut: 0,
      };
    }
    if (cmd === 'get_app_snapshot') return snapshotFixture();
    return null;
  });
  return calls;
}

describe('ResetSettings', () => {
  it('lists the player data, backs it up first and resets the chosen parts', async () => {
    const calls = backend();
    render(<ResetSettings busy={false} />);
    const installations = await screen.findByRole('checkbox', { name: /^Installations/ });
    expect(screen.getByText(/2\.0 GiB: libraries, assets, Java/)).toBeVisible();
    expect(screen.getByText('Deletes 4 backups (300.0 MiB).')).toBeVisible();
    expect(screen.getByRole('button', { name: 'Reset…' })).toBeDisabled();

    fireEvent.click(installations);
    fireEvent.click(screen.getByRole('checkbox', { name: /^Backups/ }));
    // Only the installation with player data is listed, with a backup offered.
    const listed = screen.getByText('Modpack').closest('li') as HTMLElement;
    expect(listed).toHaveTextContent('1 world (Base), 3 mods');
    expect(screen.queryByText('Plain')).toBeNull();
    expect(screen.getByRole('checkbox', { name: /Back them up before deleting/ })).toBeChecked();
    expect(
      screen.getByText('Existing backups are deleted first; the new ones are kept.'),
    ).toBeVisible();

    // Cancel gives the focus back to Reset….
    screen.getByRole('button', { name: 'Reset…' }).focus();
    fireEvent.click(screen.getByRole('button', { name: 'Reset…' }));
    expect(screen.getByRole('alertdialog', { name: 'Reset' })).toHaveFocus();
    expect(screen.getByRole('alertdialog')).toHaveAccessibleDescription(
      'Reset these parts? Installations Backups This cannot be undone.',
    );
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.getByRole('button', { name: 'Reset…' })).toHaveFocus();
    fireEvent.click(screen.getByRole('button', { name: 'Reset…' }));
    const confirm = screen.getByText('Reset these parts?').closest('div') as HTMLElement;
    expect(
      within(confirm)
        .getAllByRole('listitem')
        .map((i) => i.textContent),
    ).toEqual(['Installations', 'Backups']);
    fireEvent.click(screen.getByRole('button', { name: 'Reset' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'reset_launcher')?.args).toMatchObject({
        request: {
          payload: {
            installations: true,
            backUpInstallations: true,
            backups: true,
            settings: false,
            gameFiles: false,
          },
        },
      });
    });
    expect(
      await screen.findByText(/Reset done: 2 installations deleted \(1 backed up first\)/),
    ).toBeVisible();
    // Nothing is chosen any more, so Reset… is closed: the focus goes to what was done.
    expect(screen.getByRole('button', { name: 'Reset…' })).toBeDisabled();
    await waitFor(() => {
      expect(screen.getByText(/Reset done/).closest('[tabindex]')).toHaveFocus();
    });
  });

  it('keeps its own question while the reset makes the launcher busy', async () => {
    let open: () => void = () => undefined;
    const calls = backend(
      new Promise<void>((resolve) => {
        open = resolve;
      }),
    );
    const { rerender } = render(<ResetSettings busy={false} />);
    fireEvent.click(await screen.findByRole('checkbox', { name: /^Temporary files/ }));
    fireEvent.click(screen.getByRole('button', { name: 'Reset…' }));
    const yes = screen.getByRole('button', { name: 'Reset' });
    yes.focus();
    fireEvent.click(yes);
    await waitFor(() => {
      expect(calls.filter((c) => c.cmd === 'reset_launcher')).toHaveLength(1);
    });
    // The reset holds the launcher: the question and the focus stay, without "waits".
    rerender(<ResetSettings busy />);
    expect(screen.getByRole('alertdialog', { name: 'Reset' })).toBeVisible();
    expect(yes).toHaveFocus();
    expect(screen.getByText('Resetting…')).toBeVisible();
    expect(screen.queryByText('Reset waits until the running operation or game ends.')).toBeNull();
    // A second press while it goes sends nothing more.
    fireEvent.click(yes);
    open();
    expect(await screen.findByText(/Reset done/)).toBeVisible();
    rerender(<ResetSettings busy={false} />);
    expect(calls.filter((c) => c.cmd === 'reset_launcher')).toHaveLength(1);
    expect(screen.getByText(/Reset done/).closest('[tabindex]')).toHaveFocus();
  });

  it('waits while something runs', async () => {
    backend();
    render(<ResetSettings busy />);
    expect(await screen.findByRole('checkbox', { name: /^Installations/ })).toBeDisabled();
    expect(screen.getByText('Reset waits until the running operation or game ends.')).toBeVisible();
  });
});
