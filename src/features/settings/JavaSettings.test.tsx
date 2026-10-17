import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import { instanceFixture, snapshotFixture } from '../../test/fixtures';
import { JavaSettings } from './JavaSettings';

describe('JavaSettings', () => {
  it('switches the runtime source through the backend and lists installed Java', async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === 'set_app_settings') return snapshotFixture().settings;
      if (cmd === 'get_app_snapshot') return snapshotFixture();
      return null;
    });
    const settings = snapshotFixture().settings;
    const instances = [
      instanceFixture({
        displayName: 'Old world',
        java: { provider: 'mojang', major: 8, release: '8u202' },
      }),
      instanceFixture({ id: '1f1e2d3c-4b5a-4978-8796-a5b4c3d2e1f0', displayName: 'Fresh' }),
    ];
    render(<JavaSettings settings={settings} instances={instances} />);
    expect(screen.getByRole('radio', { name: /Mojang \(recommended\)/ })).toBeChecked();
    expect(screen.getByText('Mojang · Java 8 (8u202)')).toBeVisible();
    expect(screen.getByText('Chosen when it is installed')).toBeVisible();
    fireEvent.click(screen.getByRole('radio', { name: /Eclipse Temurin/ }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'set_app_settings')?.args).toMatchObject({
        request: { payload: { javaProvider: 'temurin' } },
      });
    });
  });
});
