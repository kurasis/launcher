import { type Dispatch, type SetStateAction, useEffect, useId, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { coveredByDialog, inDialog } from '../../components/dialogStack';
import { Icon } from '../../components/Icon';
import { Notice } from '../../components/Notice';
import type { AppSnapshot, FriendsView, SettingsView } from '../../lib/contracts';
import { onFriendsMessage } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { usePlayerHeads } from '../skins/usePlayerHeads';
import { ChatList } from './ChatList';
import { ChatView } from './ChatView';
import { ConfirmEmail } from './ConfirmEmail';
import { focusLost } from './focus';
import styles from './Friends.module.css';
import { FriendsTab } from './FriendsTab';
import type { GameForInvites } from './InviteCard';
import { NewGroup } from './NewGroup';
import { RequestsTab } from './RequestsTab';
import { playChime, unlockSoundOnFirstInput } from './sound';
import { Spinner } from './Spinner';
import { problemText, sanctionText, useFriends } from './useFriends';

const t = strings.friends;
type Tab = 'chats' | 'friends' | 'requests';
const TABS: readonly Tab[] = ['chats', 'friends', 'requests'];

/** The running game's server as an invite, and the installations to join with. */
function gameForInvites(snapshot: AppSnapshot, openWorld: (room: string) => void): GameForInvites {
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
    openWorld,
  };
}

/** Where the sheet was: it opens again on the same tab and chat. */
interface Place {
  tab: Tab;
  room: string | null;
  /** A new group is being made in the conversation pane. */
  grouping: boolean;
}

/**
 * Friends and chats of the launcher account (D-064): a row of concept A's rail with a
 * count of unread messages and requests, and the whole friends system in a side sheet
 * at the window's inline end (D-072).
 */
