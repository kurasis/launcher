import { Button } from '../../components/Button';
import { Screen } from '../../components/Screen';
import type { AppSnapshot } from '../../lib/contracts';
import { strings } from '../../lib/strings';
import styles from './Skins.module.css';
import { Wardrobe } from './Wardrobe';

interface Props {
  snapshot: AppSnapshot;
  onOpenAccounts: () => void;
}

/** The Skins tab: the launcher account's wardrobe once it is signed in (D-065). */
export function SkinsView({ snapshot, onOpenAccounts }: Props) {
  const s = strings.accounts.skins;
  const account = snapshot.account.launcherAccount;
  const nick = account.id !== null && !account.needsSignIn ? account.nick : null;
  if (nick !== null) return <Wardrobe key={account.id} nick={nick} />;
  return (
    <Screen title={strings.nav.skins}>
      <div className={styles.signIn}>
        <p>{account.needsSignIn ? s.signInAgain : s.signInFirst}</p>
        <Button variant="primary" onClick={onOpenAccounts}>
          {s.openAccounts}
        </Button>
      </div>
    </Screen>
  );
}
