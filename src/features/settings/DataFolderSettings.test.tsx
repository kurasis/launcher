import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import type { DataLocation } from '../../lib/contracts';
import { DataFolderSettings } from './DataFolderSettings';

const location: DataLocation = {
  mode: 'default',
  path: '/home/test/.local/share/launcher',
  defaultPath: '/home/test/.local/share/launcher',
  portablePath: '/opt/launcher/launcher-data',
  pendingPath: null,
  pendingReused: false,
};

describe('DataFolderSettings', () => {
  it('copies the data to a chosen folder after confirmation', async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === 'get_data_location') return location;
      if (cmd === 'pick_data_folder') return '/mnt/games/launcher';
      if (cmd === 'move_data_location') return { ...location, pendingPath: '/mnt/games/launcher' };
      return null;
    });
    render(<DataFolderSettings busy={false} />);
    expect(await screen.findByText(location.path)).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Move back to the default folder' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Move to another folder…' }));
    expect(await screen.findByText('/mnt/games/launcher')).toBeVisible();
    expect(calls.some((c) => c.cmd === 'move_data_location')).toBe(false);
    fireEvent.click(screen.getByRole('button', { name: 'Copy data' }));
    expect(await screen.findByText(/Quit and start the launcher again/)).toBeVisible();
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'move_data_location')?.args).toMatchObject({
        request: { payload: { target: 'custom', path: '/mnt/games/launcher' } },
      });
    });
    expect(screen.queryByRole('button', { name: 'Make portable' })).toBeNull();
  });

  it('cannot move while something runs', async () => {
    mockIPC((cmd) => (cmd === 'get_data_location' ? location : null));
    render(<DataFolderSettings busy />);
    expect(await screen.findByRole('button', { name: 'Make portable' })).toBeDisabled();
    expect(screen.getByText(/Wait until the running operation/)).toBeVisible();
  });
});
