import type { AccountSummary } from '../../lib/contracts';
import { strings } from '../../lib/strings';
import styles from './AccountChip.module.css';
import { defaultPlayer, playerLabel } from './players';

/** Who plays by default, in the top bar. Informational only. */
export function AccountChip({ account }: { account: AccountSummary }) {
  return (
    <p className={styles.chip}>
      <span className={styles.label}>{strings.accounts.chipLabel}:</span>{' '}
      {playerLabel(defaultPlayer(account))}
    </p>
  );
}
