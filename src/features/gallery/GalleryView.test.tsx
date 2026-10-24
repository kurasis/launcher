import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { Dialog } from '../../components/Dialog';
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

type Answers = Record<string, (args: unknown) => unknown>;

function setup(found: Gallery = gallery, answers: Answers = {}) {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    const answer = answers[cmd];
    if (answer !== undefined) return answer(args);
    if (cmd === 'list_screenshots') return found;
    if (cmd === 'get_app_snapshot') return snapshotFixture();
    return null;
  });
  render(<GalleryView snapshot={snapshotFixture({ instances: [survival, modded] })} />);
  return calls;
}

/** The screenshots folders: what is deleted is gone from the next list. With `held`, each
 * delete waits for `finish` (or `fail`). */
function disk({ held = false } = {}) {
  let found = gallery;
  const waiting: {
    drop: () => void;
    resolve: (value: null) => void;
    reject: (e: unknown) => void;
  }[] = [];
  const answers: Answers = {
    list_screenshots: () => found,
    delete_screenshot: (args) => {
      const { instanceId, file } = (args as { request: { payload: Shown } }).request.payload;
      const drop = () => {
        found = {
          ...found,
          // As the backend lists them: an installation without screenshots is left out.
          installations: found.installations
            .map((group) =>
              group.instanceId === instanceId
                ? { ...group, screenshots: group.screenshots.filter((s) => s.file !== file) }
                : group,
            )
            .filter((group) => group.screenshots.length > 0),
        };
      };
      if (!held) {
        drop();
        return null;
      }
      return new Promise((resolve, reject) => {
        waiting.push({ drop, resolve, reject });
      });
    },
  };
  return {
    answers,
    finish() {
      const next = waiting.shift();
      next?.drop();
      next?.resolve(null);
    },
    fail(error: string) {
      waiting.shift()?.reject(error);
    },
  };
}

interface Shown {
  instanceId: string;
  file: string;
}

/** Deletes the shot the viewer shows, answering its question. */
function deleteShown() {
  fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Delete' }));
  fireEvent.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Delete' }));
}

const named = (cmd: string, calls: { cmd: string; args: unknown }[]) =>
  calls.filter((c) => c.cmd === cmd);

/** Text as read: without the isolation marks, one kind of space. */
const plain = (element: Element) =>
  element.textContent.replace(/[\u2068\u2069]/gu, '').replace(/\s/gu, ' ');

/** Opens the viewer on the Modded screenshot, the first one. */
async function openFirst() {
  const modGroup = await screen.findByRole('region', { name: /Modded/ });
  fireEvent.click(within(modGroup).getByRole('button'));
  return screen.getByRole('dialog', { name: /Modded/ });
}

afterEach(() => {
  vi.useRealTimers();
  document.documentElement.dir = 'ltr';
});

