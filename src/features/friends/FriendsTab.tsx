import { useId, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import forms from '../../components/forms.module.css';
import type { FriendAction, FriendPerson, FriendsView, SettingsView } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { Confirm } from './Confirm';
import styles from './Friends.module.css';
import type { GameForInvites } from './InviteCard';
import { inviteBody } from './invite';
import { PlayerHead, Presence } from './People';
import { useAction } from './useAction';

const t = strings.friends;
/** The longest nick the friends service takes. */
const NICK_MAX = 32;

/**
 * The Friends tab: add by nick, each friend with Message, Invite (while the game is on
 * a server), Remove and Block after a question, the blocked list, and the chat settings.
 */
export function FriendsTab({
  view,
  settings,
  game,
  heads,
  onOpen,
}: {
  view: FriendsView;
  settings: SettingsView;
  game: GameForInvites;
  heads: ReadonlyMap<string, string>;
  onOpen: (room: string) => void;
}) {
  const [nick, setNick] = useState('');
  const [notice, setNotice] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<{
    action: 'block' | 'remove';
    person: FriendPerson;
  } | null>(null);
  // The button that asked: the focus goes back to it when the question closes.
  const asker = useRef<HTMLElement | null>(null);
  const action = useAction();
  const preferences = useAction();
  const blockedId = useId();

  const ask = (what: 'block' | 'remove', person: FriendPerson) => {
    asker.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    setConfirming({ action: what, person });
  };
  const closeQuestion = () => {
    setConfirming(null);
    if (asker.current?.isConnected === true) asker.current.focus();
  };
  const act = async (what: FriendAction, person: FriendPerson) => {
    setNotice(null);
    await action.run(() => ipc.friendsAct(what, person.id));
  };
  // A press while another action runs is ignored; the button keeps the focus.
  const busy = action.pending || undefined;

  return (
    <div className={styles.tabBody}>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          const wanted = nick.trim();
          if (wanted.length === 0 || action.pending) return;
          setNotice(null);
          void action
            .run(() => ipc.friendsRequest(wanted))
            .then((result) => {
              if (result === null) return;
              setNotice(
                result === 'friends' ? t.nowFriends({ nick: wanted }) : t.sent({ nick: wanted }),
              );
              setNick('');
            });
        }}
      >
        <Field label={t.addLabel} hint={t.addHint}>
          {(props) => (
            <div className={styles.inline}>
              <input
                {...props}
                className={forms.input}
                value={nick}
                maxLength={NICK_MAX}
                autoComplete="off"
                spellCheck={false}
                dir="ltr"
                onChange={(event) => {
                  setNick(event.target.value);
                }}
              />
              <Button type="submit" disabled={nick.trim().length === 0} aria-disabled={busy}>
                {t.add}
              </Button>
            </div>
          )}
        </Field>
      </form>
      {notice !== null && (
        <p className={styles.success} role="status">
          {notice}
        </p>
      )}
      {action.error !== null && (
        <p className={styles.error} role="alert">
          {action.error}
        </p>
      )}
      {view.friends.length === 0 ? (
        <p className={styles.muted}>{t.noFriends}</p>
      ) : (
        <ul className={styles.people}>
          {view.friends.map((person) => (
            <li key={person.id} className={styles.personItem}>
              <div className={styles.person}>
                <PlayerHead nick={person.nick} heads={heads} />
                <span className={styles.who}>
                  <span className={styles.nick}>{person.nick}</span>
                  <Presence person={person} />
                </span>
                <span className={styles.actions}>
                  <Button
                    size="small"
                    aria-disabled={busy}
                    onClick={() => {
                      if (action.pending) return;
                      void action
                        .run(() => ipc.friendsOpenDirect(person.id))
                        .then((room) => {
                          if (room !== null) onOpen(room);
                        });
                    }}
                  >
                    {t.message}
                  </Button>
                  {game.invite !== null && (
                    <Button
                      size="small"
                      aria-disabled={busy}
                      onClick={() => {
                        const invite = game.invite;
                        if (invite === null || action.pending) return;
                        setNotice(null);
                        void action
                          .run(async () => {
                            const room = await ipc.friendsOpenDirect(person.id);
                            await ipc.friendsSend(room, inviteBody(invite));
                          })
                          .then((sent) => {
                            if (sent !== null) setNotice(t.inviteSent({ nick: person.nick }));
                          });
                      }}
                    >
                      {t.inviteFriend}
                    </Button>
                  )}
                  <Button
                    size="small"
                    variant="ghost"
                    onClick={() => {
                      ask('remove', person);
                    }}
                  >
                    {t.remove}
                  </Button>
                  <Button
                    size="small"
                    variant="ghost"
                    onClick={() => {
                      ask('block', person);
                    }}
                  >
                    {t.block}
                  </Button>
                </span>
              </div>
              {confirming !== null && confirming.person.id === person.id && (
                <Confirm
                  label={person.nick}
                  text={
                    confirming.action === 'block'
                      ? t.confirmBlock({ nick: person.nick })
                      : t.confirmRemove({ nick: person.nick })
                  }
                  action={confirming.action === 'block' ? t.block : t.remove}
                  pending={action.pending}
                  onYes={() => {
                    void act(confirming.action, person).then(closeQuestion);
                  }}
                  onNo={closeQuestion}
                />
              )}
            </li>
          ))}
        </ul>
      )}
      {view.blocked.length > 0 && (
        <section className={styles.group} aria-labelledby={blockedId}>
          <h3 id={blockedId} className={styles.subtitle}>
            {t.blocked}
          </h3>
          <p className={styles.muted}>{t.blockedHint}</p>
          <ul className={styles.people}>
            {view.blocked.map((person) => (
              <li key={person.id} className={styles.person}>
                <PlayerHead nick={person.nick} heads={heads} />
                <span className={styles.who}>
                  <span className={styles.nick}>{person.nick}</span>
                </span>
                <span className={styles.actions}>
                  <Button
                    size="small"
                    variant="ghost"
                    aria-disabled={busy}
                    onClick={() => {
                      if (!action.pending) void act('unblock', person);
                    }}
                  >
                    {t.unblock}
                  </Button>
                </span>
              </li>
            ))}
          </ul>
        </section>
      )}
      <fieldset className={styles.preferences}>
        <legend className={styles.subtitle}>{t.preferences}</legend>
        <label className={forms.choice}>
          <input
            type="checkbox"
            checked={view.visible}
            onChange={(event) => {
              const visible = event.target.checked;
              void preferences.run(() => ipc.friendsSetVisible(visible));
            }}
          />
          {t.showOnline}
        </label>
        <label className={forms.choice}>
          <input
            type="checkbox"
            checked={settings.chatSound}
            disabled={!settings.writable}
            onChange={(event) => {
              const on = event.target.checked;
              void preferences.run(() => ipc.setAppSettings({ chatSound: on }));
            }}
          />
          {t.chatSound}
        </label>
        <label className={forms.choice}>
          <input
            type="checkbox"
            checked={settings.chatNotifications}
            disabled={!settings.writable}
            onChange={(event) => {
              const on = event.target.checked;
              void preferences.run(() => ipc.setAppSettings({ chatNotifications: on }));
            }}
          />
          {t.chatNotifications}
        </label>
        {preferences.error !== null && (
          <p className={styles.error} role="alert">
            {preferences.error}
          </p>
        )}
      </fieldset>
    </div>
  );
}
