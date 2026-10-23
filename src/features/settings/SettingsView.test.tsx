import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { useState } from 'react';
import { describe, expect, it } from 'vitest';
import type { AppSnapshot } from '../../lib/contracts';
import type { SettingsSection } from '../../lib/strings';
import { snapshotFixture } from '../../test/fixtures';
import { SettingsView } from './SettingsView';

function backend() {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === 'get_data_location') {
      return {
        mode: 'default',
        path: '/home/test/.local/share/launcher',
        defaultPath: '/home/test/.local/share/launcher',
        portablePath: null,
        pendingPath: null,
        pendingReused: false,
      };
    }
    if (cmd === 'get_app_snapshot') return snapshotFixture();
    if (cmd === 'set_app_settings') return snapshotFixture().settings;
    return null;
  });
  return calls;
}

function Shown({ snapshot }: { snapshot: AppSnapshot }) {
  const [section, setSection] = useState<SettingsSection>('general');
  return <SettingsView snapshot={snapshot} section={section} onSection={setSection} />;
}

describe('SettingsView', () => {
  it('heads the screen and shows the general panels in the board order', async () => {
    backend();
    render(<Shown snapshot={snapshotFixture()} />);
    expect(screen.getByRole('heading', { level: 2, name: 'Settings' })).toBeVisible();
    const general = screen.getByRole('tabpanel', { name: 'General' });
    expect(
      within(general)
        .getAllByRole('heading', { level: 3 })
        .map((heading) => heading.textContent),
    ).toEqual(['Launcher data location', 'Updates']);
    // While a game runs: unload and come back, stay open, close the launcher.
    const during = within(general).getByRole('group', { name: 'While a game is running' });
    expect(
      within(during)
        .getAllByRole('radio')
        .map((radio) => radio.getAttribute('value')),
    ).toEqual(['unload_and_return', 'keep_open', 'close_launcher']);
    expect(await within(general).findByText('/home/test/.local/share/launcher')).toBeVisible();
  });

  it('chooses the theme from the segmented control', async () => {
    const calls = backend();
    render(<Shown snapshot={snapshotFixture()} />);
    const theme = screen.getByRole('radiogroup', { name: 'Theme' });
    fireEvent.click(within(theme).getByRole('radio', { name: 'Dark' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'set_app_settings')?.args).toMatchObject({
        request: { payload: { theme: 'dark' } },
      });
    });
  });

  it('moves along the sections with the arrows and opens one on a press', () => {
    backend();
    render(<Shown snapshot={snapshotFixture()} />);
    const tab = (name: string) => screen.getByRole('tab', { name });
    // One tab stop: the open section.
    expect(screen.getAllByRole('tab').filter((item) => item.tabIndex === 0)).toEqual([
      tab('General'),
    ]);
    tab('General').focus();
    fireEvent.keyDown(tab('General'), { key: 'ArrowDown' });
    expect(tab('Feed')).toHaveFocus();
    // The arrows only move: nothing opens (and loads) on the way.
    expect(tab('General')).toHaveAttribute('aria-selected', 'true');
    fireEvent.keyDown(tab('Feed'), { key: 'End' });
    expect(tab('Reset')).toHaveFocus();
    fireEvent.keyDown(tab('Reset'), { key: 'ArrowDown' });
    expect(tab('General')).toHaveFocus();
    fireEvent.keyDown(tab('General'), { key: 'ArrowUp' });
    expect(tab('Reset')).toHaveFocus();
    fireEvent.keyDown(tab('Reset'), { key: 'Home' });
    expect(tab('General')).toHaveFocus();
    fireEvent.keyDown(tab('General'), { key: 'ArrowRight' });
    expect(tab('Feed')).toHaveFocus();
    fireEvent.keyDown(tab('Feed'), { key: 'ArrowLeft' });
    expect(tab('General')).toHaveFocus();
    // Right to left, the arrows follow the reading direction.
    tab('General').style.direction = 'rtl';
    fireEvent.keyDown(tab('General'), { key: 'ArrowLeft' });
    expect(tab('Feed')).toHaveFocus();
    tab('Feed').style.direction = 'rtl';
    fireEvent.keyDown(tab('Feed'), { key: 'ArrowRight' });
    expect(tab('General')).toHaveFocus();
    expect(tab('General')).toHaveAttribute('aria-selected', 'true');
    fireEvent.click(tab('Feed'));
    expect(tab('Feed')).toHaveAttribute('aria-selected', 'true');
    expect(tab('Feed').tabIndex).toBe(0);
    expect(screen.getByRole('tabpanel', { name: 'Feed' })).toBeVisible();
  });

  it('keeps the general choices in place while a change is saved', async () => {
    let answer: (value: unknown) => void = () => undefined;
    const saves: unknown[] = [];
    mockIPC((cmd, args) => {
      if (cmd === 'set_app_settings') {
        saves.push(args);
        return new Promise((done) => {
          answer = done;
        });
      }
      if (cmd === 'get_data_location') return new Promise(() => undefined);
      return null;
    });
    render(<Shown snapshot={snapshotFixture()} />);
    const keepOpen = screen.getByRole('radio', { name: /Keep the launcher window open/ });
    keepOpen.focus();
    fireEvent.click(keepOpen);
    // Not disabled (a disabled control drops the focus); a second change waits.
    expect(keepOpen).toBeEnabled();
    expect(keepOpen).toHaveFocus();
    expect(screen.getByRole('group', { name: 'While a game is running' })).toHaveAttribute(
      'aria-busy',
      'true',
    );
    const during = screen.getByRole('group', { name: 'While a game is running' });
    fireEvent.click(within(during).getByRole('radio', { name: /^Close the launcher once/ }));
    fireEvent.click(
      within(screen.getByRole('radiogroup', { name: 'Theme' })).getByRole('radio', {
        name: 'Dark',
      }),
    );
    expect(saves).toHaveLength(1);
    answer(snapshotFixture().settings);
    await waitFor(() => {
      expect(screen.getByRole('group', { name: 'While a game is running' })).not.toHaveAttribute(
        'aria-busy',
      );
    });
  });
});
