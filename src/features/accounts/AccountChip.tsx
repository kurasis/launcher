import type { AccountSummary } from '../../lib/contracts';
import { strings } from '../../lib/strings';
import styles from './AccountChip.module.css';

/** Persistent account status in the top bar. Informational only. */
export function AccountChip({ account }: { account: AccountSummary }) {
  return (
    <p className={styles.chip}>
      <span className={styles.label}>{strings.account.label}:</span>{' '}
      {account.profileName ?? strings.account.state[account.state]}
    </p>
  );
}
