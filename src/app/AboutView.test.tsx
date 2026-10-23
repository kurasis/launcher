import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it, vi } from 'vitest';
import type { AppSnapshot, UpdateView } from '../lib/contracts';
import { onOpenReview } from '../features/reviews/openReview';
import { appInfoFixture, snapshotFixture } from '../test/fixtures';
import { AboutView } from './AboutView';

function backend(answers: Record<string, () => unknown> = {}) {
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
    const answer = answers[cmd];
    if (answer !== undefined) return answer();
    if (cmd === 'get_app_info') return appInfoFixture;
    return null;
  });
  return calls;
}

function withUpdate(update: Partial<UpdateView>): AppSnapshot {
  const snapshot = snapshotFixture();
  return { ...snapshot, update: { ...snapshot.update, ...update } };
}

describe('AboutView', () => {
  it('heads the screen, names the launcher with its version and keeps the links', async () => {
    const calls = backend();
    render(<AboutView snapshot={snapshotFixture()} />);
    expect(screen.getByRole('heading', { level: 2, name: 'About' })).toBeVisible();
    const brand = screen.getByRole('region', { name: 'Sferum Launcher' });
    expect(within(brand).getByRole('heading', { level: 3 })).toHaveTextContent('Sferum Launcher');
    expect(await within(brand).findByText('Version 0.1.0, debug build')).toBeVisible();
    expect(
      screen.getAllByRole('heading', { level: 3 }).map((heading) => heading.textContent),
    ).toEqual(['Sferum Launcher', 'Official Minecraft information', 'Launcher core']);
    // The idle update has no panel.
    expect(screen.queryByText(/What's new/)).toBeNull();
    fireEvent.click(within(brand).getByRole('button', { name: 'Source code' }));
    await waitFor(() => {
      expect(calls).toContain('open_external');
    });
  });

  it('opens the review screen', () => {
    backend();
    const seen = vi.fn();
    const stop = onOpenReview(seen);
    render(<AboutView snapshot={snapshotFixture()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Leave a review' }));
    expect(seen).toHaveBeenCalledTimes(1);
    stop();
  });

  it('works while the core is not there yet, without the update or a review', async () => {
    backend();
    render(<AboutView snapshot={null} />);
    expect(screen.queryByRole('button', { name: 'Leave a review' })).toBeNull();
    expect(screen.getByRole('button', { name: 'Source code' })).toBeVisible();
    expect(await screen.findByText('linux / x86_64')).toBeVisible();
  });

  it('lists the notes of the waiting version and restarts on a press', async () => {
    const calls = backend({
      restart_to_update: () => {
        // The core refuses while a game or an install runs.
        // eslint-disable-next-line @typescript-eslint/only-throw-error
        throw {
          code: 'APP_BUSY',
          message: 'Wait until the game or the install ends.',
          retryable: true,
          diagnosticId: 'd',
        };
      },
    });
    render(
      <AboutView
        snapshot={withUpdate({
          phase: 'ready',
          version: '0.2.0',
          notes: { en: '- Search with Ctrl K\n- Worlds reconnect\n\n<img src=x onerror=alert(1)>' },
        })}
      />,
    );
    const panel = screen.getByRole('region', { name: "What's new in version 0.2.0" });
    const notes = within(panel).getByRole('region', { name: 'What’s new in this version' });
    expect(notes).toHaveAttribute('tabindex', '0');
    expect(
      within(notes)
        .getAllByRole('listitem')
        .map((item) => item.textContent),
    ).toEqual(['Search with Ctrl K', 'Worlds reconnect', '<img src=x onerror=alert(1)>']);
    expect(document.querySelector('img[src="x"]')).toBeNull();
    expect(
      within(panel).getByText('Downloaded; installs when you close the launcher.'),
    ).toBeVisible();
    const restart = within(panel).getByRole('button', { name: 'Restart now' });
    // Never the first thing focused: it closes the launcher.
    expect(restart).not.toHaveFocus();
    fireEvent.click(restart);
    expect(await within(panel).findByRole('alert')).toHaveTextContent(/game or the install ends/);
    expect(calls.filter((cmd) => cmd === 'restart_to_update')).toHaveLength(1);
  });

  it('shows the notes of the version now running until they are read', () => {
    backend();
    render(
      <AboutView
        snapshot={withUpdate({
          phase: 'idle',
          version: null,
          whatsNew: { version: '0.1.9', notes: {} },
        })}
      />,
    );
    const panel = screen.getByRole('region', { name: "What's new in version 0.1.9" });
    expect(panel).toHaveTextContent('No notes for this version.');
    expect(within(panel).queryByRole('button')).toBeNull();
  });

  it('downloads an available version and shows the download going', () => {
    const calls = backend();
    const { rerender } = render(
      <AboutView snapshot={withUpdate({ phase: 'available', version: '0.2.0', notes: {} })} />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Download' }));
    expect(calls).toContain('download_update');
    rerender(
      <AboutView
        snapshot={withUpdate({
          phase: 'downloading',
          version: '0.2.0',
          notes: {},
          downloaded: 50,
          total: 200,
        })}
      />,
    );
    expect(screen.getByText('Downloading… 25%')).toBeVisible();
    expect(screen.getByRole('progressbar', { name: 'Downloading…' })).toHaveAttribute('max', '200');
  });
});
