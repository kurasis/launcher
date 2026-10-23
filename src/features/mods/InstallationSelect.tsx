import type { Ref } from 'react';
import forms from '../../components/forms.module.css';
import type { InstanceSummary } from '../../lib/contracts';
import { strings } from '../../lib/strings';
import { gameLabel } from '../instances/loaderLabel';
import catalog from './Catalog.module.css';

interface Props {
  id: string;
  instances: readonly InstanceSummary[];
  value: string | null;
  onChange: (instanceId: string | null) => void;
  selectRef?: Ref<HTMLSelectElement> | undefined;
}

/** "Installation": the one what fits is shown for and what Add puts things into. */
export function InstallationSelect({ id, instances, value, onChange, selectRef }: Props) {
  const t = strings.mods;
  return (
    <div className={catalog.installation}>
      <label htmlFor={id}>{t.installation}</label>
      <select
        id={id}
        ref={selectRef}
        className={forms.select}
        value={value ?? ''}
        onChange={(event) => {
          onChange(event.target.value === '' ? null : event.target.value);
        }}
      >
        <option value="">{t.noInstallation}</option>
        {instances.map((i) => (
          <option key={i.id} value={i.id}>
            {i.displayName} ({gameLabel(i)})
          </option>
        ))}
      </select>
    </div>
  );
}
