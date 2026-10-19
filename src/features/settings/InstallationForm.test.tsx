import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import { instanceFixture, snapshotFixture } from '../../test/fixtures';
import { InstallationForm } from './InstallationForm';

describe('InstallationForm', () => {
  it('saves fullscreen and Minecraft arguments with the window settings', async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === 'update_instance_settings') return instanceFixture({ fullscreen: true });
      if (cmd === 'get_app_snapshot') return snapshotFixture();
      return null;
    });
    const instance = instanceFixture({ gameArguments: '--server old.example' });
    render(<InstallationForm instance={instance} memory={snapshotFixture().memory} />);
    const args = screen.getByLabelText('Minecraft arguments');
    expect(args).toHaveValue('--server old.example');
    expect(
      screen.getByRole('checkbox', { name: 'Start the game in fullscreen' }),
    ).not.toBeChecked();
    fireEvent.click(screen.getByRole('checkbox', { name: 'Start the game in fullscreen' }));
    fireEvent.change(args, { target: { value: '--quickPlaySingleplayer "My World"' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'update_instance_settings')?.args).toMatchObject({
        request: {
          payload: {
            instanceId: instance.id,
            maxHeapMib: 4096,
            fullscreen: true,
            gameArguments: '--quickPlaySingleplayer "My World"',
          },
        },
      });
    });
  });

  it('sends a changed fullscreen with a new request ID after a lost answer', async () => {
    const saves: { requestId: string; fullscreen: boolean }[] = [];
    mockIPC((cmd, args) => {
      if (cmd === 'update_instance_settings') {
        const { request } = args as {
          request: { requestId: string; payload: { fullscreen: boolean } };
        };
        saves.push({ requestId: request.requestId, fullscreen: request.payload.fullscreen });
        // The first answer is lost on the way (not a typed error).
        if (saves.length === 1) throw new Error('connection lost');
        return instanceFixture();
      }
      if (cmd === 'get_app_snapshot') return snapshotFixture();
      return null;
    });
    render(<InstallationForm instance={instanceFixture()} memory={snapshotFixture().memory} />);
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() => {
      expect(saves).toHaveLength(1);
    });
    fireEvent.click(screen.getByRole('checkbox', { name: 'Start the game in fullscreen' }));
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() => {
      expect(saves).toHaveLength(2);
    });
    expect(saves[1]?.fullscreen).toBe(true);
    expect(saves[1]?.requestId).not.toBe(saves[0]?.requestId);
  });

  it('shows the recommendation for its mods and applies it on request', async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === 'recommend_memory') {
        return { mib: 8192, workload: { kind: 'modded', mods: 330 }, wantedMib: null };
      }
      if (cmd === 'get_app_snapshot') return snapshotFixture();
      return null;
    });
    const instance = instanceFixture();
    render(<InstallationForm instance={instance} memory={snapshotFixture().memory} />);
    expect(
      await screen.findByText('Recommended: 8192 MiB for a mod loader with 330 mods.'),
    ).toBeVisible();
    expect(calls.find((c) => c.cmd === 'recommend_memory')?.args).toEqual({
      instanceId: instance.id,
    });
    fireEvent.click(screen.getByRole('button', { name: 'Use recommended' }));
    expect(screen.getByLabelText('Maximum memory (MiB)')).toHaveValue(8192);
    expect(screen.queryByRole('button', { name: 'Use recommended' })).toBeNull();
  });
});
