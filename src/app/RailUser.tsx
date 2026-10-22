import { useEffect, useState } from 'react';
import { SkinHead } from '../components/SkinHead';
import type { AccountSummary, OperationKind } from '../lib/contracts';
import { ipc } from '../lib/ipc';
import { strings } from '../lib/strings';
import { defaultPlayer, type Player } from '../features/accounts/players';
import { onOwnSkinChanged } from '../features/skins/ownSkin';
import { isPngDataUrl, isWardrobe } from '../features/skins/textures';
import styles from './App.module.css';
import { QuitControl } from './QuitControl';

/**
 * The launcher account's worn skin as a data: URL while `owner` (that account's nick)
 * plays by default; fetched again when the account changes or the skin is changed.
 */
function useOwnSkin(owner: string | null): string | null {
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

function playerName(player: Player): string {
  return player.kind === 'microsoft' ? player.account.profileName : player.name;
}

/** Who plays by default, with their skin's head, at the foot of the rail; and Quit. */
export function RailUser({
  account,
  activeKind,
}: {
  account: AccountSummary;
  activeKind: OperationKind | null;
}) {
  const player = defaultPlayer(account);
  const texture = useOwnSkin(player.kind === 'launcher' ? player.name : null);
  return (
    <div className={styles.user}>
      <SkinHead texture={texture} size={32} />
      <p className={styles.userText}>
        <span className={styles.userName}>{playerName(player)}</span>
        <span className={styles.userNote}>{strings.nav.playsByDefault}</span>
      </p>
      <QuitControl activeKind={activeKind} compact />
    </div>
  );
}
