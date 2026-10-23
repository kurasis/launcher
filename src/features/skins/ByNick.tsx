import { useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import { Segmented } from '../../components/Segmented';
import forms from '../../components/forms.module.css';
import type { SkinKind, SkinsProblem } from '../../lib/contracts';
import { strings } from '../../lib/strings';
import styles from './Skins.module.css';

/** A launcher nick, as the service takes it; nothing else is ever sent as one. */
const NICK = /^[A-Za-z0-9_]{3,16}$/;

interface Props {
  /** What to take at first. */
  kind: SkinKind;
  /** A request is on its way: the form waits. */
  busy: boolean;
  /** Asks for what the nick wears and opens it; why not, or null. */
  onTake: (kind: SkinKind, nick: string) => Promise<SkinsProblem | null>;
  onClose: () => void;
}

/** Another player's skin or cape, by their launcher nick, to try on and keep. */
export function ByNick({ kind: firstKind, busy, onTake, onClose }: Props) {
  const s = strings.accounts.skins;
  const t = s.byNick;
  const [nick, setNick] = useState('');
  const [kind, setKind] = useState<SkinKind>(firstKind);
  const [problem, setProblem] = useState<SkinsProblem | null>(null);
  const valid = NICK.test(nick);
  return (
    <form
      className={styles.byNick}
      aria-label={s.wardrobe.byNick}
      onSubmit={(event) => {
        event.preventDefault();
        if (!valid || busy) return;
        setProblem(null);
        void onTake(kind, nick).then(setProblem);
      }}
      onKeyDown={(event) => {
        if (event.key === 'Escape') {
          event.preventDefault();
          onClose();
        }
      }}
    >
      <p className={styles.hint}>{t.hint}</p>
      <div className={styles.byNickRow}>
        <Field label={t.nick} hint={t.nickHint}>
          {(props) => (
            <input
              {...props}
              className={forms.input}
              value={nick}
              dir="ltr"
              maxLength={16}
              autoComplete="off"
              autoCapitalize="off"
              spellCheck={false}
              // Opened on purpose: typing starts here.
              autoFocus
              aria-invalid={nick !== '' && !valid}
              onChange={(event) => {
                setNick(event.target.value.slice(0, 16));
              }}
            />
          )}
        </Field>
        <div className={styles.byNickKind}>
          <span className={styles.label}>{t.what}</span>
          <Segmented
            label={t.what}
            options={[
              { value: 'skin', label: s.skin },
              { value: 'cape', label: s.cape },
            ]}
            value={kind}
            pending={busy}
            onChange={setKind}
          />
        </div>
      </div>
      <div className={forms.actions}>
        <Button type="submit" variant="primary" disabled={!valid} aria-disabled={busy || undefined}>
          {t.take}
        </Button>
        <Button onClick={onClose}>{s.cancel}</Button>
      </div>
      {problem !== null && <Notice tone="warning">{s.problems[problem]}</Notice>}
    </form>
  );
}
