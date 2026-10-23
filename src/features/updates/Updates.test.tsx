import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { afterEach, describe, expect, it } from 'vitest';
import type { UpdateView } from '../../lib/contracts';
import { loadLanguage, setLanguage } from '../../lib/strings';
import { snapshotFixture } from '../../test/fixtures';
import { notesFor } from './notes';
import { UpdateBadge } from './UpdateBadge';
import { UpdateSettings } from './UpdateSettings';
import { WhatsNew } from './WhatsNew';

const view = (overrides: Partial<UpdateView>): UpdateView => ({
  ...snapshotFixture().update,
  version: '0.2.0',
  notes: { en: 'Faster downloads', ru: 'Быстрее загрузки' },
  ...overrides,
});

function calls() {
  const seen: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    seen.push({ cmd, args });
    if (cmd === 'check_for_update' || cmd === 'download_update') return view({ phase: 'idle' });
    if (cmd === 'get_app_snapshot') return snapshotFixture();
    return null;
  });
  return seen;
}

describe('launcher updates', () => {
  afterEach(() => {
    setLanguage('en');
  });

  it('stays out of the way until there is an update', () => {
    const { container } = render(<UpdateBadge update={view({ phase: 'idle' })} />);
    expect(container).toBeEmptyDOMElement();
  });

  it('offers a restart when the update is ready, or waits for the close', async () => {
    const seen = calls();
    render(<UpdateBadge update={view({ phase: 'ready', downloaded: 10, total: 10 })} />);
    fireEvent.click(screen.getByRole('button', { name: 'Version 0.2.0 is ready' }));
    const dialog = screen.getByRole('dialog', { name: 'Launcher 0.2.0' });
    expect(dialog).toHaveTextContent('What’s new: Faster downloads');
    expect(dialog).toHaveTextContent('installs when you close the launcher');
    // The safe choice takes the focus: Enter never restarts by accident.
    expect(screen.getByRole('button', { name: 'When I close the launcher' })).toHaveFocus();
    fireEvent.click(screen.getByRole('button', { name: 'Restart now' }));
    await waitFor(() => {
      expect(seen.some((c) => c.cmd === 'restart_to_update')).toBe(true);
    });
  });

  it('downloads on a press, links .deb installs, and says why a step failed', async () => {
    const seen = calls();
    const { unmount } = render(
      <UpdateBadge update={view({ phase: 'available', mandatory: true })} />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Version 0.2.0 is available' }));
    expect(screen.getByText('This update is required.')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Download' }));
    await waitFor(() => {
      expect(seen.some((c) => c.cmd === 'download_update')).toBe(true);
    });
    unmount();

    render(
      <UpdateBadge
        update={view({ phase: 'notice', downloadUrl: 'https://upd.lapi.sh/releases/0.2.0/x.deb' })}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Version 0.2.0 is available' }));
    expect(screen.getByText(/updated by your system's package manager/)).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Download the new version' }));
    await waitFor(() => {
      expect(seen.some((c) => c.cmd === 'open_update_page')).toBe(true);
    });
  });

  it('explains a refused signature in the interface language', async () => {
    calls();
    setLanguage(await loadLanguage('ru'));
    render(<UpdateBadge update={view({ phase: 'failed', error: 'signature' })} />);
    fireEvent.click(screen.getByRole('button', { name: 'Обновление не удалось' }));
    expect(
      screen.getByText('Подпись обновления не совпала, поэтому оно не установлено.'),
    ).toBeVisible();
    expect(screen.getByText('Быстрее загрузки')).toBeVisible();
    expect(notesFor({ en: 'A', ru: 'Б' })).toBe('Б');
    expect(notesFor({ en: 'A' })).toBe('A');
    expect(notesFor({})).toBeNull();
  });

  it('shows what is new once and forgets it on "Got it"', async () => {
    const seen = calls();
    render(<WhatsNew update={view({ whatsNew: { version: '0.2.0', notes: { en: 'New' } } })} />);
    expect(screen.getByText("What's new in version 0.2.0")).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Got it' }));
    await waitFor(() => {
      expect(seen.some((c) => c.cmd === 'dismiss_whats_new')).toBe(true);
    });
  });

  it('lets the player choose the update mode and check now', async () => {
    const seen = calls();
    const snapshot = snapshotFixture();
    render(<UpdateSettings snapshot={snapshot} />);
    expect(screen.getByRole('radio', { name: /Download and install/ })).toBeChecked();
    fireEvent.click(screen.getByRole('radio', { name: 'Only tell me' }));
    await waitFor(() => {
      expect(seen.find((c) => c.cmd === 'set_app_settings')?.args).toMatchObject({
        request: { payload: { updateMode: 'notify' } },
      });
    });
    fireEvent.click(screen.getByRole('button', { name: 'Check now' }));
    expect(await screen.findByText('You have the newest version.')).toBeVisible();
  });

  it('switches to beta and looks there at once', async () => {
    const seen = calls();
    const snapshot = snapshotFixture();
    const { rerender } = render(<UpdateSettings snapshot={snapshot} />);
    expect(screen.getByRole('radio', { name: 'Stable' })).toBeChecked();
    fireEvent.click(screen.getByRole('radio', { name: /Beta/ }));
    await waitFor(() => {
      expect(seen.find((c) => c.cmd === 'set_app_settings')?.args).toMatchObject({
        request: { payload: { updateChannel: 'beta' } },
      });
    });
    await waitFor(() => {
      expect(seen.some((c) => c.cmd === 'check_for_update')).toBe(true);
    });
    rerender(
      <UpdateSettings
        snapshot={{ ...snapshot, settings: { ...snapshot.settings, updateChannel: 'beta' } }}
      />,
    );
    expect(screen.getByText(/may have problems/)).toBeVisible();
    fireEvent.click(screen.getByRole('radio', { name: 'Stable' }));
    rerender(<UpdateSettings snapshot={snapshot} />);
    expect(screen.getByText(/you keep this version until a newer stable one/)).toBeVisible();
  });

  it('tells the player the launcher went back, and forgets it on "Got it"', async () => {
    const seen = calls();
    render(<WhatsNew update={view({ rolledBack: { failed: '0.3.0', version: '0.2.0' } })} />);
    expect(screen.getByText(/Version 0.3.0 did not start properly/)).toHaveTextContent(
      'went back to version 0.2.0',
    );
    fireEvent.click(screen.getByRole('button', { name: 'Got it' }));
    await waitFor(() => {
      expect(seen.some((c) => c.cmd === 'dismiss_whats_new')).toBe(true);
    });
  });

  it('says where the update stands, and checks even when settings are read-only', async () => {
    const seen = calls();
    const snapshot = snapshotFixture();
    render(
      <UpdateSettings
        snapshot={{
          ...snapshot,
          appVersion: '0.1.9',
          settings: { ...snapshot.settings, writable: false },
          update: view({ phase: 'ready', version: '0.2.0' }),
        }}
      />,
    );
    expect(screen.getByRole('heading', { level: 3, name: 'Updates' })).toBeVisible();
    const status = screen.getByText(
      'This is version 0.1.9. Version 0.2.0 is downloaded and installs when you close the launcher.',
    );
    expect(status).toBeVisible();
    // A version found by a check is read out.
    expect(status).toHaveAttribute('role', 'status');
    // The choices cannot be written; looking for a version still works.
    expect(screen.getByRole('radio', { name: 'Only tell me' })).toBeDisabled();
    const check = screen.getByRole('button', { name: 'Check now' });
    expect(check).toBeEnabled();
    check.focus();
    fireEvent.click(check);
    // Checking keeps the button and its focus.
    expect(screen.getByRole('button', { name: 'Checking…' })).toHaveAttribute(
      'aria-disabled',
      'true',
    );
    expect(screen.getByRole('button', { name: 'Checking…' })).toHaveFocus();
    expect(screen.getByRole('button', { name: 'Checking…' })).toBeEnabled();
    // A second press while it looks asks nothing more.
    fireEvent.click(screen.getByRole('button', { name: 'Checking…' }));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Check now' })).toBeVisible();
    });
    expect(seen.filter((c) => c.cmd === 'check_for_update')).toHaveLength(1);
  });

  it('says when a build does not look for updates', () => {
    const snapshot = snapshotFixture();
    render(
      <UpdateSettings
        snapshot={{ ...snapshot, update: { ...snapshot.update, configured: false } }}
      />,
    );
    expect(screen.getByText('This build does not look for updates.')).toBeVisible();
  });
});
