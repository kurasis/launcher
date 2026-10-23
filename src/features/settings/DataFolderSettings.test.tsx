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
    let copied: () => void = () => undefined;
    const copy = new Promise<void>((done) => {
      copied = done;
    });
    mockIPC(async (cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === 'get_data_location') return location;
      if (cmd === 'pick_data_folder') return '/mnt/games/launcher';
      if (cmd === 'move_data_location') {
        await copy;
        return { ...location, pendingPath: '/mnt/games/launcher' };
      }
      return null;
    });
    render(<DataFolderSettings busy={false} />);
    expect(await screen.findByText(location.path)).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Move back to the default folder' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Move to another folder…' }));
    expect(await screen.findByText('/mnt/games/launcher')).toBeVisible();
    expect(calls.some((c) => c.cmd === 'move_data_location')).toBe(false);
    const yes = screen.getByRole('button', { name: 'Copy data' });
    yes.focus();
    fireEvent.click(yes);
    // The question goes with the button pressed: the focus goes to what is happening.
    expect(screen.getByRole('status')).toHaveTextContent('Copying the data…');
    expect(screen.getByRole('status')).toHaveFocus();
    copied();
    expect(await screen.findByText(/Quit and start the launcher again/)).toBeVisible();
    await waitFor(() => {
      expect(
        screen.getByText(/Quit and start the launcher again/).closest('[tabindex]'),
      ).toHaveFocus();
    });
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'move_data_location')?.args).toMatchObject({
        request: { payload: { target: 'custom', path: '/mnt/games/launcher' } },
      });
    });
    expect(screen.queryByRole('button', { name: 'Make portable' })).toBeNull();
  });

  it('says why the folder picker failed', async () => {
    mockIPC((cmd) => {
      if (cmd === 'get_data_location') return location;
      if (cmd === 'pick_data_folder') {
        // The Rust core rejects with a plain error envelope, not an Error instance.
        // eslint-disable-next-line @typescript-eslint/only-throw-error
        throw { code: 'INTERNAL', message: 'The folder dialog could not open.', diagnosticId: 'd' };
      }
      return null;
    });
    render(<DataFolderSettings busy={false} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Move to another folder…' }));
    expect(await screen.findByText('The folder dialog could not open.')).toBeVisible();
  });

  it('cannot move while something runs', async () => {
    mockIPC((cmd) => (cmd === 'get_data_location' ? location : null));
    render(<DataFolderSettings busy />);
    expect(await screen.findByRole('button', { name: 'Make portable' })).toBeDisabled();
    expect(screen.getByText(/Wait until the running operation/)).toBeVisible();
  });

  it('says it is loading, then asks in place and gives the focus back on Cancel', async () => {
    let answer: (value: DataLocation) => void = () => undefined;
    mockIPC((cmd) => {
      if (cmd === 'get_data_location') {
        return new Promise<DataLocation>((done) => {
          answer = done;
        });
      }
      return null;
    });
    render(<DataFolderSettings busy={false} />);
    expect(screen.getByRole('heading', { level: 3, name: 'Launcher data location' })).toBeVisible();
    expect(screen.getByRole('status')).toHaveTextContent('Loading…');
    answer(location);
    const portable = await screen.findByRole('button', { name: 'Make portable' });
    // The mode first, then the folder.
    const mode = screen.getByText('The per-user folder the operating system provides.');
    expect(mode.compareDocumentPosition(screen.getByText(location.path))).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    portable.focus();
    fireEvent.click(portable);
    const question = screen.getByRole('alertdialog', { name: 'Copy data' });
    expect(question).toHaveFocus();
    expect(question).toHaveTextContent('/opt/launcher/launcher-data');
    fireEvent.keyDown(question, { key: 'Escape' });
    expect(screen.queryByRole('alertdialog')).toBeNull();
    expect(screen.getByRole('button', { name: 'Make portable' })).toHaveFocus();
  });
});
