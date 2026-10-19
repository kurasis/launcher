import { useEffect, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import forms from '../../components/forms.module.css';
import type { ChatMessage, ChatRoom, FriendsView } from '../../lib/contracts';
import { friendsProblem, ipc, onFriendsMessage, onFriendsRemoved } from '../../lib/ipc';
import { numberLocale, strings } from '../../lib/strings';
import styles from './Friends.module.css';
import { problemText, roomTitle, sanctionText } from './useFriends';

const t = strings.friends;
/** The friends server's page of history. */
const PAGE = 50;
const MESSAGE_MAX = 2000;
const NAME_MAX = 40;

/** Adds messages to a room's history by id, oldest first, without twins. */
function merge(list: ChatMessage[], more: ChatMessage[]): ChatMessage[] {
  const byId = new Map(list.map((m) => [m.id, m]));
  for (const message of more) byId.set(message.id, message);
  return [...byId.values()].sort((a, b) => a.id - b.id);
}

function time(seconds: number): string {
  const date = new Date(seconds * 1000);
  const today = new Date().toDateString() === date.toDateString();
  return today
    ? date.toLocaleTimeString(numberLocale(), { hour: '2-digit', minute: '2-digit' })
    : date.toLocaleString(numberLocale(), {
        day: 'numeric',
        month: 'short',
        hour: '2-digit',
        minute: '2-digit',
      });
}

/**
 * One chat: its history (loaded a page at a time), new messages as they come,
 * and writing. Message text is always shown as text, never as markup or links.
 */
export function ChatView({
  room,
  view,
  onBack,
}: {
  room: ChatRoom;
  view: FriendsView;
  onBack: () => void;
}) {
  const me = view.me;
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [more, setMore] = useState(false);
  const [loading, setLoading] = useState(true);
  const [draft, setDraft] = useState('');
  const [leaving, setLeaving] = useState(false);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [reporting, setReporting] = useState<number | null>(null);
  const [reason, setReason] = useState('');
  const [reported, setReported] = useState<Set<number>>(new Set());
  const [showMembers, setShowMembers] = useState(false);
  const [newName, setNewName] = useState<string | null>(null);
  const [adding, setAdding] = useState('');
  const list = useRef<HTMLOListElement>(null);
  const readUpTo = useRef(0);
  const stick = useRef(true);

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

  // Keep the newest message in sight unless the player scrolled up to read.
  const last = messages.at(-1)?.id ?? 0;
  useEffect(() => {
    const element = list.current;
    if (element !== null && stick.current) element.scrollTop = element.scrollHeight;
  }, [last]);

  // What is on screen counts as read.
  useEffect(() => {
    if (last > readUpTo.current && room.unread > 0) {
      readUpTo.current = last;
      void ipc.friendsRead(room.id, last).catch(() => undefined);
    }
  }, [last, room.id, room.unread]);

  const isGroup = room.kind === 'group';
  const owner = isGroup && me !== null && room.owner === me.id;
  const other = isGroup ? null : room.members.find((m) => m.id !== me?.id);
  // Two people who are no longer friends keep their history but write no more.
  const readOnly =
    other !== undefined && other !== null && !view.friends.some((f) => f.id === other.id);
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
    const oldest = messages[0]?.id ?? null;
    stick.current = false;
    await run(async () => {
      const page = await ipc.friendsMessages(room.id, oldest);
      setMessages((current) => merge(current, page));
      setMore(page.length >= PAGE);
    });
  };

  const send = async () => {
    const body = draft.trim();
    if (body.length === 0 || sending) return;
    setSending(true);
    stick.current = true;
    const ok = await run(async () => {
      const message = await ipc.friendsSend(room.id, body);
      setMessages((current) => merge(current, [message]));
    });
    if (ok) setDraft('');
    setSending(false);
  };

  return (
    <div className={styles.chat}>
      <div className={styles.chatHead}>
        <button type="button" className={styles.link} onClick={onBack}>
          {t.back}
        </button>
        <h3 className={styles.chatTitle}>{roomTitle(room, me)}</h3>
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
      </div>

      {isGroup && showMembers && (
        <div className={styles.members}>
          <ul className={styles.list}>
            {room.members.map((member) => (
              <li key={member.id} className={styles.row}>
                <span className={styles.nick}>
                  {member.nick}
                  {member.id === room.owner && ' ★'}
                </span>
                {owner && member.id !== me.id && (
                  <button
                    type="button"
                    className={styles.link}
                    onClick={() =>
                      void run(() => ipc.friendsChangeRoom(room.id, 'remove', member.id))
                    }
                  >
                    {t.removeMember}
                  </button>
                )}
              </li>
            ))}
          </ul>
          {owner && addable.length > 0 && (
            <div className={styles.inline}>
              <select
                className={forms.select}
                aria-label={t.addMember}
                value={adding}
                onChange={(event) => {
                  setAdding(event.target.value);
                }}
              >
                <option value="">{t.addMember}</option>
                {addable.map((friend) => (
                  <option key={friend.id} value={friend.id}>
                    {friend.nick}
                  </option>
                ))}
              </select>
              <Button
                disabled={adding === ''}
                onClick={() =>
                  void run(() => ipc.friendsChangeRoom(room.id, 'add', adding)).then((ok) => {
                    if (ok) setAdding('');
                  })
                }
              >
                {t.add}
              </Button>
            </div>
          )}
          {owner &&
            (newName === null ? (
              <button
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
              >
                <input
                  className={forms.input}
                  aria-label={t.groupName}
                  value={newName}
                  maxLength={NAME_MAX}
                  onChange={(event) => {
                    setNewName(event.target.value);
                  }}
                />
                <Button type="submit" disabled={newName.trim().length === 0}>
                  {t.rename}
                </Button>
              </form>
            ))}
          {me !== null && !leaving && (
            <button
              type="button"
              className={styles.danger}
              onClick={() => {
                setLeaving(true);
              }}
            >
              {t.leave}
            </button>
          )}
          {me !== null && leaving && (
            <div className={styles.confirm} role="alertdialog" aria-label={t.leave}>
              <p>{t.confirmLeave}</p>
              <div className={styles.inline}>
                <Button
                  variant="primary"
                  onClick={() =>
                    void run(() => ipc.friendsChangeRoom(room.id, 'remove', me.id)).then((ok) => {
                      setLeaving(false);
                      if (ok) onBack();
                    })
                  }
                >
                  {t.leave}
                </Button>
                <Button
                  onClick={() => {
                    setLeaving(false);
                  }}
                >
                  {t.cancel}
                </Button>
              </div>
            </div>
          )}
        </div>
      )}

      <ol
        className={styles.messages}
        ref={list}
        aria-label={roomTitle(room, me)}
        onScroll={(event) => {
          const element = event.currentTarget;
          stick.current = element.scrollHeight - element.scrollTop - element.clientHeight < 40;
        }}
      >
        {more && (
          <li className={styles.earlier}>
            <button type="button" className={styles.link} onClick={() => void earlier()}>
              {t.earlier}
            </button>
          </li>
        )}
        {!loading && messages.length === 0 && error === null && (
          <li className={styles.muted}>{t.noMessages}</li>
        )}
        {messages.map((message) => {
          const mine = message.sender === me?.id;
          return (
            <li key={message.id} className={styles.message} data-mine={mine ? 'true' : undefined}>
              <p className={styles.meta}>
                <strong>{nick(message.sender)}</strong>{' '}
                <time dateTime={new Date(message.created * 1000).toISOString()}>
                  {time(message.created)}
                </time>
              </p>
              <p className={styles.body} dir="auto">
                {message.body}
              </p>
              {!mine &&
                (reported.has(message.id) ? (
                  <p className={styles.muted}>{t.reported}</p>
                ) : reporting === message.id ? (
                  <form
                    className={styles.inline}
                    onSubmit={(event) => {
                      event.preventDefault();
                      void run(() => ipc.friendsReport(message.id, reason.trim())).then((ok) => {
                        if (!ok) return;
                        setReported((current) => new Set(current).add(message.id));
                        setReporting(null);
                        setReason('');
                      });
                    }}
                  >
                    <input
                      className={forms.input}
                      aria-label={t.reportReason}
                      placeholder={t.reportReason}
                      // Opening the form brings it into view and ready to type.
                      autoFocus
                      value={reason}
                      maxLength={500}
                      onChange={(event) => {
                        setReason(event.target.value);
                      }}
                    />
                    <Button type="submit">{t.report}</Button>
                  </form>
                ) : (
                  <button
                    type="button"
                    className={styles.quiet}
                    onClick={() => {
                      setReporting(message.id);
                      setReason('');
                    }}
                  >
                    {t.report}
                  </button>
                ))}
            </li>
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
            void send();
          }}
        >
          <textarea
            className={[forms.input, styles.draft].join(' ')}
            aria-label={t.write}
            placeholder={t.write}
            title={t.sendHint}
            rows={2}
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
          <Button type="submit" variant="primary" disabled={sending || draft.trim().length === 0}>
            {t.send}
          </Button>
        </form>
      )}
    </div>
  );
}
