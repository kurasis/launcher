import { SkinHead } from '../components/SkinHead';
import type { AccountSummary, OperationKind } from '../lib/contracts';
import { strings } from '../lib/strings';
import { defaultPlayer, type Player } from '../features/accounts/players';
import { useMicrosoftHeads } from '../features/accounts/useMicrosoftHeads';
import { useOwnSkin } from '../features/skins/useOwnSkin';
import styles from './App.module.css';
import { QuitControl } from './QuitControl';

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
  const heads = useMicrosoftHeads(player.kind === 'microsoft' ? [player.account] : []);
  const head = player.kind === 'microsoft' ? (heads.get(player.account.profileId) ?? null) : null;
  return (
    <div className={styles.user}>
      <SkinHead texture={texture} head={head} size={32} />
      <p className={styles.userText}>
        <span className={styles.userName}>{playerName(player)}</span>
        <span className={styles.userNote}>{strings.nav.playsByDefault}</span>
      </p>
      <QuitControl activeKind={activeKind} compact />
    </div>
  );
}
