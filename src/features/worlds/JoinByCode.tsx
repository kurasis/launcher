import { useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type { WorldsPreview, WorldsRoom } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { Dialog } from '../mods/Dialog';
import { worldsErrorText } from './useWorlds';
import styles from './Worlds.module.css';

interface Props {
  onClose: () => void;
  onJoined: (room: WorldsRoom) => void;
}

/** Looks at where an invite code leads, then becomes a member there. */
export function JoinByCode({ onClose, onJoined }: Props) {
  const t = strings.worlds.code;
  const [code, setCode] = useState('');
  const [preview, setPreview] = useState<WorldsPreview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const look = () => {
    if (code.trim() === '') return;
    setBusy(true);
    setError(null);
    setPreview(null);
    ipc
      .worldsPreview(code.trim())
      .then(setPreview)
      .catch((e: unknown) => {
        setError(worldsErrorText(e));
      })
      .finally(() => {
        setBusy(false);
      });
  };

  const accept = () => {
    setBusy(true);
    setError(null);
    ipc
      .worldsRedeem(code.trim())
      .then(onJoined)
      .catch((e: unknown) => {
        setError(worldsErrorText(e));
        setBusy(false);
      });
  };

  return (
    <Dialog title={strings.worlds.joinByCode} onClose={onClose}>
      <form
        className={styles.form}
        onSubmit={(event) => {
          event.preventDefault();
          look();
        }}
      >
        <Field label={t.label}>
          {(props) => (
            <input
              {...props}
              className={forms.input}
              value={code}
              maxLength={512}
              autoComplete="off"
              spellCheck={false}
              onChange={(e) => {
                setCode(e.target.value);
                setPreview(null);
              }}
            />
          )}
        </Field>
        {preview !== null && (
          <p role="status">
            {t.preview({
              name: preview.name,
              owner: preview.ownerNick,
              version: preview.minecraft,
              members: preview.members,
            })}
          </p>
        )}
        {error !== null && <Notice tone="danger">{error}</Notice>}
        <div className={forms.actions}>
          <Button onClick={onClose}>{strings.worlds.cancel}</Button>
          {preview === null ? (
            <Button type="submit" variant="primary" disabled={busy || code.trim() === ''}>
              {t.look}
            </Button>
          ) : (
            <Button variant="primary" disabled={busy} onClick={accept}>
              {t.accept}
            </Button>
          )}
        </div>
      </form>
    </Dialog>
  );
}
