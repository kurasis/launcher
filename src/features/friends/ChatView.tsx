import { Fragment, useEffect, useId, useRef, useState } from 'react';
import { type MenuItem, MoreMenu } from '../../app/MoreMenu';
import { Button } from '../../components/Button';
import { Icon } from '../../components/Icon';
import forms from '../../components/forms.module.css';
import type { ChatMessage, ChatRoom, FriendsView } from '../../lib/contracts';
import { friendsProblem, ipc, onFriendsMessage, onFriendsRemoved } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { around } from '../downloads/operationText';
import { isoTime, messageTime } from './chatTime';
import { Confirm } from '../../components/Confirm';
import { focusLost, useRowFocus } from './focus';
import styles from './Friends.module.css';
import { type GameForInvites, InviteCard } from './InviteCard';
import { inviteBody, parseInvite } from './invite';
import { PlayerHead, Presence, RoomAvatar } from './People';
import { Spinner } from './Spinner';
import { WorldInviteCard } from './WorldInviteCard';
import { parseWorldInvite } from './worldInvite';
import { problemText, roomTitle, sanctionText } from './useFriends';

const t = strings.friends;
/** The friends server's page of history. */
const PAGE = 50;
const MESSAGE_MAX = 2000;
const NAME_MAX = 40;
const REASON_MAX = 500;

/** Adds messages to a room's history by id, oldest first, without twins. */
function merge(list: ChatMessage[], more: ChatMessage[]): ChatMessage[] {
  const byId = new Map(list.map((m) => [m.id, m]));
  for (const message of more) byId.set(message.id, message);
  return [...byId.values()].sort((a, b) => a.id - b.id);
}

/** A question asked before a step that cannot be taken back. */
type Asking = 'leave' | 'remove' | 'block';

/**
 * One chat in the conversation pane: who it is with, its history (loaded a page at a
 * time), new messages as they come, and writing. Message text is always shown as text,
 * never as markup or links.
 */
