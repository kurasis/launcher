import { act, renderHook, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import { usePlayerHeads } from './usePlayerHeads';

const HEAD = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAgAAAAI';

/** Each test asks for nicks of its own: the heads are kept between lists. */
function questions(answer: (nicks: string[]) => unknown) {
  const asked: string[][] = [];
  mockIPC((cmd, args) => {
    if (cmd !== 'skins_heads') return null;
    const nicks = (args as { nicks: string[] }).nicks;
    asked.push(nicks);
    return answer(nicks);
  });
  return asked;
}

describe('player heads', () => {
  it('keeps only PNG heads of the nicks asked for, whatever the answer holds', async () => {
    questions(() => ({
      Kira_1: HEAD,
      Max_1: 'data:image/svg+xml;base64,PHN2Zz4=',
      Lee_1: 'javascript:alert(1)',
      Stranger_1: HEAD,
      __proto__: HEAD,
      ['constructor']: HEAD,
    }));
    const { result } = renderHook(() => usePlayerHeads(['Kira_1', 'Max_1', 'Lee_1']));
    await waitFor(() => {
      expect(result.current.size).toBe(1);
    });
    expect([...result.current.entries()]).toEqual([['Kira_1', HEAD]]);
  });

  it('never asks for what cannot be a nick', async () => {
    const asked = questions(() => ({}));
    renderHook(() =>
      usePlayerHeads(['<img src=x>', 'a b c', '', 'ab', 'x'.repeat(17), '../../x', 'Ok_nick_2']),
    );
    await waitFor(() => {
      expect(asked).toHaveLength(1);
    });
    expect(asked[0]).toEqual(['Ok_nick_2']);
  });

  it('asks 200 nicks at a time at most', async () => {
    const nicks = Array.from({ length: 450 }, (_, i) => `many_${String(i)}`);
    const asked = questions((list) => Object.fromEntries(list.map((nick) => [nick, HEAD])));
    const { result } = renderHook(() => usePlayerHeads(nicks));
    await waitFor(() => {
      expect(result.current.size).toBe(450);
    });
    expect(asked.map((list) => list.length)).toEqual([200, 200, 50]);
  });

  it('asks once for lists that show the same players, and keeps the answer', async () => {
    const asked = questions((list) => Object.fromEntries(list.map((nick) => [nick, HEAD])));
    const first = renderHook(() => usePlayerHeads(['Shared_1', 'Shared_2']));
    const second = renderHook(() => usePlayerHeads(['Shared_2', 'Shared_1', 'Shared_1']));
    await waitFor(() => {
      expect(second.result.current.size).toBe(2);
    });
    expect(first.result.current.size).toBe(2);
    expect(asked).toEqual([['Shared_1', 'Shared_2']]);
    first.unmount();
    second.unmount();
    // Opened again: the heads are there at once, nothing is asked.
    const again = renderHook(() => usePlayerHeads(['Shared_1']));
    expect(again.result.current.get('Shared_1')).toBe(HEAD);
    await act(async () => {
      await Promise.resolve();
    });
    expect(asked).toHaveLength(1);
  });

  it('takes nothing from an answer that is not a map, or a failed question', async () => {
    for (const [index, answer] of [[HEAD], 'Kira_1', 7, null].entries()) {
      const nick = `odd_${String(index)}`;
      const asked = questions(() => answer);
      const { result, unmount } = renderHook(() => usePlayerHeads([nick]));
      await waitFor(() => {
        expect(asked).toHaveLength(1);
      });
      expect(result.current.size).toBe(0);
      unmount();
    }
    const asked = questions(() => {
      throw new Error('unreachable');
    });
    const { result } = renderHook(() => usePlayerHeads(['Failed_1']));
    await waitFor(() => {
      expect(asked).toHaveLength(1);
    });
    expect(result.current.size).toBe(0);
  });
});
