import { Button } from '../../components/Button';
import { Panel } from '../../components/Panel';
import type { AppSnapshot } from '../../lib/contracts';
import { strings } from '../../lib/strings';
import { SkinStudio } from './SkinStudio';
import styles from './Skins.module.css';

interface Props {
  snapshot: AppSnapshot;
  onOpenAccounts: () => void;
}

/** The Skins tab: the same screen as in the launcher account's panel, for a
 * signed-in launcher account (D-065). */
export function SkinsView({ snapshot, onOpenAccounts }: Props) {
  const s = strings.accounts.skins;
  const account = snapshot.account.launcherAccount;
  const signedIn = account.id !== null && account.nick !== null && !account.needsSignIn;
  return (
    <Panel title={strings.nav.skins}>
      {signedIn ? (
        <SkinStudio nick={account.nick} />
      ) : (
        <div className={styles.signIn}>
          <p>{account.needsSignIn ? s.signInAgain : s.signInFirst}</p>
          <Button variant="primary" onClick={onOpenAccounts}>
            {s.openAccounts}
          </Button>
        </div>
      )}
    </Panel>
  );
}
