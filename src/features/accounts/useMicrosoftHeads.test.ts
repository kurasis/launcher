import { renderHook, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import type { MicrosoftAccount } from '../../lib/contracts';
import { useMicrosoftHeads } from './useMicrosoftHeads';

const HEAD = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAgAAAAI';

function account(profileId: string): MicrosoftAccount {
  return { profileId, profileName: profileId, state: 'verified_access', error: null };
}

describe('Microsoft heads', () => {
  it('keeps only PNG heads of the accounts asked for, whatever the answer holds', async () => {
    mockIPC(() => ({
      steve: HEAD,
      alex: 'data:image/svg+xml;base64,PHN2Zz4=',
      kim: 'javascript:alert(1)',
      stranger: HEAD,
      __proto__: HEAD,
      ['constructor']: HEAD,
    }));
    const { result } = renderHook(() =>
      useMicrosoftHeads([account('steve'), account('alex'), account('kim')]),
    );
    await waitFor(() => {
      expect(result.current.size).toBe(1);
    });
    expect([...result.current.entries()]).toEqual([['steve', HEAD]]);
  });

  it('takes nothing from an answer that is not a map', async () => {
    for (const answer of [[HEAD], 'steve', 7, null]) {
      let asked = false;
      mockIPC(() => {
        asked = true;
        return answer;
      });
      const { result, unmount } = renderHook(() => useMicrosoftHeads([account('steve')]));
      await waitFor(() => {
        expect(asked).toBe(true);
      });
      expect(result.current.size).toBe(0);
      unmount();
    }
  });
});
