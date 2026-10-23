import { renderHook, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import { useTextures } from './useTextures';

const PNG = 'data:image/png;base64,iVBORw0KGgo=';
const hash = (n: number) => n.toString(16).padStart(64, '0');

/** The core, answering each texture when the test says. */
function core(answer: (asked: string) => unknown = () => PNG) {
  const asked: string[] = [];
  const waiting: (() => void)[] = [];
  let open = 0;
  let most = 0;
  mockIPC((cmd, args) => {
    if (cmd !== 'skins_texture') return null;
    const wanted = (args as { hash: string }).hash;
    asked.push(wanted);
    open += 1;
    most = Math.max(most, open);
    return new Promise((resolve) => {
      waiting.push(() => {
        open -= 1;
        resolve(answer(wanted));
      });
    });
  });
  return {
    asked,
    most: () => most,
    /** Answers the oldest question still waiting. */
    next: () => waiting.shift()?.(),
    waiting: () => waiting.length,
  };
}

describe('useTextures', () => {
  it('asks for every texture once, three at a time, in the order given', async () => {
    const c = core();
    const hashes = [1, 2, 3, 4, 5].map(hash);
    const { result } = renderHook(() => useTextures([...hashes, hash(1)]));
    await waitFor(() => {
      expect(c.waiting()).toBe(3);
    });
    expect(c.asked).toEqual(hashes.slice(0, 3));
    for (let answered = 0; answered < 5; answered++) {
      await waitFor(() => {
        expect(c.waiting()).toBeGreaterThan(0);
      });
      c.next();
    }
    await waitFor(() => {
      expect(result.current.size).toBe(5);
    });
    expect(c.asked).toEqual(hashes);
    expect(c.most()).toBe(3);
    expect([...result.current.values()]).toEqual([PNG, PNG, PNG, PNG, PNG]);
  });

  it('asks for nothing that is not a hash', async () => {
    const c = core();
    const { result } = renderHook(() =>
      useTextures(['../../etc/passwd', 'A'.repeat(64), `${hash(1)} `, '', hash(2)]),
    );
    await waitFor(() => {
      expect(c.waiting()).toBe(1);
    });
    c.next();
    await waitFor(() => {
      expect(result.current.get(hash(2))).toBe(PNG);
    });
    expect(c.asked).toEqual([hash(2)]);
  });

  it('keeps only PNG data URLs, and asks again for a failed one when the list changes', async () => {
    let good = false;
    const c = core(() => (good ? PNG : 'javascript:alert(1)'));
    const { result, rerender } = renderHook(({ list }) => useTextures(list), {
      initialProps: { list: [hash(1)] },
    });
    await waitFor(() => {
      expect(c.waiting()).toBe(1);
    });
    c.next();
    await waitFor(() => {
      expect(result.current.get(hash(1))).toBe('');
    });
    good = true;
    rerender({ list: [hash(1), hash(2)] });
    await waitFor(() => {
      expect(c.waiting()).toBe(2);
    });
    c.next();
    c.next();
    await waitFor(() => {
      expect(result.current.get(hash(1))).toBe(PNG);
    });
    expect(c.asked).toEqual([hash(1), hash(1), hash(2)]);
  });

  it('lets go of a texture the list no longer has', async () => {
    const c = core();
    const { result, rerender } = renderHook(({ list }) => useTextures(list), {
      initialProps: { list: [hash(1)] },
    });
    await waitFor(() => {
      expect(c.waiting()).toBe(1);
    });
    c.next();
    await waitFor(() => {
      expect(result.current.get(hash(1))).toBe(PNG);
    });
    rerender({ list: [hash(2)] });
    await waitFor(() => {
      expect(c.waiting()).toBe(1);
    });
    c.next();
    await waitFor(() => {
      expect(result.current.get(hash(2))).toBe(PNG);
    });
    expect(result.current.has(hash(1))).toBe(false);
    // An answer for a texture let go meanwhile is not kept either.
    rerender({ list: [hash(3)] });
    await waitFor(() => {
      expect(c.waiting()).toBe(1);
    });
    rerender({ list: [hash(4)] });
    await waitFor(() => {
      expect(c.waiting()).toBe(2);
    });
    c.next();
    c.next();
    await waitFor(() => {
      expect(result.current.get(hash(4))).toBe(PNG);
    });
    expect(result.current.has(hash(3))).toBe(false);
  });
});