export function FriendsCorner({
  snapshot,
  onOpenAccounts,
  onOpenWorld,
}: {
  snapshot: AppSnapshot;
  onOpenAccounts: () => void;
  /** Shows a room in Worlds, such as one joined from a chat invite. */
  onOpenWorld: (room: string) => void;
}) {
  const settings = snapshot.settings;
  const [open, setOpen] = useState(false);
  const [place, setPlace] = useState<Place>({ tab: 'chats', room: null, grouping: false });
  const buttonRef = useRef<HTMLButtonElement>(null);
  const slot = useRef<HTMLDivElement>(null);
  const game = gameForInvites(snapshot, (room) => {
    setOpen(false);
    onOpenWorld(room);
  });
  const view = useFriends();
  // The chat on screen makes no sound for its own messages; none is while a new group
  // is being made in its place.
  useChime(view, settings, open && place.tab === 'chats' && !place.grouping ? place.room : null);

  // Like a press on the scrim, the focus or a press going anywhere outside the sheet and
  // its row puts the sheet away: nothing under it can be reached while it is open. A
  // dialog over it (a question from the launcher) keeps it, and so does a lost focus.
  // Escape closes it from anywhere, unless a control in it took the key (a menu, a
  // question) or a dialog is open (the key is the dialog's).
  useEffect(() => {
    if (!open) return;
    const elsewhere = (target: EventTarget | null) =>
      target instanceof Node && !(slot.current?.contains(target) ?? true) && !inDialog(target);
    const onFocus = (event: FocusEvent) => {
      if (elsewhere(event.target)) setOpen(false);
    };
    const onPress = (event: PointerEvent) => {
      if (elsewhere(event.target)) setOpen(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || event.defaultPrevented || event.isComposing) return;
      if (coveredByDialog(null)) return;
      setOpen(false);
      buttonRef.current?.focus();
    };
    document.addEventListener('focusin', onFocus, true);
    document.addEventListener('pointerdown', onPress, true);
    window.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('focusin', onFocus, true);
      document.removeEventListener('pointerdown', onPress, true);
      window.removeEventListener('keydown', onKey);
    };
  }, [open]);

  if (view === null) return null;
  const waiting = view.unread + view.incoming.length;
  // Closing puts focus back on the rail's row.
  const close = () => {
    setOpen(false);
    buttonRef.current?.focus();
  };
  const name = [
    t.open,
    view.signedIn && view.unread > 0 && t.unread({ count: view.unread }),
    view.signedIn && view.incoming.length > 0 && t.requestsWaiting({ count: view.incoming.length }),
  ]
    .filter(Boolean)
    .join(', ');
  return (
    <div ref={slot} className={styles.railSlot}>
      <button
        ref={buttonRef}
        type="button"
        className={styles.railButton}
        aria-label={name}
        aria-expanded={open}
        title={t.open}
        onClick={() => {
          if (open) close();
          else setOpen(true);
        }}
      >
        <Icon name="friends" className={styles.railIcon} />
        <span className={styles.railLabel} aria-hidden="true">
          {t.title}
        </span>
        {view.signedIn && waiting > 0 && (
          <span className={styles.badge}>{waiting > 99 ? '99+' : waiting}</span>
        )}
      </button>
      {/* After its row, so Tab goes from one into the other. */}
      {open && (
        <>
          <FriendsSheet
            view={view}
            settings={settings}
            game={game}
            place={place}
            onPlace={setPlace}
            onOpenAccounts={() => {
              setOpen(false);
              onOpenAccounts();
            }}
            onClose={close}
          />
          <div className={styles.scrim} aria-hidden="true" onClick={close} />
        </>
      )}
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

/** Every player the sheet may show a face of, once. */
function nicksOf(view: FriendsView): string[] {
  const people = [...view.friends, ...view.incoming, ...view.outgoing, ...view.blocked];
  const members = view.rooms.flatMap((room) => room.members);
  return [...people, ...members]
    .filter((person) => person.id !== view.me?.id)
    .map((person) => person.nick);
}

function FriendsSheet({
  view,
  settings,
  game,
  place,
  onPlace,
  onOpenAccounts,
  onClose,
}: {
  view: FriendsView;
  settings: SettingsView;
  game: GameForInvites;
  place: Place;
  onPlace: Dispatch<SetStateAction<Place>>;
  onOpenAccounts: () => void;
  onClose: () => void;
}) {
  const sheet = useRef<HTMLElement>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const plus = useRef<HTMLButtonElement>(null);
  const pane = useRef<HTMLDivElement>(null);
  // The control that had the focus last, and where it goes when the new group's form goes.
  const lastFocus = useRef<HTMLElement | null>(null);
  const afterGroup = useRef<'chat' | 'plus' | null>(null);
  const [needsShown, setNeedsShown] = useState(false);
  const headingId = useId();
  const tabsId = useId();
  const heads = usePlayerHeads(nicksOf(view));
  // Opening the sheet moves the focus into it.
  useEffect(() => {
    heading.current?.focus();
  }, []);
  // The new group's form went: the focus goes to the group made, or back to "+".
  const { grouping } = place;
  useEffect(() => {
    const next = afterGroup.current;
    afterGroup.current = null;
    if (grouping || next === null || !focusLost()) return;
    const target = next === 'chat' ? pane.current?.querySelector('textarea') : plus.current;
    target?.focus();
  }, [grouping]);
  // Whatever else took the focus away with it (a chat left, the last request answered):
  // the open tab takes it, so Tab and Escape go on working in the sheet.
  useEffect(() => {
    const gone = lastFocus.current;
    if (gone === null || gone.isConnected || !focusLost()) return;
    const tab = sheet.current?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]');
    (tab ?? heading.current)?.focus();
  });
  const unconfirmed = view.problem === 'email_not_confirmed';
  const ready = view.signedIn && view.problem === null && view.me !== null;
  const { tab } = place;
  const room = place.room === null ? undefined : view.rooms.find((r) => r.id === place.room);
  const show = (next: Partial<Place>) => {
    onPlace((current) => ({ ...current, ...next }));
  };
  const openRoom = (id: string) => {
    onPlace({ tab: 'chats', room: id, grouping: false });
  };
  const fewFriends = view.friends.length < 2;

  let body;
  if (unconfirmed) {
    body = <ConfirmEmail onOpenAccounts={onOpenAccounts} />;
  } else if (!view.signedIn) {
    body = (
      <div className={styles.state}>
        <p>{t.signedOut}</p>
        <Button variant="primary" onClick={onOpenAccounts}>
          {t.goToAccounts}
        </Button>
      </div>
    );
  } else {
    const label = (item: Tab) => {
      const count = item === 'chats' ? view.unread : item === 'requests' ? view.incoming.length : 0;
      return count > 0 ? t.tabCount({ label: t.tabs[item], count }) : t.tabs[item];
    };
    body = (
      <>
        <div
          role="tablist"
          aria-label={t.title}
          className={styles.tabs}
          onKeyDown={(event) => {
            const keys: Record<string, number> = { ArrowLeft: -1, ArrowRight: 1 };
            let step = keys[event.key];
            const at = TABS.indexOf(tab);
            let next: Tab | undefined;
            if (event.key === 'Home') next = TABS[0];
            else if (event.key === 'End') next = TABS[TABS.length - 1];
            else if (step !== undefined) {
              if (getComputedStyle(event.currentTarget).direction === 'rtl') step = -step;
              next = TABS[(at + step + TABS.length) % TABS.length];
            }
            if (next === undefined) return;
            event.preventDefault();
            show({ tab: next });
            event.currentTarget.querySelector<HTMLElement>(`[data-tab="${next}"]`)?.focus();
          }}
        >
          {TABS.map((item) => (
            <button
              key={item}
              type="button"
              role="tab"
              id={`${tabsId}-${item}`}
              data-tab={item}
              aria-selected={tab === item}
              aria-controls={`${tabsId}-panel`}
              tabIndex={tab === item ? 0 : -1}
              className={styles.tab}
              onClick={() => {
                show({ tab: item });
              }}
            >
              {label(item)}
            </button>
          ))}
        </div>
        <div
          role="tabpanel"
          id={`${tabsId}-panel`}
          aria-labelledby={`${tabsId}-${tab}`}
          className={styles.tabPanel}
        >
          {tab === 'chats' && (
            <div className={styles.chats} data-empty={view.rooms.length === 0 || undefined}>
              {view.rooms.length > 0 && (
                <div className={styles.chatList}>
                  <ChatList view={view} heads={heads} chosen={room?.id ?? null} onOpen={openRoom} />
                </div>
              )}
              <div ref={pane} className={styles.pane}>
                {grouping ? (
                  <NewGroup
                    view={view}
                    heads={heads}
                    onDone={(created) => {
                      afterGroup.current = created === null ? 'plus' : 'chat';
                      onPlace((current) => ({
                        tab: 'chats',
                        room: created ?? current.room,
                        grouping: false,
                      }));
                    }}
                  />
                ) : room !== undefined ? (
                  <ChatView
                    key={room.id}
                    room={room}
                    view={view}
                    game={game}
                    heads={heads}
                    onLeft={() => {
                      // Only if it is still the chat on screen: the player may have moved on.
                      const left = room.id;
                      onPlace((current) =>
                        current.room === left ? { ...current, room: null } : current,
                      );
                    }}
                  />
                ) : (
                  <p className={styles.paneEmpty}>
                    {view.rooms.length > 0 ? t.chooseChat : t.noChats}
                  </p>
                )}
              </div>
            </div>
          )}
          {tab === 'friends' && (
            <FriendsTab
              view={view}
              settings={settings}
              game={game}
              heads={heads}
              onOpen={openRoom}
            />
          )}
          {tab === 'requests' && <RequestsTab view={view} heads={heads} />}
        </div>
      </>
    );
  }

  return (
    <section
      ref={sheet}
      className={styles.sheet}
      aria-labelledby={headingId}
      onFocus={(event) => {
        lastFocus.current = event.target instanceof HTMLElement ? event.target : null;
      }}
    >
      <div className={styles.head}>
        <h2 id={headingId} ref={heading} className={styles.title} tabIndex={-1}>
          {t.open}
        </h2>
        {ready && (
          <Button
            ref={plus}
            size="small"
            className={styles.square}
            aria-label={t.newGroup}
            title={fewFriends ? t.newGroupNeeds : t.newGroup}
            // Not yet for so few friends: a press says why.
            aria-disabled={fewFriends || undefined}
            data-unavailable={fewFriends || undefined}
            onClick={() => {
              if (fewFriends) setNeedsShown(true);
              else show({ tab: 'chats', grouping: true });
            }}
          >
            <Icon name="plus" size={18} />
          </Button>
        )}
        <button
          type="button"
          className={styles.close}
          aria-label={t.close}
          title={t.close}
          onClick={onClose}
        >
          <Icon name="x" size={18} />
        </button>
      </div>
      {needsShown && fewFriends && (
        <p className={styles.muted} role="status">
          {t.newGroupNeeds}
        </p>
      )}
      {view.sanction !== null && <Notice tone="warning">{sanctionText(view.sanction)}</Notice>}
      {view.signedIn && view.problem !== null ? (
        !unconfirmed && <Notice tone="warning">{problemText(view.problem)}</Notice>
      ) : view.signedIn && view.me === null ? (
        <Spinner label={t.connecting} />
      ) : (
        view.signedIn && !view.connected && <Notice tone="info">{t.offline}</Notice>
      )}
      {body}
    </section>
  );
}
