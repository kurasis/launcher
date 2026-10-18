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
});
