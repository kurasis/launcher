import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import { snapshotFixture } from '../../test/fixtures';
import { DefaultsForm } from './DefaultsForm';

describe('DefaultsForm', () => {
  it('saves defaults for new installations through the backend', async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === 'set_app_settings') return snapshotFixture().settings;
      if (cmd === 'get_app_snapshot') return snapshotFixture();
      return null;
    });
    const snapshot = snapshotFixture();
    render(<DefaultsForm settings={snapshot.settings} memory={snapshot.memory} />);
    // Recommended per kind of installation until a fixed amount is chosen.
    expect(
      screen.getByRole('radio', { name: 'Recommended for each kind of installation' }),
    ).toBeChecked();
    expect(screen.queryByLabelText('Maximum memory (MiB)')).toBeNull();
    fireEvent.click(screen.getByRole('radio', { name: 'The same for every new installation' }));
    fireEvent.change(screen.getByLabelText('Maximum memory (MiB)'), { target: { value: '6144' } });
    fireEvent.click(screen.getByRole('checkbox', { name: 'Start the game in fullscreen' }));
    fireEvent.click(screen.getByRole('button', { name: 'Save defaults' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'set_app_settings')?.args).toMatchObject({
        request: {
          payload: {
            defaultMaxHeapMib: 6144,
            defaultResolution: { width: 1280, height: 720 },
            defaultFullscreen: true,
          },
        },
      });
    });
    expect(await screen.findByText('Saved.')).toBeVisible();
  });

  it('goes back to the recommendation per kind of installation', async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === 'set_app_settings') return snapshotFixture().settings;
      if (cmd === 'get_app_snapshot') return snapshotFixture();
      return null;
    });
    const snapshot = snapshotFixture();
    render(
      <DefaultsForm
        settings={{ ...snapshot.settings, defaultMaxHeapMib: 5120 }}
        memory={snapshot.memory}
      />,
    );
    expect(screen.getByLabelText('Maximum memory (MiB)')).toHaveValue(5120);
    fireEvent.click(
      screen.getByRole('radio', { name: 'Recommended for each kind of installation' }),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Save defaults' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'set_app_settings')?.args).toMatchObject({
        request: { payload: { defaultMaxHeapMib: 0 } },
      });
    });
  });
});
