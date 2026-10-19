import { useEffect, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import forms from '../../components/forms.module.css';
import type {
  AppSnapshot,
  ChatRoom,
  FriendAction,
  FriendPerson,
  FriendsView,
  SettingsView,
} from '../../lib/contracts';
import { accountProblem, friendsProblem, ipc, onFriendsMessage } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { ChatView } from './ChatView';
import type { GameForInvites } from './InviteCard';
import { inviteBody } from './invite';
import styles from './Friends.module.css';
import { playChime, unlockSoundOnFirstInput } from './sound';
import { problemText, roomTitle, sanctionText, useFriends } from './useFriends';

const t = strings.friends;
type Tab = 'chats' | 'friends' | 'requests';
const NAME_MAX = 40;
const GROUP_MAX = 20;

/**
 * Friends and chats of the launcher account (D-064): a small chat button in
 * the corner, next to the downloads, with the whole friends system in its
 * panel.
 */
/** The running game's server as an invite, and the installations to join with. */
function gameForInvites(snapshot: AppSnapshot): GameForInvites {
  const run = snapshot.currentRun;
  const running =
    run !== null &&
    run.kind === 'game' &&
    !['exited', 'failed', 'cancelled', 'stopped'].includes(run.state);
  const instance =
    run?.instanceId == null ? undefined : snapshot.instances.find((i) => i.id === run.instanceId);
  return {
    invite:
      running && run.server !== null && instance !== undefined
        ? { server: run.server, version: instance.versionId, loader: instance.loader?.kind ?? null }
        : null,
    instances: snapshot.instances,
    running,
  };
}

export function FriendsCorner({
  snapshot,
  onOpenAccounts,
}: {
  snapshot: AppSnapshot;
  onOpenAccounts: () => void;
}) {
  const settings = snapshot.settings;
  const game = gameForInvites(snapshot);
  const view = useFriends();
  const [open, setOpen] = useState(false);
  const [roomId, setRoomId] = useState<string | null>(null);
  useChime(view, settings, open ? roomId : null);
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
          settings={settings}
          game={game}
          roomId={roomId}
          onRoom={setRoomId}
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

/**
 * The chime for a message from someone else or a new friend request. While the
 * launcher is not in front, the system notification (made by the core) speaks
 * instead, unless the player turned those off. A chat open on screen makes no
 * sound for its own messages.
 */
function useChime(view: FriendsView | null, settings: SettingsView, openRoom: string | null) {
  const me = view?.me?.id ?? null;
  const latest = useRef({ me, settings, openRoom });
  useEffect(() => {
    latest.current = { me, settings, openRoom };
  }, [me, settings, openRoom]);
  const requests = useRef<Set<string> | null>(null);

  useEffect(() => unlockSoundOnFirstInput(), []);

  const wanted = () => {
    const { settings } = latest.current;
    return settings.chatSound && (document.hasFocus() || !settings.chatNotifications);
  };

  useEffect(
    () =>
      onFriendsMessage((message) => {
        const { me, openRoom } = latest.current;
        if (message.sender === me) return;
        if (message.room === openRoom && document.hasFocus()) return;
        if (wanted()) playChime();
      }),
    [],
  );

  // A request that was not there before: the first state only remembers.
  const incoming = view?.incoming;
  useEffect(() => {
    if (incoming === undefined) return;
    const ids = new Set(incoming.map((p) => p.id));
    const before = requests.current;
    requests.current = ids;
    if (before !== null && [...ids].some((id) => !before.has(id)) && wanted()) playChime();
  }, [incoming]);
}

function Spinner({ label }: { label: string }) {
  return (
    <p className={styles.loading} role="status">
      <span className={styles.spinner} aria-hidden="true" />
      {label}
    </p>
  );
}

/** The account has no confirmed email yet: what to do, right here. */
function ConfirmEmail({ onOpenAccounts }: { onOpenAccounts: () => void }) {
  const a = strings.accounts;
  const [checking, setChecking] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const check = async () => {
    setChecking(true);
    setNote(null);
    try {
      // Confirmed: friends tries again by itself and this panel goes.
      const confirmed = await ipc.launcherAccountRecheck();
      if (!confirmed) setNote(a.launcherStillNotConfirmed);
    } catch (e) {
      setNote(a.launcherProblems[accountProblem(e)]);
    } finally {
      setChecking(false);
    }
  };
  return (
    <div className={styles.stack}>
      <p>{t.emailNeeded}</p>
      <div className={styles.inline}>
        <Button variant="primary" disabled={checking} onClick={() => void check()}>
          {checking ? a.launcherChecking : a.launcherCheck}
        </Button>
        <Button
          onClick={() => {
            setNote(null);
            void ipc
              .launcherAccountResendConfirmation()
              .then((problem) => {
                setNote(problem === null ? t.emailResent : a.launcherProblems[problem]);
              })
              .catch(() => {
                setNote(a.launcherProblems.server);
              });
          }}
        >
          {a.launcherResend}
        </Button>
      </div>
      {checking && <Spinner label={t.checking} />}
      {note !== null && (
        <p className={styles.muted} role="status">
          {note}
        </p>
      )}
      <button type="button" className={styles.link} onClick={onOpenAccounts}>
        {t.goToAccounts}
      </button>
    </div>
  );
}

function FriendsPanel({
  view,
  settings,
  game,
  roomId,
  onRoom,
  onOpenAccounts,
}: {
  view: FriendsView;
  settings: SettingsView;
  game: GameForInvites;
  roomId: string | null;
  onRoom: (room: string | null) => void;
  onOpenAccounts: () => void;
}) {
  const [tab, setTab] = useState<Tab>('chats');
  const [grouping, setGrouping] = useState(false);
  const room = roomId === null ? undefined : view.rooms.find((r) => r.id === roomId);
  const setRoomId = onRoom;
  const unconfirmed = view.problem === 'email_not_confirmed';

  let body;
  if (unconfirmed) {
    body = <ConfirmEmail onOpenAccounts={onOpenAccounts} />;
  } else if (!view.signedIn) {
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
        game={game}
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
          {tab === 'friends' && (
            <Friends view={view} settings={settings} game={game} onOpen={setRoomId} />
          )}
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
      {view.sanction !== null && (
        <p className={styles.error} role="status">
          {sanctionText(view.sanction)}
        </p>
      )}
      {view.signedIn && view.problem !== null ? (
        !unconfirmed && (
          <p className={styles.muted} role="status">
            {problemText(view.problem)}
          </p>
        )
      ) : view.signedIn && view.me === null ? (
        <Spinner label={t.connecting} />
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
        <RoomSections view={view} nick={nick} onOpen={onOpen} />
      )}
    </div>
  );
}

/** Rooms with unread messages first, apart from the rest. */
function RoomSections({
  view,
  nick,
  onOpen,
}: {
  view: FriendsView;
  nick: (id: string) => string;
  onOpen: (room: string) => void;
}) {
  const unread = view.rooms.filter((r) => r.unread > 0);
  const read = view.rooms.filter((r) => r.unread === 0);
  const list = (rooms: ChatRoom[]) => (
    <ul className={styles.list}>
      {rooms.map((room) => (
        <li key={room.id}>
          <button
            type="button"
            className={styles.roomButton}
            data-unread={room.unread > 0 ? 'true' : undefined}
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
  );
  if (unread.length === 0) return list(read);
  return (
    <>
      <h3 className={styles.subtitle}>{t.unreadSection}</h3>
      {list(unread)}
      {read.length > 0 && (
        <>
          <h3 className={styles.subtitle}>{t.readSection}</h3>
          {list(read)}
        </>
      )}
    </>
  );
}

function Friends({
  view,
  settings,
  game,
  onOpen,
}: {
  view: FriendsView;
  settings: SettingsView;
  game: GameForInvites;
  onOpen: (room: string) => void;
}) {
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
      <label className={forms.choice}>
        <input
          type="checkbox"
          checked={view.visible}
          disabled={action.pending}
          onChange={(event) => {
            const visible = event.target.checked;
            void action.run(() => ipc.friendsSetVisible(visible));
          }}
        />
        {t.showOnline}
      </label>
      <label className={forms.choice}>
        <input
          type="checkbox"
          checked={settings.chatSound}
          disabled={action.pending || !settings.writable}
          onChange={(event) => {
            const on = event.target.checked;
            void action.run(() => ipc.setAppSettings({ chatSound: on }));
          }}
        />
        {t.chatSound}
      </label>
      <label className={forms.choice}>
        <input
          type="checkbox"
          checked={settings.chatNotifications}
          disabled={action.pending || !settings.writable}
          onChange={(event) => {
            const on = event.target.checked;
            void action.run(() => ipc.setAppSettings({ chatNotifications: on }));
          }}
        />
        {t.chatNotifications}
      </label>
      {view.friends.length === 0 ? (
        <p className={styles.muted}>{t.noFriends}</p>
      ) : (
        <ul className={styles.list}>
          {view.friends.map((person) => (
            <li key={person.id} className={styles.row}>
              <span className={styles.who}>
                <span className={styles.nick}>
                  <span
                    className={person.online ? styles.online : styles.away}
                    aria-hidden="true"
                  />
                  {person.nick}
                </span>
                {person.online && (
                  <span className={styles.status} dir="auto">
                    {person.playing !== null ? t.playing({ game: person.playing }) : t.online}
                  </span>
                )}
              </span>
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
                {game.invite !== null && (
                  <button
                    type="button"
                    className={styles.link}
                    disabled={action.pending}
                    onClick={() => {
                      const invite = game.invite;
                      if (invite === null) return;
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
                  </button>
                )}
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
          {view.incoming.length > 1 && (
            <Button
              disabled={action.pending}
              onClick={() => void action.run(() => ipc.friendsDeclineAll())}
            >
              {t.declineAll}
            </Button>
          )}
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
                  <button
                    type="button"
                    className={styles.quiet}
                    disabled={action.pending}
                    onClick={() => void action.run(() => ipc.friendsReportRequest(person.id))}
                  >
                    {t.spam}
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
