import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import { snapshotFixture } from '../../test/fixtures';
import { DownloadSourceSettings } from './DownloadSourceSettings';
import { sourceNames } from './sourceNames';

describe('DownloadSourceSettings', () => {
  it('switches the download source and says which sites the mirror stands in for', async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === 'get_app_snapshot') return snapshotFixture();
      return null;
    });
    render(
      <DownloadSourceSettings
        settings={snapshotFixture().settings}
        mirroredHosts={[
          'maven.minecraftforge.net',
          'piston-data.mojang.com',
          'libraries.minecraft.net',
        ]}
      />,
    );
    expect(screen.getByRole('radio', { name: /Automatic/ })).toBeChecked();
    expect(
      screen.getByText("Not reachable now: Forge, Mojang. Downloading from the launcher's mirror."),
    ).toBeVisible();
    fireEvent.click(screen.getByRole('radio', { name: /Mirror first/ }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'set_app_settings')?.args).toMatchObject({
        request: { payload: { downloadSource: 'mirror_first' } },
      });
    });
  });

  it('names each site once', () => {
    expect(
      sourceNames([
        'api.adoptium.net',
        'github.com',
        'meta.fabricmc.net',
        'maven.fabricmc.net',
        'maven.quiltmc.org',
        'maven.neoforged.net',
        'example.org',
      ]),
    ).toEqual(['Adoptium (Java)', 'Fabric', 'Quilt', 'NeoForge', 'example.org']);
  });
});
