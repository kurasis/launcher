import { useId } from 'react';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type { AccountSummary, InstanceSummary, ProfileRef } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { defaultPlayer, lookup, playerLabel } from '../accounts/players';
import styles from './Instances.module.css';

const a = strings.accounts;

function encode(profile: ProfileRef | null): string {
  return profile === null ? '' : `${profile.kind}:${profile.id}`;
}

function decode(value: string): ProfileRef | null {
  if (value === '') return null;
  const [kind, ...rest] = value.split(':');
  const id = rest.join(':');
  return kind === 'microsoft' ? { kind: 'microsoft', id } : { kind: 'notoken', id };
}

/** Chooses who plays one installation: the default, an account or a demo profile (D-045). */
export function PlayAs({
  instance,
  account,
}: {
  instance: InstanceSummary;
  account: AccountSummary;
}) {
  const id = useId();
  const choose = useCommand((requestId: string, profile: ProfileRef | null) =>
    ipc.setInstanceProfile(instance.id, profile, requestId),
  );
  // A profile that no longer exists plays as the default.
  const current = lookup(account, instance.profile) === null ? null : instance.profile;
  return (
    <div className={styles.playAs}>
      <label htmlFor={id}>{a.playAs}</label>
      <select
        id={id}
        className={forms.select}
        value={encode(current)}
        disabled={choose.pending}
        onChange={(event) => void choose.run(decode(event.target.value))}
      >
        <option value="">{a.playAsDefault({ name: playerLabel(defaultPlayer(account)) })}</option>
        {account.accounts.map((item) => (
          <option key={item.profileId} value={`microsoft:${item.profileId}`}>
            {a.microsoftLabel({ name: item.profileName })}
          </option>
        ))}
        {account.notokenProfiles.map((item) => (
          <option key={item.id} value={`notoken:${item.id}`}>
            {a.notokenLabel({ name: item.name })}
          </option>
        ))}
      </select>
      {choose.error !== null && <Notice tone="danger">{choose.error}</Notice>}
    </div>
  );
}