describe('GalleryView', () => {
  it('groups screenshots by installation with its version', async () => {
    setup();
    const modGroup = await screen.findByRole('region', { name: /Modded/ });
    expect(
      within(modGroup).getByRole('heading', {
        name: /Modded.*, .?Minecraft 1\.20\.1 · Fabric 0\.16\.14/,
      }),
    ).toBeVisible();
    expect(within(modGroup).getByText('1 screenshot')).toBeVisible();
    expect(within(modGroup).getAllByRole('button')).toHaveLength(1);
    const survivalGroup = screen.getByRole('region', { name: /Survival/ });
    expect(
      within(survivalGroup).getByRole('heading', { name: /Survival.*, .?Minecraft 1\.21\.1/ }),
    ).toBeVisible();
    expect(within(survivalGroup).getByText('2 screenshots')).toBeVisible();
    expect(screen.getByText('3 screenshots · \u20686.0 MiB\u2069')).toBeVisible();
    const images = survivalGroup.querySelectorAll('img');
    expect(images[0]?.getAttribute('src')).toBe(
      `gallery://localhost/thumb/${survival.id}/2026-10-02_10.00.00.png?v=2026-10-02T10%3A00%3A00Z`,
    );

    fireEvent.change(screen.getByLabelText('Installation'), { target: { value: survival.id } });
    expect(screen.queryByRole('region', { name: /Modded/ })).toBeNull();
    expect(screen.getByRole('region', { name: /Survival/ })).toBeVisible();
    // The count is of what is shown.
    expect(screen.getByText('2 screenshots · \u20684.0 MiB\u2069')).toBeVisible();
  });

  it('shows one screenshot at a time and opens, reveals or deletes it', async () => {
    const calls = setup();
    const viewer = await openFirst();
    expect(within(viewer).getByText('1 of 3')).toBeVisible();
    expect(within(viewer).getByText(/1920 × 1080/)).toBeVisible();
    // At the first shot Previous does nothing, but keeps a focus it has.
    const previous = within(viewer).getByRole('button', { name: 'Previous' });
    expect(previous).toHaveAttribute('aria-disabled', 'true');
    fireEvent.click(previous);
    expect(within(viewer).getByText('1 of 3')).toBeVisible();

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
    const question = screen.getByRole('alertdialog');
    expect(within(question).getByText(/cannot be brought back/)).toBeVisible();
    expect(named('delete_screenshot', calls)).toHaveLength(0);
    fireEvent.click(within(question).getByRole('button', { name: 'Delete' }));
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

  it('leaves its keys to a dialog over it', async () => {
    setup();
    await openFirst();
    // A question that opened by itself (a download on a metered connection).
    function Question() {
      const [open, setOpen] = useState(true);
      return open ? (
        <Dialog
          title="Download?"
          onClose={() => {
            setOpen(false);
          }}
        >
          <button type="button">Download</button>
        </Dialog>
      ) : null;
    }
    render(<Question />);
    fireEvent.keyDown(document.body, { key: 'ArrowRight' });
    expect(screen.getByText('1 of 3')).toBeVisible();
    fireEvent.keyDown(document.body, { key: 'Escape' });
    expect(screen.queryByRole('dialog', { name: 'Download?' })).toBeNull();
    expect(screen.getByRole('dialog', { name: /Modded/ })).toBeVisible();
  });

  it('says when each screenshot was taken and how big it is', async () => {
    vi.useFakeTimers({ toFake: ['Date'] });
    vi.setSystemTime(new Date(2026, 9, 3, 12, 0));
    const at = (day: number) => new Date(2026, 9, day, 10, 0).toISOString();
    setup({
      installations: [
        {
          instanceId: survival.id,
          screenshots: [shot('a.png', at(3)), shot('b.png', at(2)), shot('c.png', at(1))],
        },
      ],
      totalBytes: 6 * 1024 * 1024,
    });
    const group = await screen.findByRole('region', { name: /Survival/ });
    const captions = [...group.querySelectorAll('li p')].map(plain);
    expect(captions).toEqual([
      'Today, 10:00 AM, 2.0 MiB',
      'Yesterday, 10:00 AM, 2.0 MiB',
      'October 1, 2.0 MiB',
    ]);
  });

  it('keeps the focus on Open and Show in folder while they go', async () => {
    const calls = setup(gallery, { open_screenshot: () => new Promise(() => undefined) });
    const viewer = await openFirst();
    const reveal = within(viewer).getByRole('button', { name: 'Show in folder' });
    reveal.focus();
    fireEvent.click(reveal);
    await waitFor(() => {
      expect(reveal).toHaveAttribute('aria-disabled', 'true');
    });
    expect(reveal).toHaveFocus();
    expect(reveal).toBeEnabled();
    fireEvent.click(reveal);
    fireEvent.click(within(viewer).getByRole('button', { name: 'Open in the image viewer' }));
    expect(named('open_screenshot', calls)).toHaveLength(1);
  });

  it('closes only the question on Escape and gives the focus back to Delete', async () => {
    setup();
    const viewer = await openFirst();
    fireEvent.click(within(viewer).getByRole('button', { name: 'Delete' }));
    const question = screen.getByRole('alertdialog');
    fireEvent.keyDown(question, { key: 'Escape' });
    expect(screen.queryByRole('alertdialog')).toBeNull();
    expect(screen.getByRole('dialog', { name: /Modded/ })).toBeVisible();
    await waitFor(() => {
      expect(within(viewer).getByRole('button', { name: 'Delete' })).toHaveFocus();
    });
  });

  it('gives the focus back to the thumbnail of the screenshot it showed last', async () => {
    setup();
    const viewer = await openFirst();
    fireEvent.keyDown(viewer, { key: 'ArrowRight' });
    expect(within(viewer).getByText('2 of 3')).toBeVisible();
    fireEvent.click(within(viewer).getByRole('button', { name: 'Close' }));
    expect(screen.queryByRole('dialog')).toBeNull();
    const survivalGroup = screen.getByRole('region', { name: /Survival/ });
    await waitFor(() => {
      expect(within(survivalGroup).getAllByRole('button')[0]).toHaveFocus();
    });
  });

  it('moves along the reading direction and leaves keys typed in a field alone', async () => {
    setup();
    document.documentElement.dir = 'rtl';
    const viewer = await openFirst();
    fireEvent.keyDown(viewer, { key: 'ArrowLeft' });
    expect(within(viewer).getByText('2 of 3')).toBeVisible();
    fireEvent.keyDown(viewer, { key: 'ArrowRight' });
    expect(within(viewer).getByText('1 of 3')).toBeVisible();
    // A field outside the viewer: the Ctrl K palette's.
    const field = document.createElement('input');
    document.body.append(field);
    try {
      fireEvent.keyDown(field, { key: 'ArrowLeft' });
      expect(within(viewer).getByText('1 of 3')).toBeVisible();
    } finally {
      field.remove();
    }
  });

  it('stays closed when it is closed while a delete goes', async () => {
    let finish: (value: null) => void = () => undefined;
    setup(gallery, {
      delete_screenshot: () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    });
    const viewer = await openFirst();
    fireEvent.click(within(viewer).getByRole('button', { name: 'Delete' }));
    fireEvent.click(
      within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Delete' }),
    );
    fireEvent.keyDown(document.body, { key: 'Escape' });
    expect(screen.queryByRole('dialog')).toBeNull();
    finish(null);
    expect(await screen.findByText('Screenshot deleted.')).toBeVisible();
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('looks for screenshots while a game runs and again when it ends', async () => {
    const calls = setup();
    const running = snapshotFixture({
      instances: [survival, modded],
      activeOperation: { kind: 'launch', instanceId: survival.id, operationId: 'op-9' },
    });
    const { rerender } = render(<GalleryView snapshot={running} />);
    await waitFor(() => {
      expect(screen.getAllByRole('region', { name: /Survival/ })).toHaveLength(2);
    });
    const before = named('list_screenshots', calls).length;
    rerender(<GalleryView snapshot={snapshotFixture({ instances: [survival, modded] })} />);
    await waitFor(() => {
      expect(named('list_screenshots', calls)).toHaveLength(before + 1);
    });
  });

  it('gives the focus to the title when the only shot shown is deleted', async () => {
    const files = disk();
    // The list after the delete comes when the test says, so the focus is seen before it.
    let listed = 0;
    let release: () => void = () => undefined;
    const list = files.answers.list_screenshots;
    setup(gallery, {
      ...files.answers,
      list_screenshots: (args) =>
        ++listed === 1
          ? list?.(args)
          : new Promise((resolve) => {
              release = () => {
                resolve(list?.(args));
              };
            }),
    });
    await screen.findByRole('region', { name: /Modded/ });
    fireEvent.change(screen.getByLabelText('Installation'), { target: { value: modded.id } });
    await openFirst();
    deleteShown();
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });
    // Not the thumbnail of the shot deleted, which goes with the list.
    const title = screen.getByRole('heading', { name: 'Screenshots' });
    expect(title).toHaveFocus();
    release();
    expect(await screen.findByText('This installation has no screenshots yet.')).toBeVisible();
    expect(title).toHaveFocus();
  });

  it('hands the focus on to the next thumbnail when closed while a delete goes', async () => {
    const files = disk({ held: true });
    setup(gallery, files.answers);
    const viewer = await openFirst();
    fireEvent.keyDown(viewer, { key: 'ArrowRight' });
    deleteShown();
    fireEvent.keyDown(document.body, { key: 'Escape' });
    expect(screen.queryByRole('dialog')).toBeNull();
    files.finish();
    const survivalGroup = screen.getByRole('region', { name: /Survival/ });
    await waitFor(() => {
      expect(within(survivalGroup).getAllByRole('button')).toHaveLength(1);
    });
    await waitFor(() => {
      expect(within(survivalGroup).getByRole('button')).toHaveFocus();
    });
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('keeps the shot of a viewer opened again while a delete went', async () => {
    const files = disk({ held: true });
    setup(gallery, files.answers);
    const viewer = await openFirst();
    fireEvent.keyDown(viewer, { key: 'ArrowRight' });
    deleteShown();
    fireEvent.keyDown(document.body, { key: 'Escape' });
    await openFirst();
    files.finish();
    const reopened = screen.getByRole('dialog', { name: /Modded/ });
    expect(await within(reopened).findByText('1 of 2')).toBeVisible();
    expect(within(reopened).getByRole('status')).toHaveTextContent('Screenshot deleted.');
  });

  it('says on the page that a delete failed after the viewer closed', async () => {
    const files = disk({ held: true });
    setup(gallery, files.answers);
    await openFirst();
    deleteShown();
    fireEvent.keyDown(document.body, { key: 'Escape' });
    files.fail('The file is in use.');
    expect(await screen.findByRole('alert')).toHaveTextContent('The file is in use.');
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(screen.getAllByRole('button', { name: /Open the screenshot/ })).toHaveLength(3);
  });

  it('says a second delete again, and not once more on the page after closing', async () => {
    const files = disk({ held: true });
    setup(gallery, files.answers);
    await openFirst();
    deleteShown();
    files.finish();
    const status = () => within(screen.getByRole('dialog')).getByRole('status');
    await waitFor(() => {
      expect(status()).toHaveTextContent('Screenshot deleted.');
    });
    expect(screen.getByRole('dialog', { name: /Survival/ })).toBeVisible();
    // Emptied while the next one goes, so the same words are read out again.
    deleteShown();
    expect(status()).toHaveTextContent('');
    files.finish();
    await waitFor(() => {
      expect(status()).toHaveTextContent('Screenshot deleted.');
    });
    fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Close' }));
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(screen.queryByText('Screenshot deleted.')).toBeNull();
  });

  it('forgets what the buttons of another shot did', async () => {
    setup(gallery, {
      open_screenshot: () => {
        throw new Error('No program opens this file.');
      },
    });
    const viewer = await openFirst();
    fireEvent.click(within(viewer).getByRole('button', { name: 'Open in the image viewer' }));
    expect(await within(viewer).findByRole('alert')).toHaveTextContent(
      'No program opens this file.',
    );
    fireEvent.keyDown(viewer, { key: 'ArrowRight' });
    expect(within(viewer).getByText('2 of 3')).toBeVisible();
    expect(within(viewer).queryByRole('alert')).toBeNull();
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
