import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import type { Gallery, Screenshot } from '../../lib/contracts';
import { instanceFixture, snapshotFixture } from '../../test/fixtures';
import { GalleryView } from './GalleryView';
import { galleryUrl } from './galleryUrl';

const survival = instanceFixture({ displayName: 'Survival' });
const modded = instanceFixture({
  id: '1f1e2d3c-4b5a-4978-8796-a5b4c3d2e1f0',
  displayName: 'Modded',
  versionId: '1.20.1',
  loader: { kind: 'fabric', version: '0.16.14' },
});

function shot(file: string, takenAt: string): Screenshot {
  return { file, takenAt, bytes: 2 * 1024 * 1024, width: 1920, height: 1080 };
}

const gallery: Gallery = {
  installations: [
    {
      instanceId: modded.id,
      screenshots: [shot('2026-10-03_10.00.00.png', '2026-10-03T10:00:00Z')],
    },
    {
      instanceId: survival.id,
      screenshots: [
        shot('2026-10-02_10.00.00.png', '2026-10-02T10:00:00Z'),
        shot('2026-10-01_10.00.00.png', '2026-10-01T10:00:00Z'),
      ],
    },
  ],
  totalBytes: 6 * 1024 * 1024,
};

function setup(found: Gallery = gallery) {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === 'list_screenshots') return found;
    if (cmd === 'get_app_snapshot') return snapshotFixture();
    return null;
  });
  render(<GalleryView snapshot={snapshotFixture({ instances: [survival, modded] })} />);
  return calls;
}

const named = (cmd: string, calls: { cmd: string; args: unknown }[]) =>
  calls.filter((c) => c.cmd === cmd);

describe('GalleryView', () => {
  it('groups screenshots by installation with its version', async () => {
    setup();
    const modGroup = await screen.findByRole('region', { name: 'Modded' });
    expect(within(modGroup).getByText('Minecraft 1.20.1 · Fabric 0.16.14')).toBeVisible();
    expect(within(modGroup).getByText('1 screenshot')).toBeVisible();
    expect(within(modGroup).getAllByRole('button')).toHaveLength(1);
    const survivalGroup = screen.getByRole('region', { name: 'Survival' });
    expect(within(survivalGroup).getByText('Minecraft 1.21.1')).toBeVisible();
    expect(within(survivalGroup).getByText('2 screenshots')).toBeVisible();
    expect(screen.getByText('3 screenshots · 6.0 MiB')).toBeVisible();
    const images = survivalGroup.querySelectorAll('img');
    expect(images[0]?.getAttribute('src')).toBe(
      `gallery://localhost/thumb/${survival.id}/2026-10-02_10.00.00.png?v=2026-10-02T10%3A00%3A00Z`,
    );

    fireEvent.change(screen.getByLabelText('Installation'), { target: { value: survival.id } });
    expect(screen.queryByRole('region', { name: 'Modded' })).toBeNull();
    expect(screen.getByRole('region', { name: 'Survival' })).toBeVisible();
  });

  it('shows one screenshot at a time and opens, reveals or deletes it', async () => {
    const calls = setup();
    const modGroup = await screen.findByRole('region', { name: 'Modded' });
    fireEvent.click(within(modGroup).getByRole('button'));
    const viewer = screen.getByRole('dialog', { name: /Modded/ });
    expect(within(viewer).getByText('1 of 3')).toBeVisible();
    expect(within(viewer).getByText(/1920 × 1080/)).toBeVisible();
    expect(within(viewer).getByRole('button', { name: 'Previous' })).toBeDisabled();

    fireEvent.keyDown(viewer, { key: 'ArrowRight' });
    expect(screen.getByRole('dialog', { name: /Survival/ })).toBeVisible();
    expect(screen.getByText('2 of 3')).toBeVisible();

    fireEvent.click(screen.getByRole('button', { name: 'Show in folder' }));
    await waitFor(() => {
      expect(named('open_screenshot', calls)[0]?.args).toMatchObject({
        request: {
          payload: { instanceId: survival.id, file: '2026-10-02_10.00.00.png', reveal: true },
        },
      });
    });
    fireEvent.click(screen.getByRole('button', { name: 'Open in the image viewer' }));
    await waitFor(() => {
      expect(named('open_screenshot', calls)[1]?.args).toMatchObject({
        request: { payload: { reveal: false } },
      });
    });

    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    expect(screen.getByText(/cannot be brought back/)).toBeVisible();
    expect(named('delete_screenshot', calls)).toHaveLength(0);
    fireEvent.click(screen.getAllByRole('button', { name: 'Delete' })[0] as HTMLElement);
    await waitFor(() => {
      expect(named('delete_screenshot', calls)[0]?.args).toMatchObject({
        request: { payload: { instanceId: survival.id, file: '2026-10-02_10.00.00.png' } },
      });
    });
    expect(await screen.findByText('Screenshot deleted.')).toBeVisible();
    // The next screenshot is shown after a delete.
    expect(screen.getByText(/2026-10-01_10\.00\.00\.png/)).toBeVisible();

    // Keys work without focus inside the viewer: the focused button is gone.
    fireEvent.keyDown(document.body, { key: 'Escape' });
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('explains how to take a screenshot when there are none', async () => {
    setup({ installations: [], totalBytes: 0 });
    expect(
      await screen.findByText('No screenshots yet. Press F2 in the game to take one.'),
    ).toBeVisible();
  });

  it('builds the Windows address of the protocol', () => {
    const agent = navigator.userAgent;
    Object.defineProperty(navigator, 'userAgent', {
      value: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)',
      configurable: true,
    });
    try {
      expect(galleryUrl('full', survival.id, shot('a b.png', 't'))).toBe(
        `http://gallery.localhost/full/${survival.id}/a%20b.png?v=t`,
      );
    } finally {
      Object.defineProperty(navigator, 'userAgent', { value: agent, configurable: true });
    }
  });
});
