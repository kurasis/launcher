import { useEffect, useState } from 'react';
import { ipc } from '../../lib/ipc';
import { onOwnSkinChanged } from './ownSkin';
import { isPngDataUrl, isWardrobe } from './textures';

/**
 * The launcher account's worn skin as a data: URL while `owner` (that account's nick)
 * is shown; fetched again when the account changes or the skin is changed.
 */
export function useOwnSkin(owner: string | null): string | null {
  const [shown, setShown] = useState<{ owner: string; texture: string | null } | null>(null);
  const [changes, setChanges] = useState(0);
  useEffect(
    () =>
      onOwnSkinChanged(() => {
        setChanges((n) => n + 1);
      }),
    [],
  );
  useEffect(() => {
    if (owner === null) return;
    let live = true;
    ipc
      .skinsMine()
      .then((mine: unknown) =>
        isWardrobe(mine) && mine.skin !== null ? ipc.skinsTexture(mine.skin.hash) : null,
      )
      .then((url) => {
        if (live) setShown({ owner, texture: isPngDataUrl(url) ? url : null });
      })
      // Only a picture: the head this account had stays, or the default one.
      .catch(() => {
        if (live) setShown((prev) => (prev?.owner === owner ? prev : { owner, texture: null }));
      });
    return () => {
      live = false;
    };
  }, [owner, changes]);
  return owner !== null && shown?.owner === owner ? shown.texture : null;
}
