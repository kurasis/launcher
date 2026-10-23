import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it, vi } from 'vitest';
import type { WorldsPreview } from '../../lib/contracts';
import { WorldInviteCard } from './WorldInviteCard';

const ROOM = '10000000-0000-4000-8000-000000000001';

const PREVIEW: WorldsPreview = {
  room: ROOM,
  name: 'Our base',
  ownerNick: 'Alex',
  members: 2,
  minecraft: '1.21.1',
  mods: null,
  hosting: false,
};

/** A failure of the worlds service as the core sends it. */
function problem(worldsProblem: string) {
  return Object.assign(new Error('The worlds service said no.'), {
    code: 'NETWORK',
    retryable: true,
    diagnosticId: 'd-1',
    safeDetails: { worldsProblem },
  });
}

/** A code of the service's shape, different for each number. */
function code(n: number): string {
  return `Code${String(n).padStart(4, '0')}AbCdEfGhIjKl`;
}

function setup(preview: (code: string) => WorldsPreview) {
  const asked: string[] = [];
  mockIPC((cmd, args) => {
    if (cmd !== 'worlds_preview') return null;
    const { code: wanted } = args as { code: string };
    asked.push(wanted);
    return preview(wanted);
  });
  return asked;
}

function card(n: number, mine = false, onOpen = vi.fn()) {
  return <WorldInviteCard key={n} code={code(n)} from="Alex" mine={mine} onOpen={onOpen} />;
}

describe('world invite card', () => {
  it('keeps Join when looking failed on the way, and looks again on a press', async () => {
    let down = true;
    const asked = setup(() => {
      if (down) throw problem('network');
      return PREVIEW;
    });
    render(card(1));
    expect(
      await screen.findByText('worlds.lapi.sh cannot be reached. Check the connection.'),
    ).toBeVisible();
    // Only a refused code takes Join away.
    expect(screen.getByRole('button', { name: 'Join the room' })).toBeVisible();
    down = false;
    fireEvent.click(screen.getByRole('button', { name: 'Look' }));
    expect(await screen.findByText('Our base, Minecraft 1.21.1')).toBeVisible();
    expect(screen.queryByText(/cannot be reached/)).toBeNull();
    expect(screen.queryByRole('button', { name: 'Look' })).toBeNull();
    // A failure is not kept: the press asked again.
    expect(asked).toEqual([code(1), code(1)]);
  });

  it('takes Join away for a refused code and remembers the refusal', async () => {
    const asked = setup(() => {
      throw problem('invite_invalid');
    });
    const { unmount } = render(card(2));
    expect(await screen.findByText('This invite code is wrong, used up or expired.')).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Join the room' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Look' })).toBeNull();
    unmount();
    // The same code shown again asks nothing for a while.
    render(card(2));
    expect(await screen.findByText('This invite code is wrong, used up or expired.')).toBeVisible();
    expect(asked).toEqual([code(2)]);
  });

  it('looks by itself only a few times, then waits for a press', async () => {
    const asked = setup(() => PREVIEW);
    const onOpen = vi.fn();
    // Codes 1 and 2 were looked at above; 3 and on are new.
    const cards = Array.from({ length: 14 }, (_, i) => card(i + 3, true, onOpen));
    render(<div>{cards}</div>);
    await waitFor(() => {
      expect(screen.getAllByText('Our base, Minecraft 1.21.1')).toHaveLength(10);
    });
    // Twelve looks in ten minutes: two went to the cards above.
    expect(asked).toHaveLength(10);
    const looks = screen.getAllByRole('button', { name: 'Look' });
    expect(looks).toHaveLength(4);
    const waiting = looks[0]?.closest('div[class*="card"]') as HTMLElement;
    fireEvent.click(looks[0] as HTMLElement);
    expect(await within(waiting).findByText('Our base, Minecraft 1.21.1')).toBeVisible();
    expect(asked).toHaveLength(11);
    fireEvent.click(within(waiting).getByRole('button', { name: 'Open room' }));
    expect(onOpen).toHaveBeenCalledWith(ROOM);
  });
});
