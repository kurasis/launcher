import { act, fireEvent, render, screen } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it, vi } from 'vitest';
import type { ModsSetProblem } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { LaunchCheck } from './LaunchCheck';

const INSTANCE = '00000000-0000-4000-8000-000000000001';

const problem: ModsSetProblem = {
  kind: 'missing',
  titles: ['Extra'],
  id: 'cloth-config',
  versions: [],
  found: null,
  from: null,
  loader: null,
};

function setup(problems: ModsSetProblem[]) {
  const launches: unknown[] = [];
  mockIPC((cmd, args) => {
    if (cmd === 'mods_check') return problems;
    if (cmd === 'launch_game') {
      launches.push(args);
      return { runId: 'r' };
    }
    return null;
  });
  const onOpenMods = vi.fn();
  render(<LaunchCheck onOpenMods={onOpenMods} />);
  return { launches, onOpenMods };
}

const strip = (text: string | null) => (text ?? '').replace(/[⁨⁩]/g, '');

describe('the check before a launch', () => {
  it('starts at once when the mods’ files say nothing', async () => {
    const { launches } = setup([]);
    await act(async () => {
      await ipc.launchGame(INSTANCE, 'a');
    });
    expect(launches).toHaveLength(1);
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('asks first, and starts anyway only when the player says so', async () => {
    const { launches, onOpenMods } = setup([problem]);
    let started: Promise<unknown> = Promise.resolve();
    act(() => {
      started = ipc.launchGame(INSTANCE, 'b');
    });
    const dialog = await screen.findByRole('dialog', { name: 'The game may not start' });
    expect(strip(dialog.textContent)).toContain(
      'Extra needs cloth-config, which is not in the installation.',
    );
    // Not now: nothing starts.
    fireEvent.click(screen.getByRole('button', { name: 'Not now' }));
    await act(async () => {
      expect(await started).toBeNull();
    });
    expect(launches).toHaveLength(0);
    expect(screen.queryByRole('dialog')).toBeNull();

    // Looking at the mods instead.
    act(() => {
      started = ipc.launchGame(INSTANCE, 'c');
    });
    fireEvent.click(await screen.findByRole('button', { name: 'Open the installation’s mods' }));
    await act(async () => {
      expect(await started).toBeNull();
    });
    expect(onOpenMods).toHaveBeenCalledWith(INSTANCE);

    // Start anyway: it starts, and the same problems are not asked about again.
    act(() => {
      started = ipc.launchGame(INSTANCE, 'd');
    });
    fireEvent.click(await screen.findByRole('button', { name: 'Start anyway' }));
    await act(async () => {
      await started;
    });
    expect(launches).toHaveLength(1);
    await act(async () => {
      await ipc.launchGame(INSTANCE, 'e');
    });
    expect(launches).toHaveLength(2);
    expect(screen.queryByRole('dialog')).toBeNull();
  });
});
