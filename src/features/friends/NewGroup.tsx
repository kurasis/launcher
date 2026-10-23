import { useEffect, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import forms from '../../components/forms.module.css';
import type { FriendsView } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import styles from './Friends.module.css';
import { PlayerHead } from './People';
import { useAction } from './useAction';

const t = strings.friends;
const NAME_MAX = 40;
/** A group is its owner and at most 19 friends. */
const GROUP_MAX = 20;

/** A new group in the conversation pane: its name and at least two friends. */
export function NewGroup({
  view,
  heads,
  onDone,
}: {
  view: FriendsView;
  heads: ReadonlyMap<string, string>;
  /** The new group's room, or null when the player cancelled. */
  onDone: (room: string | null) => void;
}) {
  const [name, setName] = useState('');
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const action = useAction();
  const heading = useRef<HTMLHeadingElement>(null);
  const ready = name.trim().length > 0 && chosen.size >= 2 && chosen.size < GROUP_MAX;
  // Opened from the sheet's "+": the form takes the focus.
  useEffect(() => {
    heading.current?.focus();
  }, []);
  return (
    <form
      className={styles.newGroup}
      onSubmit={(event) => {
        event.preventDefault();
        if (!ready || action.pending) return;
        void action
          .run(() => ipc.friendsCreateGroup(name.trim(), [...chosen]))
          .then((room) => {
            if (room !== null) onDone(room);
          });
      }}
    >
      <h3 ref={heading} className={styles.paneTitle} tabIndex={-1}>
        {t.newGroup}
      </h3>
      <Field label={t.groupName}>
        {(props) => (
          <input
            {...props}
            className={forms.input}
            value={name}
            maxLength={NAME_MAX}
            onChange={(event) => {
              setName(event.target.value);
            }}
          />
        )}
      </Field>
      <fieldset className={styles.choices}>
        <legend className={styles.subtitle}>{t.groupMembers}</legend>
        {view.friends.map((friend) => (
          <label key={friend.id} className={styles.choice}>
            <input
              type="checkbox"
              checked={chosen.has(friend.id)}
              onChange={(event) => {
                const next = new Set(chosen);
                if (event.target.checked) next.add(friend.id);
                else next.delete(friend.id);
                setChosen(next);
              }}
            />
            <PlayerHead nick={friend.nick} heads={heads} size={24} />
            <span className={styles.nick}>{friend.nick}</span>
          </label>
        ))}
      </fieldset>
      {action.error !== null && (
        <p className={styles.error} role="alert">
          {action.error}
        </p>
      )}
      <div className={styles.inline}>
        <Button
          type="submit"
          variant="primary"
          disabled={!ready}
          aria-disabled={action.pending || undefined}
        >
          {t.createGroup}
        </Button>
        <Button
          onClick={() => {
            onDone(null);
          }}
        >
          {t.cancel}
        </Button>
      </div>
    </form>
  );
}
