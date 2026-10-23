import { useEffect, useState } from 'react';
import type { MicrosoftAccount } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { isPngDataUrl } from '../skins/textures';

/** Heads by profile ID, kept with the accounts they were asked for. */
interface Heads {
  for: string;
  heads: ReadonlyMap<string, string>;
}

/**
 * The heads of the Microsoft accounts signed in here, by profile ID: the skin Mojang
 * serves for each, cut by the core. Asked again when the accounts change; an account
 * without one, or an answer that is not a PNG, gets the default face.
 */
export function useMicrosoftHeads(
  accounts: readonly MicrosoftAccount[],
): ReadonlyMap<string, string> {
  const key = accounts
    .map((account) => account.profileId)
    .sort()
    .join(' ');
  const [shown, setShown] = useState<Heads | null>(null);
  useEffect(() => {
    if (key === '') return;
    let live = true;
    ipc
      .skinsMicrosoftHeads()
      .then((answer: unknown) => {
        if (!live) return;
        const heads = new Map<string, string>();
        if (typeof answer === 'object' && answer !== null) {
          for (const [id, head] of Object.entries(answer)) {
            if (key.split(' ').includes(id) && isPngDataUrl(head)) heads.set(id, head);
          }
        }
        setShown({ for: key, heads });
      })
      // Only pictures: the default faces stay.
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [key]);
  return shown?.for === key ? shown.heads : NONE;
}

const NONE: ReadonlyMap<string, string> = new Map();
