import { useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import forms from '../../components/forms.module.css';
import type { SupportedVersion } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';

interface CreateInstanceFormProps {
  versions: SupportedVersion[];
  onDone: () => void;
}

const t = strings.createForm;

export function CreateInstanceForm({ versions, onDone }: CreateInstanceFormProps) {
  const [name, setName] = useState('');
  const [versionId, setVersionId] = useState(versions[0]?.id ?? '');
  const create = useCommand((requestId: string, displayName: string, version: string) =>
    ipc.createInstance({ displayName, versionId: version }, requestId),
  );

  const submit = async () => {
    const created = await create.run(name, versionId);
    if (created) onDone();
  };

  return (
    <Panel title={t.title}>
      <form
        className={forms.form}
        onSubmit={(event) => {
          event.preventDefault();
          void submit();
        }}
      >
        <Field label={t.name}>
          {(props) => (
            <input
              {...props}
              className={forms.input}
              value={name}
              maxLength={64}
              required
              autoComplete="off"
              onChange={(event) => {
                setName(event.target.value);
              }}
            />
          )}
        </Field>
        <Field label={t.version} hint={t.isolation}>
          {(props) => (
            <select
              {...props}
              className={forms.select}
              value={versionId}
              onChange={(event) => {
                setVersionId(event.target.value);
              }}
            >
              {versions.map((version) => (
                <option key={version.id} value={version.id}>
                  {version.id}
                  {version.status === 'pending_verification' ? ` (${t.pending})` : ''}
                </option>
              ))}
            </select>
          )}
        </Field>
        {create.error !== null && <Notice tone="danger">{create.error}</Notice>}
        <div className={forms.actions}>
          <Button variant="primary" type="submit" disabled={create.pending || name.trim() === ''}>
            {t.submit}
          </Button>
          <Button onClick={onDone}>{t.cancel}</Button>
        </div>
      </form>
    </Panel>
  );
}
