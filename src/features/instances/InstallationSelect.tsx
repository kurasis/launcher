import type { Ref } from 'react';
import forms from '../../components/forms.module.css';
import type { InstanceSummary } from '../../lib/contracts';
import { gameLabel } from './loaderLabel';
import styles from './InstallationSelect.module.css';

interface Props {
  id: string;
  label: string;
  /** The first choice, for no installation in particular ("None", "All installations");
   * without it one installation is always chosen. */
  none?: string | undefined;
  instances: readonly InstanceSummary[];
  value: string | null;
  onChange: (instanceId: string | null) => void;
  selectRef?: Ref<HTMLSelectElement> | undefined;
  /** As wide as its longest choice, in a toolbar: a name is never cut off (in Arabic the
   * cut would take the start of a Latin or Cyrillic name). */
  fit?: boolean;
}

/** An installation chosen beside its label: each one by its name and game. */
export function InstallationSelect({
  id,
  label,
  none,
  instances,
  value,
  onChange,
  selectRef,
  fit = false,
}: Props) {
  return (
    <div className={[styles.installation, fit && styles.fit].filter(Boolean).join(' ')}>
      <label htmlFor={id}>{label}</label>
      <select
        id={id}
        ref={selectRef}
        className={forms.select}
        value={value ?? ''}
        onChange={(event) => {
          onChange(event.target.value === '' ? null : event.target.value);
        }}
      >
        {none !== undefined && <option value="">{none}</option>}
        {instances.map((i) => (
          <option key={i.id} value={i.id}>
            {i.displayName} ({gameLabel(i)})
          </option>
        ))}
      </select>
    </div>
  );
}