export function ChatView({
  room,
  view,
  game,
  heads,
  onLeft,
}: {
  room: ChatRoom;
  view: FriendsView;
  game: GameForInvites;
  heads: ReadonlyMap<string, string>;
  /** The player left this group. */
  onLeft: () => void;
}) {
  const me = view.me;
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [more, setMore] = useState(false);
  const [loading, setLoading] = useState(true);
  const [draft, setDraft] = useState('');
  // The question asked, and the button that asked it: the focus goes back to it.
  const [asking, setAsking] = useState<{ what: Asking; by: HTMLElement | null } | null>(null);
  const [answering, setAnswering] = useState(false);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [reporting, setReporting] = useState<number | null>(null);
  const [reason, setReason] = useState('');
  const [reported, setReported] = useState<Set<number>>(new Set());
  const [showMembers, setShowMembers] = useState(false);
  const [loadingEarlier, setLoadingEarlier] = useState(false);
  // What was unread when the chat opened stays marked while it is open.
  const [unreadAtOpen] = useState(room.unread);
  const divider = useRef<HTMLLIElement>(null);
  const [newName, setNewName] = useState<string | null>(null);
  const [adding, setAdding] = useState('');
  const list = useRef<HTMLOListElement>(null);
  const box = useRef<HTMLTextAreaElement>(null);
  const renameLink = useRef<HTMLButtonElement>(null);
  const addSelect = useRef<HTMLSelectElement>(null);
  const memberList = useRef<HTMLUListElement>(null);
  const memberFocus = useRowFocus(memberList, room.members);
  // The message whose report form just closed, and whether earlier messages were asked
  // for: the focus goes on from there when what had it goes away.
  const reportClosed = useRef<number | null>(null);
  const askedEarlier = useRef(false);
  const readUpTo = useRef(0);
  const stick = useRef(true);
  const hintId = useId();
  // The control in the chat that had the focus last.
  const lastFocus = useRef<Element | null>(null);

  useEffect(() => {
    let live = true;
    const stop = onFriendsMessage((message) => {
      if (live && message.room === room.id) setMessages((current) => merge(current, [message]));
    });
    // A moderator removed a message: it leaves the chat at once.
    const stopRemoved = onFriendsRemoved((removed) => {
      if (live && removed.room === room.id) {
        setMessages((current) => current.filter((m) => m.id !== removed.message));
      }
    });
    ipc
      .friendsMessages(room.id, null)
      .then((page) => {
        if (!live) return;
        setMessages((current) => merge(current, page));
        setMore(page.length >= PAGE);
      })
      .catch((e: unknown) => {
        if (live) setError(problemText(friendsProblem(e)));
      })
      .finally(() => {
        if (live) setLoading(false);
      });
    return () => {
      live = false;
      stop();
      stopRemoved();
    };
  }, [room.id]);

  // Keep the newest message in sight unless the player scrolled up to read, and bring
  // it into sight when it is mine; on opening, the first unread message comes into
  // sight instead, and stays there until the player scrolls or a message comes.
  const last = messages.at(-1)?.id ?? 0;
  const lastMine = me !== null && messages.at(-1)?.sender === me.id;
  const shownFirstUnread = useRef(false);
  /** Where the first unread message was put in sight, while the player has not scrolled. */
  const pinned = useRef<number | null>(null);
  const showFirstUnread = (element: HTMLOListElement, line: HTMLLIElement) => {
    element.scrollTop = Math.max(0, line.offsetTop - element.offsetTop - 8);
    pinned.current = element.scrollTop;
    stick.current = element.scrollHeight - element.scrollTop - element.clientHeight < 40;
  };
  useEffect(() => {
    const element = list.current;
    if (element === null) return;
    if (!shownFirstUnread.current && divider.current !== null) {
      shownFirstUnread.current = true;
      showFirstUnread(element, divider.current);
      return;
    }
    pinned.current = null;
    if (stick.current || lastMine) {
      element.scrollTop = element.scrollHeight;
      stick.current = true;
    }
  }, [last, lastMine]);

  // What grows in the list after it was drawn (a world's facts coming in) keeps the
  // first unread message in sight while it is there, else the newest one while it was.
  useEffect(() => {
    const element = list.current;
    if (element === null || typeof MutationObserver === 'undefined') return;
    const observer = new MutationObserver(() => {
      if (pinned.current !== null && divider.current !== null) {
        showFirstUnread(element, divider.current);
      } else if (stick.current) {
        element.scrollTop = element.scrollHeight;
      }
    });
    observer.observe(element, { childList: true, subtree: true, characterData: true });
    return () => {
      observer.disconnect();
    };
  }, []);

  // A closed form gives the focus back to what opened it, or to what stands in its place.
  useEffect(() => {
    if (newName === null && focusLost()) renameLink.current?.focus();
  }, [newName]);
  useEffect(() => {
    const id = reportClosed.current;
    reportClosed.current = null;
    if (reporting !== null || id === null || !focusLost()) return;
    list.current?.querySelector<HTMLElement>(`[data-report="${String(id)}"]`)?.focus();
  }, [reporting]);
  // The last page came: "Earlier messages" went, and the list keeps the focus.
  useEffect(() => {
    if (more || !askedEarlier.current) return;
    askedEarlier.current = false;
    if (focusLost()) list.current?.focus();
  }, [more]);

  // Whatever else went with the focus (a message a moderator removed, a card's button):
  // the list takes it, so Tab and Escape go on working.
  useEffect(() => {
    const gone = lastFocus.current;
    if (gone === null || gone.isConnected || !focusLost()) return;
    lastFocus.current = null;
    list.current?.focus();
  });

  // What is on screen counts as read.
  useEffect(() => {
    if (last > readUpTo.current && room.unread > 0) {
      readUpTo.current = last;
      void ipc.friendsRead(room.id, last).catch(() => undefined);
    }
  }, [last, room.id, room.unread]);

  const isGroup = room.kind === 'group';
  const owner = isGroup && me !== null && room.owner === me.id;
  const other = isGroup ? undefined : room.members.find((m) => m.id !== me?.id);
  const friend = other === undefined ? undefined : view.friends.find((f) => f.id === other.id);
  // Two people who are no longer friends keep their history but write no more.
  const readOnly = other !== undefined && friend === undefined;
  const canWrite = view.sanction === null && !readOnly;
  const title = roomTitle(room, me);
  const nick = (id: string) =>
    id === me?.id ? t.you : (room.members.find((m) => m.id === id)?.nick ?? '—');
  const addable = view.friends.filter((f) => !room.members.some((m) => m.id === f.id));

  const run = async (work: () => Promise<unknown>) => {
    setError(null);
    try {
      await work();
      return true;
    } catch (e) {
      setError(problemText(friendsProblem(e)));
      return false;
    }
  };

  const earlier = async () => {
    if (loadingEarlier) return;
    const oldest = messages[0]?.id ?? null;
    stick.current = false;
    askedEarlier.current = true;
    setLoadingEarlier(true);
    await run(async () => {
      const page = await ipc.friendsMessages(room.id, oldest);
      setMessages((current) => merge(current, page));
      setMore(page.length >= PAGE);
    });
    setLoadingEarlier(false);
  };

  // The first message that was unread when the chat opened: the last
  // `unreadAtOpen` messages from others.
  let firstUnread: number | null = null;
  if (unreadAtOpen > 0) {
    let count = 0;
    for (const message of [...messages].reverse()) {
      if (message.sender !== me?.id) count++;
      if (count === unreadAtOpen) {
        firstUnread = message.id;
        break;
      }
    }
  }

  // Messages that are invites to a server or to a world, shown as cards.
  const invites = new Map(
    messages.flatMap((m) => {
      const invite = parseInvite(m.body);
      return invite === null ? [] : [[m.id, invite] as const];
    }),
  );
  const worldInvites = new Map(
    messages.flatMap((m) => {
      const code = parseWorldInvite(m.body);
      return code === null ? [] : [[m.id, code] as const];
    }),
  );

  const sendInvite = async () => {
    if (game.invite === null || sending) return;
    setSending(true);
    const body = inviteBody(game.invite);
    await run(async () => {
      const message = await ipc.friendsSend(room.id, body);
      setMessages((current) => merge(current, [message]));
    });
    setSending(false);
  };

  const stopReporting = () => {
    reportClosed.current = reporting;
    setReporting(null);
    setReason('');
  };

  const send = async () => {
    const body = draft.trim();
    if (body.length === 0 || sending) return;
    setSending(true);
    const ok = await run(async () => {
      const message = await ipc.friendsSend(room.id, body);
      setMessages((current) => merge(current, [message]));
    });
    if (ok) setDraft('');
    setSending(false);
  };

  const ask = (what: Asking) => {
    if (answering) return;
    const by = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    setAsking({ what, by });
  };
  const closeQuestion = () => {
    const by = asking?.by ?? null;
    setAsking(null);
    if (by?.isConnected === true) by.focus();
  };
  const answer = async () => {
    if (asking === null || answering || me === null) return;
    const { what } = asking;
    setAnswering(true);
    const ok = await run(() =>
      what === 'leave'
        ? ipc.friendsChangeRoom(room.id, 'remove', me.id)
        : other === undefined
          ? Promise.resolve()
          : ipc.friendsAct(what, other.id),
    );
    setAnswering(false);
    closeQuestion();
    if (ok && what === 'leave') onLeft();
  };

  const entries: MenuItem[] = [];
  if (canWrite && game.invite !== null) {
    entries.push({
      kind: 'item',
      key: 'invite',
      label: t.inviteToServer({ server: game.invite.server }),
      onSelect: () => {
        void sendInvite();
      },
    });
  }
  if (isGroup && me !== null) {
    entries.push({
      kind: 'item',
      key: 'leave',
      label: t.leave,
      onSelect: () => {
        ask('leave');
      },
    });
  }
  if (other !== undefined) {
    if (friend !== undefined) {
      entries.push({
        kind: 'item',
        key: 'remove',
        label: t.removeFriend,
        onSelect: () => {
          ask('remove');
        },
      });
    }
    entries.push({
      kind: 'item',
      key: 'block',
      label: t.block,
      onSelect: () => {
        ask('block');
      },
    });
  }

  return (
    <div
      className={styles.chat}
      onFocus={(event) => {
        lastFocus.current = event.target;
      }}
    >
      <div className={styles.chatHead}>
        <RoomAvatar room={room} me={me} heads={heads} size={28} />
        <h3 className={styles.chatTitle} dir="auto">
          {title}
        </h3>
        {friend !== undefined && <Presence person={friend} />}
        {isGroup && (
          <button
            type="button"
            className={styles.link}
            aria-expanded={showMembers}
            onClick={() => {
              setShowMembers(!showMembers);
            }}
          >
            {t.members({ count: room.members.length })}
          </button>
        )}
        {entries.length > 0 && (
          <div className={styles.chatMenu}>
            <MoreMenu
              label={t.chatActions}
              ariaLabel={t.chatActions}
              icon={<Icon name="more" size={18} />}
              caret={false}
              current={false}
              buttonClassName={styles.iconButton}
              entries={entries}
            />
          </div>
        )}
      </div>

      {asking !== null && (
        <Confirm
          // Another question is a new one: it takes the focus and is read out.
          key={asking.what}
          label={asking.what === 'leave' ? t.leave : (other?.nick ?? title)}
          text={
            asking.what === 'leave'
              ? t.confirmLeave
              : asking.what === 'block'
                ? t.confirmBlock({ nick: other?.nick ?? title })
                : t.confirmRemove({ nick: other?.nick ?? title })
          }
          action={asking.what === 'leave' ? t.leave : asking.what === 'block' ? t.block : t.remove}
          cancel={t.cancel}
          pending={answering}
          onYes={() => void answer()}
          onNo={closeQuestion}
        />
      )}

      {isGroup && showMembers && (
        <div className={styles.members}>
          <ul className={styles.people} ref={memberList} {...memberFocus}>
            {room.members.map((member) => (
              <li key={member.id} className={styles.person}>
                <PlayerHead nick={member.nick} heads={heads} size={24} />
                <span className={styles.who}>
                  <span className={styles.nick}>
                    {member.nick}
                    {member.id === room.owner && ' ★'}
                  </span>
                </span>
                {owner && member.id !== me.id && (
                  <span className={styles.actions}>
                    <Button
                      size="small"
                      variant="ghost"
                      onClick={() =>
                        void run(() => ipc.friendsChangeRoom(room.id, 'remove', member.id))
                      }
                    >
                      {t.removeMember}
                    </Button>
                  </span>
                )}
              </li>
            ))}
          </ul>
          {owner && addable.length > 0 && (
            <div className={styles.inline}>
              <select
                ref={addSelect}
                className={forms.select}
                aria-label={t.addMember}
                value={adding}
                onChange={(event) => {
                  setAdding(event.target.value);
                }}
              >
                <option value="">{t.addMember}</option>
                {addable.map((person) => (
                  <option key={person.id} value={person.id}>
                    {person.nick}
                  </option>
                ))}
              </select>
              <Button
                size="small"
                disabled={adding === ''}
                onClick={(event) => {
                  const by = event.currentTarget;
                  void run(() => ipc.friendsChangeRoom(room.id, 'add', adding)).then((ok) => {
                    if (!ok) return;
                    // The emptied choice turns Add off: the focus, if still on it, goes to
                    // the choice. Where the player went meanwhile, it stays.
                    if (document.activeElement === by || focusLost()) addSelect.current?.focus();
                    setAdding('');
                  });
                }}
              >
                {t.addToGroup}
              </Button>
            </div>
          )}
          {owner &&
            (newName === null ? (
              <button
                ref={renameLink}
                type="button"
                className={styles.link}
                onClick={() => {
                  setNewName(room.name);
                }}
              >
                {t.rename}
              </button>
            ) : (
              <form
                className={styles.inline}
                onSubmit={(event) => {
                  event.preventDefault();
                  void run(() => ipc.friendsChangeRoom(room.id, 'name', newName.trim())).then(
                    (ok) => {
                      if (ok) setNewName(null);
                    },
                  );
                }}
                onKeyDown={(event) => {
                  // Escape closes the renaming, not the whole sheet.
                  if (event.key === 'Escape') {
                    event.preventDefault();
                    event.stopPropagation();
                    setNewName(null);
                  }
                }}
              >
                <input
                  className={forms.input}
                  aria-label={t.groupName}
                  // The Rename link that opened it is gone: the field takes the focus.
                  autoFocus
                  value={newName}
                  maxLength={NAME_MAX}
                  onChange={(event) => {
                    setNewName(event.target.value);
                  }}
                />
                <Button type="submit" size="small" disabled={newName.trim().length === 0}>
                  {t.rename}
                </Button>
              </form>
            ))}
        </div>
      )}

      <ol
        className={styles.messages}
        ref={list}
        aria-label={title}
        // Takes the focus when what had it in the list went away.
        tabIndex={-1}
        onScroll={(event) => {
          const element = event.currentTarget;
          // The player moved it: the first unread message is theirs to keep in sight.
          if (pinned.current !== null && Math.abs(element.scrollTop - pinned.current) > 1) {
            pinned.current = null;
          }
          stick.current = element.scrollHeight - element.scrollTop - element.clientHeight < 40;
        }}
      >
        {more && (
          <li className={styles.earlier}>
            {/* It stays while the page comes, keeping the focus. */}
            <button
              type="button"
              className={styles.link}
              aria-disabled={loadingEarlier || undefined}
              onClick={() => void earlier()}
            >
              {loadingEarlier && <span className={styles.spinner} aria-hidden="true" />}
              {loadingEarlier ? t.loadingMessages : t.earlier}
            </button>
          </li>
        )}
        {loading && messages.length === 0 && (
          <li>
            <Spinner label={t.loadingMessages} />
          </li>
        )}
        {!loading && messages.length === 0 && error === null && (
          <li className={styles.muted}>{t.noMessages}</li>
        )}
        {messages.map((message) => {
          const mine = message.sender === me?.id;
          const invite = invites.get(message.id);
          const worldCode = worldInvites.get(message.id);
          const sender = nick(message.sender);
          // Only the time is a <time>; the words around it are the language's.
          const [before, after] = around((value) => t.messageMeta({ nick: sender, time: value }));
          return (
            <Fragment key={message.id}>
              {message.id === firstUnread && (
                <li className={styles.newDivider} ref={divider} role="separator">
                  <span>{t.newMessages}</span>
                </li>
              )}
              <li className={styles.message} data-mine={mine ? 'true' : undefined}>
                <p className={styles.meta}>
                  {before}
                  <time dateTime={isoTime(message.created)}>{messageTime(message.created)}</time>
                  {after}
                </p>
                {invite !== undefined ? (
                  <InviteCard invite={invite} mine={mine} game={game} from={sender} />
                ) : worldCode !== undefined ? (
                  <WorldInviteCard
                    code={worldCode}
                    from={sender}
                    mine={mine}
                    onOpen={game.openWorld}
                  />
                ) : (
                  <p className={styles.bubble} dir="auto">
                    {message.body}
                  </p>
                )}
                {!mine &&
                  (reported.has(message.id) ? (
                    <p
                      className={styles.muted}
                      role="status"
                      tabIndex={-1}
                      data-report={message.id}
                    >
                      {t.reported}
                    </p>
                  ) : reporting === message.id ? (
                    <form
                      className={styles.reportForm}
                      onSubmit={(event) => {
                        event.preventDefault();
                        void run(() => ipc.friendsReport(message.id, reason.trim())).then((ok) => {
                          if (!ok) return;
                          setReported((current) => new Set(current).add(message.id));
                          stopReporting();
                        });
                      }}
                      onKeyDown={(event) => {
                        // Escape closes the report, not the whole panel.
                        if (event.key === 'Escape') {
                          event.preventDefault();
                          event.stopPropagation();
                          stopReporting();
                        }
                      }}
                    >
                      <input
                        className={forms.input}
                        aria-label={t.reportReason}
                        placeholder={t.reportReason}
                        // Opening the form brings it into view and ready to type.
                        autoFocus
                        value={reason}
                        maxLength={REASON_MAX}
                        onChange={(event) => {
                          setReason(event.target.value);
                        }}
                      />
                      <Button type="submit" size="small">
                        {t.report}
                      </Button>
                      <Button size="small" onClick={stopReporting}>
                        {t.cancel}
                      </Button>
                    </form>
                  ) : (
                    <button
                      type="button"
                      className={styles.report}
                      data-report={message.id}
                      onClick={() => {
                        setReporting(message.id);
                        setReason('');
                      }}
                    >
                      {t.report}
                    </button>
                  ))}
              </li>
            </Fragment>
          );
        })}
      </ol>

      {error !== null && (
        <p className={styles.error} role="alert">
          {error}
        </p>
      )}

      {view.sanction !== null ? (
        <p className={styles.muted} role="status">
          {sanctionText(view.sanction)}
        </p>
      ) : readOnly ? (
        <p className={styles.muted}>{t.readOnly}</p>
      ) : (
        <form
          className={styles.composer}
          onSubmit={(event) => {
            event.preventDefault();
            void send().then(() => {
              box.current?.focus();
            });
          }}
        >
          <div className={styles.composerRow}>
            <textarea
              ref={box}
              className={[forms.input, styles.draft].join(' ')}
              aria-label={t.write}
              aria-describedby={hintId}
              placeholder={t.write}
              rows={1}
              dir="auto"
              maxLength={MESSAGE_MAX}
              value={draft}
              onChange={(event) => {
                setDraft(event.target.value);
              }}
              onKeyDown={(event) => {
                if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) {
                  event.preventDefault();
                  void send();
                }
              }}
            />
            <Button
              type="submit"
              variant="primary"
              className={styles.send}
              aria-label={t.send}
              title={t.send}
              // Never disabled: the focus stays on it while the message goes.
              aria-disabled={sending || draft.trim().length === 0 || undefined}
            >
              <Icon name="send" size={18} className={styles.mirror} />
            </Button>
          </div>
          <p id={hintId} className={styles.hint}>
            {t.sendHint}
          </p>
        </form>
      )}
    </div>
  );
}
