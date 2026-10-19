import { useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import forms from '../../components/forms.module.css';
import type { FriendAction, FriendPerson, FriendsView } from '../../lib/contracts';
import { friendsProblem, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { ChatView } from './ChatView';
import styles from './Friends.module.css';
import { problemText, roomTitle, useFriends } from './useFriends';

const t = strings.friends;
type Tab = 'chats' | 'friends' | 'requests';
const NAME_MAX = 40;
const GROUP_MAX = 20;

/**
 * Friends and chats of the launcher account (D-064): a small chat button in
 * the corner, next to the downloads, with the whole friends system in its
 * panel.
 */
export function FriendsCorner({ onOpenAccounts }: { onOpenAccounts: () => void }) {
  const view = useFriends();
  const [open, setOpen] = useState(false);
  if (view === null) return null;
  const waiting = view.unread + view.incoming.length;
  return (
    <div
      className={styles.corner}
      onKeyDown={(event) => {
        if (event.key === 'Escape') setOpen(false);
      }}
    >
      {open && (
        <FriendsPanel
          view={view}
          onOpenAccounts={() => {
            setOpen(false);
            onOpenAccounts();
          }}
        />
      )}
      <button
        type="button"
        className={styles.button}
        aria-label={view.unread > 0 ? `${t.open}, ${t.unread({ count: view.unread })}` : t.open}
        aria-expanded={open}
        title={t.open}
        onClick={() => {
          setOpen(!open);
        }}
      >
        <svg className={styles.icon} viewBox="0 0 24 24" aria-hidden="true">
          <path d="M20 12.5a7.5 7.5 0 0 1-11 6.6L4 20.5l1.4-4.6A7.5 7.5 0 1 1 20 12.5Z" />
          <path d="M9 12.5h.01M12.5 12.5h.01M16 12.5h.01" />
        </svg>
        {view.signedIn && waiting > 0 && (
          <span className={styles.badge}>{waiting > 99 ? '99+' : waiting}</span>
        )}
      </button>
    </div>
  );
}

function FriendsPanel({ view, onOpenAccounts }: { view: FriendsView; onOpenAccounts: () => void }) {
  const [tab, setTab] = useState<Tab>('chats');
  const [roomId, setRoomId] = useState<string | null>(null);
  const [grouping, setGrouping] = useState(false);
  const room = roomId === null ? undefined : view.rooms.find((r) => r.id === roomId);

  let body;
  if (!view.signedIn) {
    body = (
      <div className={styles.stack}>
        <p>{t.signedOut}</p>
        <Button variant="primary" onClick={onOpenAccounts}>
          {t.goToAccounts}
        </Button>
      </div>
    );
  } else if (room !== undefined) {
    body = (
      <ChatView
        key={room.id}
        room={room}
        view={view}
        onBack={() => {
          setRoomId(null);
        }}
      />
    );
  } else if (grouping) {
    body = (
      <NewGroup
        view={view}
        onDone={(created) => {
          setGrouping(false);
          if (created !== null) setRoomId(created);
        }}
      />
    );
  } else {
    const tabs: Tab[] = ['chats', 'friends', 'requests'];
    body = (
      <>
        <div className={styles.tabs} role="tablist">
          {tabs.map((item) => (
            <button
              key={item}
              type="button"
              role="tab"
              className={styles.tab}
              aria-selected={tab === item}
              onClick={() => {
                setTab(item);
              }}
            >
              {t.tabs[item]}
              {item === 'requests' && view.incoming.length > 0 && (
                <span className={styles.count}>{view.incoming.length}</span>
              )}
              {item === 'chats' && view.unread > 0 && (
                <span className={styles.count}>{view.unread}</span>
              )}
            </button>
          ))}
        </div>
        <div role="tabpanel">
          {tab === 'chats' && (
            <Chats
              view={view}
              onOpen={setRoomId}
              onNewGroup={() => {
                setGrouping(true);
              }}
            />
          )}
          {tab === 'friends' && <Friends view={view} onOpen={setRoomId} />}
          {tab === 'requests' && <Requests view={view} />}
        </div>
      </>
    );
  }

  return (
    <section
      className={styles.panel}
      aria-label={t.title}
      data-signed-in={view.signedIn ? 'true' : undefined}
    >
      <h2 className={styles.title}>{t.title}</h2>
      {view.signedIn && view.problem !== null ? (
        <p className={styles.muted} role="status">
          {problemText(view.problem)}
        </p>
      ) : (
        view.signedIn &&
        !view.connected && (
          <p className={styles.muted} role="status">
            {t.offline}
          </p>
        )
      )}
      {body}
    </section>
  );
}

/** Runs one action at a time and keeps what went wrong. */
function useAction() {
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = async <T,>(work: () => Promise<T>): Promise<T | null> => {
    setPending(true);
    setError(null);
    try {
      return await work();
    } catch (e) {
      setError(problemText(friendsProblem(e)));
      return null;
    } finally {
      setPending(false);
    }
  };
  return { pending, error, run };
}

function Chats({
  view,
  onOpen,
  onNewGroup,
}: {
  view: FriendsView;
  onOpen: (room: string) => void;
  onNewGroup: () => void;
}) {
  const nick = (id: string) =>
    id === view.me?.id
      ? t.you
      : (view.rooms.flatMap((r) => r.members).find((m) => m.id === id)?.nick ?? '—');
  return (
    <div className={styles.stack}>
      <Button disabled={view.friends.length < 2} onClick={onNewGroup}>
        {t.newGroup}
      </Button>
      {view.rooms.length === 0 ? (
        <p className={styles.muted}>{t.noChats}</p>
      ) : (
        <ul className={styles.list}>
          {view.rooms.map((room) => (
            <li key={room.id}>
              <button
                type="button"
                className={styles.roomButton}
                onClick={() => {
                  onOpen(room.id);
                }}
              >
                <span className={styles.roomName}>{roomTitle(room, view.me)}</span>
                {room.unread > 0 && (
                  <span className={styles.count} aria-label={t.unread({ count: room.unread })}>
                    {room.unread}
                  </span>
                )}
                {room.last !== null && (
                  <span className={styles.preview} dir="auto">
                    {nick(room.last.sender)}: {room.last.body}
                  </span>
                )}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function Friends({ view, onOpen }: { view: FriendsView; onOpen: (room: string) => void }) {
  const [nick, setNick] = useState('');
  const [notice, setNotice] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<{
    action: 'block' | 'remove';
    person: FriendPerson;
  } | null>(null);
  const action = useAction();

  const act = async (what: FriendAction, person: FriendPerson) => {
    setNotice(null);
    setConfirming(null);
    await action.run(() => ipc.friendsAct(what, person.id));
  };

  return (
    <div className={styles.stack}>
      <form
        className={styles.stack}
        onSubmit={(event) => {
          event.preventDefault();
          const wanted = nick.trim();
          if (wanted.length === 0) return;
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
                maxLength={32}
                autoComplete="off"
                spellCheck={false}
                dir="ltr"
                onChange={(event) => {
                  setNick(event.target.value);
                }}
              />
              <Button type="submit" disabled={action.pending || nick.trim().length === 0}>
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
      {confirming !== null && (
        <div className={styles.confirm} role="alertdialog" aria-label={confirming.person.nick}>
          <p>
            {confirming.action === 'block'
              ? t.confirmBlock({ nick: confirming.person.nick })
              : t.confirmRemove({ nick: confirming.person.nick })}
          </p>
          <div className={styles.inline}>
            <Button
              variant="primary"
              disabled={action.pending}
              onClick={() => void act(confirming.action, confirming.person)}
            >
              {confirming.action === 'block' ? t.block : t.remove}
            </Button>
            <Button
              onClick={() => {
                setConfirming(null);
              }}
            >
              {t.cancel}
            </Button>
          </div>
        </div>
      )}
      {view.friends.length === 0 ? (
        <p className={styles.muted}>{t.noFriends}</p>
      ) : (
        <ul className={styles.list}>
          {view.friends.map((person) => (
            <li key={person.id} className={styles.row}>
              <span className={styles.nick}>{person.nick}</span>
              <span className={styles.actions}>
                <button
                  type="button"
                  className={styles.link}
                  disabled={action.pending}
                  onClick={() =>
                    void action
                      .run(() => ipc.friendsOpenDirect(person.id))
                      .then((room) => {
                        if (room !== null) onOpen(room);
                      })
                  }
                >
                  {t.message}
                </button>
                <button
                  type="button"
                  className={styles.quiet}
                  onClick={() => {
                    setConfirming({ action: 'remove', person });
                  }}
                >
                  {t.remove}
                </button>
                <button
                  type="button"
                  className={styles.quiet}
                  onClick={() => {
                    setConfirming({ action: 'block', person });
                  }}
                >
                  {t.block}
                </button>
              </span>
            </li>
          ))}
        </ul>
      )}
      {view.blocked.length > 0 && (
        <>
          <h3 className={styles.subtitle}>{t.blocked}</h3>
          <p className={styles.muted}>{t.blockedHint}</p>
          <ul className={styles.list}>
            {view.blocked.map((person) => (
              <li key={person.id} className={styles.row}>
                <span className={styles.nick}>{person.nick}</span>
                <button
                  type="button"
                  className={styles.link}
                  disabled={action.pending}
                  onClick={() => void act('unblock', person)}
                >
                  {t.unblock}
                </button>
              </li>
            ))}
          </ul>
        </>
      )}
    </div>
  );
}

function Requests({ view }: { view: FriendsView }) {
  const action = useAction();
  const act = (what: FriendAction, person: FriendPerson) => {
    void action.run(() => ipc.friendsAct(what, person.id));
  };
  if (view.incoming.length === 0 && view.outgoing.length === 0) {
    return <p className={styles.muted}>{t.noRequests}</p>;
  }
  return (
    <div className={styles.stack}>
      {action.error !== null && (
        <p className={styles.error} role="alert">
          {action.error}
        </p>
      )}
      {view.incoming.length > 0 && (
        <>
          <h3 className={styles.subtitle}>{t.incoming}</h3>
          <ul className={styles.list}>
            {view.incoming.map((person) => (
              <li key={person.id} className={styles.row}>
                <span className={styles.nick}>{person.nick}</span>
                <span className={styles.actions}>
                  <button
                    type="button"
                    className={styles.link}
                    disabled={action.pending}
                    onClick={() => {
                      act('accept', person);
                    }}
                  >
                    {t.accept}
                  </button>
                  <button
                    type="button"
                    className={styles.quiet}
                    disabled={action.pending}
                    onClick={() => {
                      act('decline', person);
                    }}
                  >
                    {t.decline}
                  </button>
                  <button
                    type="button"
                    className={styles.quiet}
                    disabled={action.pending}
                    onClick={() => {
                      act('block', person);
                    }}
                  >
                    {t.block}
                  </button>
                </span>
              </li>
            ))}
          </ul>
        </>
      )}
      {view.outgoing.length > 0 && (
        <>
          <h3 className={styles.subtitle}>{t.outgoing}</h3>
          <ul className={styles.list}>
            {view.outgoing.map((person) => (
              <li key={person.id} className={styles.row}>
                <span className={styles.nick}>{person.nick}</span>
                <button
                  type="button"
                  className={styles.quiet}
                  disabled={action.pending}
                  onClick={() => {
                    act('cancel', person);
                  }}
                >
                  {t.cancel}
                </button>
              </li>
            ))}
          </ul>
        </>
      )}
    </div>
  );
}

function NewGroup({ view, onDone }: { view: FriendsView; onDone: (room: string | null) => void }) {
  const [name, setName] = useState('');
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const action = useAction();
  const ready = name.trim().length > 0 && chosen.size >= 2 && chosen.size < GROUP_MAX;
  return (
    <form
      className={styles.stack}
      onSubmit={(event) => {
        event.preventDefault();
        if (!ready) return;
        void action
          .run(() => ipc.friendsCreateGroup(name.trim(), [...chosen]))
          .then((room) => {
            if (room !== null) onDone(room);
          });
      }}
    >
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
        <Button type="submit" variant="primary" disabled={!ready || action.pending}>
          {t.createGroup}
        </Button>
        <Button
          onClick={() => {
            onDone(null);
          }}
        >
          {t.back}
        </Button>
      </div>
    </form>
  );
}
