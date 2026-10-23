import { useCallback, useEffect, useRef, useState, type RefObject } from 'react';
import { MoreMenu } from '../../app/MoreMenu';
import { Button } from '../../components/Button';
import { Confirm } from '../../components/Confirm';
import { Field } from '../../components/Field';
import { Icon } from '../../components/Icon';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { Screen } from '../../components/Screen';
import { Tag } from '../../components/Tag';
import { useFocusBack } from '../../components/useFocusBack';
import forms from '../../components/forms.module.css';
import type {
  FriendPerson,
  LocalWorld,
  WorldsMember,
  WorldsNewInvite,
  WorldsRole,
  WorldsRoom,
  WorldsRoomPage,
  WorldsState,
} from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { numberLocale, strings } from '../../lib/strings';
import { PlayerHead, Presence } from '../friends/People';
import { useFriends } from '../friends/useFriends';
import { worldInviteBody } from '../friends/worldInvite';
import { isolate } from '../mods/helpers';
import { usePlayerHeads } from '../skins/usePlayerHeads';
import { RoomMessages, RoomPlayButtons, RoomStartButtons } from './RoomActions';
import { WorldBackups } from './WorldBackups';
import { WorldMods } from './WorldMods';
import { WorldsStatus } from './WorldsStatus';
import { useRoomActions, type RoomActionsApi } from './useRoomActions';
import { ROOMS_EVERY_MS, activeRoom, roomMods, roomOpen, worldsErrorText } from './useWorlds';
import styles from './Worlds.module.css';

const t = strings.worlds;

/** How long an invite is valid, in hours: an hour to a week. The core checks 1–168. */
const HOURS = [1, 6, 24, 72, 168] as const;
const USES = [1, 3, 10] as const;

/** Who may moderate whom: only those of a lower role, never oneself. */
const RANK: Record<WorldsRole, number> = { owner: 3, manager: 2, member: 1 };

type Asking = { kind: 'remove' | 'ban'; member: WorldsMember } | { kind: 'leave' };

interface Props {
  room: string;
  /** The room as the list has it: its name and facts before the page comes. */
  listed: WorldsRoom | null;
  /** Every room of the account, for the band's name of the room this computer is in. */
  rooms: WorldsRoom[];
  /** The signed-in launcher account. */
  me: string | null;
  local: LocalWorld[];
  state: WorldsState;
  /** What was pressed for the room this computer is in, kept by the view. */
  bandActions: RoomActionsApi;
  bandRef: RefObject<HTMLElement | null>;
  headingRef: RefObject<HTMLHeadingElement | null>;
  onBack: () => void;
  onLocal: (world: LocalWorld) => void;
  /** The account left the room. */
  onLeft: () => void;
}

/**
 * One room as its own screen (concept A): a way back, its name with its facts and what
 * this computer can do with it, what this computer does now, then its members and
 * mods beside the invite and the backups.
 */
export function RoomPage({
  room,
  listed,
  rooms,
  me,
  local,
  state,
  bandActions,
  bandRef,
  headingRef,
  onBack,
  onLocal,
  onLeft,
}: Props) {
  const [page, setPage] = useState<WorldsRoomPage | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [asking, setAsking] = useState<Asking | null>(null);
  const [answering, setAnswering] = useState(false);
  const [askError, setAskError] = useState<string | null>(null);
  // The button that asked: the focus goes back to it when the question closes.
  const opener = useRef<HTMLElement | null>(null);
  const play = useRef<HTMLButtonElement>(null);
  const start = useRef<HTMLButtonElement>(null);
  const memberList = useRef<HTMLUListElement>(null);
  // A member removed or banned takes the row's buttons along: the list has the focus.
  useFocusBack(asking === null ? null : asking.kind, opener, memberList, headingRef);

  /** The page again; it settles once shown, whether or not it came. */
  const load = useCallback(() => {
    return ipc
      .worldsRoom(room)
      .then((next) => {
        setPage(next);
        setError(null);
      })
      .catch((e: unknown) => {
        setError(worldsErrorText(e));
      });
  }, [room]);

  useEffect(() => {
    void load();
    const timer = window.setInterval(() => {
      void load();
    }, ROOMS_EVERY_MS);
    return () => {
      window.clearInterval(timer);
    };
  }, [load]);

  // Opening or closing the world here shows at once.
  const hostingHere = state.hosting?.room === room ? state.hosting.phase : null;
  useEffect(() => {
    if (hostingHere !== null) void load();
  }, [hostingHere, load]);

  const found = page?.room ?? listed;
  const world = found === null ? null : (local.find((w) => w.id === found.world) ?? null);
  const actions = useRoomActions({ roomId: room, room: found, local: world, onLocal });
  // Accept the EULA goes with its answer: Open for friends takes the focus, or the title
  // while another room is played here.
  useFocusBack(world?.eulaAccepted, start, headingRef);
  const here = activeRoom(state) === room;

  const ask = (next: Asking, from: HTMLElement | null) => {
    opener.current = from;
    setAskError(null);
    setAsking(next);
  };

  const answer = (action: () => Promise<unknown>, after: () => void) => {
    if (answering) return;
    setAnswering(true);
    setAskError(null);
    action()
      .then(() => {
        setAsking(null);
        after();
      })
      .catch((e: unknown) => {
        setAskError(worldsErrorText(e));
      })
      .finally(() => {
        setAnswering(false);
      });
  };

  const back = (
    <button type="button" className={styles.back} onClick={onBack}>
      <Icon name="back" size={18} className={styles.mirror} />
      {t.back}
    </button>
  );

  if (found === null) {
    return (
      <Screen title={t.title} headingRef={headingRef} top={back}>
        {error !== null ? (
          <Notice tone="danger">{error}</Notice>
        ) : (
          <p className={styles.muted}>{t.loading}</p>
        )}
      </Screen>
    );
  }

  const owner = found.role === 'owner';
  const manages = owner || found.role === 'manager';
  const now = roomOpen(found, state);
  const mods = roomMods(found, world);
  const backups = owner && world !== null;
  const aside = manages || backups;

  const menu =
    found.role === 'owner' ? null : (
      <MoreMenu
        label={t.roomActions}
        ariaLabel={t.roomActions}
        icon={<Icon name="more" />}
        caret={false}
        current={false}
        buttonClassName={styles.moreButton}
        entries={[
          {
            kind: 'item',
            key: 'leave',
            label: t.leave,
            onSelect: () => {
              // The menu gave the focus back to its button: the answer returns it there.
              ask(
                { kind: 'leave' },
                document.activeElement instanceof HTMLElement ? document.activeElement : null,
              );
            },
          },
        ]}
      />
    );

  return (
    <Screen
      title={found.name}
      isolateTitle
      headingRef={headingRef}
      top={back}
      aside={
        <>
          <Tag tone={now.open ? 'success' : 'outline'}>
            {now.open ? t.openNow({ online: now.online, max: found.maxPlayers }) : t.closedNow}
          </Tag>
          <Tag>{t.roles[found.role]}</Tag>
          {owner && <Tag>{world === null ? t.keptElsewhere : t.keptHere}</Tag>}
          {mods !== null && <Tag>{t.modsCount({ count: mods })}</Tag>}
        </>
      }
      actions={
        <>
          {here ? (
            <RoomPlayButtons actions={bandActions} roomId={room} state={state} playRef={play} />
          ) : (
            <RoomStartButtons
              actions={actions}
              room={found}
              local={world}
              state={state}
              startRef={start}
            />
          )}
          {menu}
        </>
      }
    >
      <div className={styles.roomHint}>
        {found.description !== '' && (
          <p className={styles.description} dir="auto">
            {found.description}
          </p>
        )}
        <p className={styles.panelHint}>
          {t.roomMeta({
            owner: isolate(found.ownerNick),
            version: found.minecraft,
            members: found.members,
          })}
        </p>
        {owner && world !== null && !world.eulaAccepted && (
          <p className={styles.panelHint}>{t.eulaNeeded}</p>
        )}
      </div>
      {asking?.kind === 'leave' && (
        <div className={styles.headWide}>
          <Confirm
            label={found.name}
            text={t.leaveConfirm({ room: isolate(found.name) })}
            action={t.leave}
            cancel={t.cancel}
            pending={answering}
            onYes={() => {
              answer(() => ipc.worldsLeaveRoom(room), onLeft);
            }}
            onNo={() => {
              setAsking(null);
            }}
          />
          {askError !== null && <Notice tone="danger">{askError}</Notice>}
        </div>
      )}
      {(actions.note !== null || actions.error !== null) && (
        <div className={styles.headWide}>
          <RoomMessages actions={actions} />
        </div>
      )}
      <WorldsStatus
        state={state}
        rooms={rooms}
        actions={bandActions}
        bandRef={bandRef}
        buttons={!here}
        playRef={here ? play : undefined}
      />
      {error !== null && <Notice tone="danger">{error}</Notice>}
      <div
        className={[styles.roomColumns, aside && styles.roomColumnsTwo].filter(Boolean).join(' ')}
      >
        <div className={styles.column}>
          <Members
            page={page}
            role={found.role}
            me={me}
            state={state}
            room={room}
            asking={asking}
            answering={answering}
            askError={askError}
            onAsk={ask}
            onNo={() => {
              setAsking(null);
            }}
            onAnswer={answer}
            onDone={load}
            listRef={memberList}
          />
          {owner && world?.loader != null && (
            <WorldMods room={found.id} world={world} state={state} />
          )}
        </div>
        {aside && (
          <div className={styles.column}>
            {manages && <Invite room={room} />}
            {backups && <WorldBackups world={world} state={state} />}
          </div>
        )}
      </div>
    </Screen>
  );
}

/** The room's members: who they are to the room and to this player, and moderation
 * where the service allows it. */
function Members({
  page,
  role,
  me,
  state,
  room,
  asking,
  answering,
  askError,
  onAsk,
  onNo,
  onAnswer,
  onDone,
  listRef,
}: {
  page: WorldsRoomPage | null;
  role: WorldsRole;
  me: string | null;
  state: WorldsState;
  room: string;
  asking: Asking | null;
  answering: boolean;
  askError: string | null;
  onAsk: (next: Asking, from: HTMLElement | null) => void;
  onNo: () => void;
  onAnswer: (action: () => Promise<unknown>, after: () => void) => void;
  /** The page again: the question closes once it shows what the answer did. */
  onDone: () => Promise<void>;
  listRef: RefObject<HTMLUListElement | null>;
}) {
  const friends = useFriends();
  const members = page?.members ?? [];
  const heads = usePlayerHeads(members.map((m) => m.nick));
  const present = members.filter((m) => m.state === 'active');
  const former = members.filter((m) => m.state !== 'active');
  // The players in the world this computer hosts, the host too.
  const playing =
    state.hosting?.room === room && state.hosting.ended === null ? state.hosting.players : [];
  const known = friends?.signedIn === true ? friends.friends : [];

  const row = (member: WorldsMember) => {
    const active = member.state === 'active';
    const here = active && playing.includes(member.nick);
    const friend: FriendPerson | undefined = known.find((f) => f.id === member.account);
    const moderates = active && member.account !== me && RANK[member.role] < RANK[role];
    const question =
      asking !== null && asking.kind !== 'leave' && asking.member.account === member.account;
    return (
      <li
        key={member.account}
        className={[styles.memberRow, !active && styles.memberGone].filter(Boolean).join(' ')}
      >
        <span className={styles.memberHead}>
          <PlayerHead nick={member.nick} heads={heads} size={32} />
        </span>
        <div className={styles.memberText}>
          <div className={styles.rowLine}>
            <span className={styles.memberName} dir="auto">
              {member.nick}
            </span>
            <Tag>{t.roles[member.role]}</Tag>
            {here && <Tag tone="success">{t.inWorld}</Tag>}
            {!active && <Tag>{member.state === 'banned' ? t.memberBanned : t.memberGone}</Tag>}
          </div>
          {member.account === me ? (
            <span className={styles.rowMeta}>{t.you}</span>
          ) : (
            // In this world already says where the friend is.
            friend !== undefined && !here && <Presence person={friend} />
          )}
        </div>
        {moderates && (
          <div className={styles.memberActions}>
            {(['remove', 'ban'] as const).map((kind) => (
              <Button
                key={kind}
                size="small"
                aria-disabled={answering || undefined}
                onClick={(event) => {
                  if (answering) return;
                  onAsk({ kind, member }, event.currentTarget);
                }}
              >
                {kind === 'remove' ? t.remove : t.ban}
              </Button>
            ))}
          </div>
        )}
        {question && (
          <div className={styles.rowWide}>
            <Confirm
              label={member.nick}
              text={(asking.kind === 'remove' ? t.removeConfirm : t.banConfirm)({
                nick: isolate(member.nick),
              })}
              action={asking.kind === 'remove' ? t.remove : t.ban}
              cancel={t.cancel}
              pending={answering}
              onYes={() => {
                onAnswer(
                  () => ipc.worldsModerate(room, member.account, asking.kind).then(onDone),
                  () => undefined,
                );
              }}
              onNo={onNo}
            />
            {askError !== null && <Notice tone="danger">{askError}</Notice>}
          </div>
        )}
      </li>
    );
  };

  return (
    <Panel title={t.members} level={3} count={page === null ? undefined : present.length}>
      {page === null ? (
        <p className={styles.muted}>{t.loading}</p>
      ) : (
        <ul ref={listRef} className={styles.memberList} aria-label={t.members} tabIndex={-1}>
          {present.map(row)}
          {former.map(row)}
        </ul>
      )}
    </Panel>
  );
}

/** A one-time code for a friend: how long and how many times, shown once, copied or
 * sent in a direct chat. */
function Invite({ room }: { room: string }) {
  const [hours, setHours] = useState<number>(24);
  const [uses, setUses] = useState<number>(1);
  const [invite, setInvite] = useState<WorldsNewInvite | null>(null);
  const [making, setMaking] = useState(false);
  const [sending, setSending] = useState(false);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const friends = useFriends();
  const [friend, setFriend] = useState('');
  const [sent, setSent] = useState<string | null>(null);
  const people = friends?.signedIn === true ? friends.friends : [];
  const chosen = people.some((p) => p.id === friend) ? friend : (people[0]?.id ?? '');

  const make = () => {
    // A code sent meanwhile would be said of the new one.
    if (making || sending) return;
    setMaking(true);
    setError(null);
    setCopied(false);
    setSent(null);
    ipc
      .worldsInvite(room, hours, uses)
      .then(setInvite)
      .catch((e: unknown) => {
        setError(worldsErrorText(e));
      })
      .finally(() => {
        setMaking(false);
      });
  };

  const send = (code: string) => {
    const person = people.find((p) => p.id === chosen);
    if (person === undefined || sending) return;
    setSending(true);
    setError(null);
    setSent(null);
    ipc
      .friendsOpenDirect(person.id)
      .then((chat) => ipc.friendsSend(chat, worldInviteBody(code)))
      .then(() => {
        setSent(person.nick);
      })
      .catch((e: unknown) => {
        setError(worldsErrorText(e));
      })
      .finally(() => {
        setSending(false);
      });
  };

  return (
    <Panel title={t.invite.title} level={3}>
      <div className={styles.inviteForm}>
        <Field label={t.invite.hours}>
          {(field) => (
            <select
              {...field}
              className={forms.select}
              value={hours}
              onChange={(e) => {
                const next = Number(e.target.value);
                if (HOURS.some((h) => h === next)) setHours(next);
              }}
            >
              {HOURS.map((h) => (
                <option key={h} value={h}>
                  {h < 24
                    ? t.invite.hourOptions({ count: h })
                    : t.invite.dayOptions({ count: h / 24 })}
                </option>
              ))}
            </select>
          )}
        </Field>
        <Field label={t.invite.uses}>
          {(field) => (
            <select
              {...field}
              className={forms.select}
              value={uses}
              onChange={(e) => {
                const next = Number(e.target.value);
                if (USES.some((u) => u === next)) setUses(next);
              }}
            >
              {USES.map((u) => (
                <option key={u} value={u}>
                  {u.toLocaleString(numberLocale())}
                </option>
              ))}
            </select>
          )}
        </Field>
        <Button aria-disabled={making || sending || undefined} onClick={make}>
          <Icon name="key" size={18} />
          {t.invite.make}
        </Button>
      </div>
      {error !== null && <Notice tone="danger">{error}</Notice>}
      {invite !== null && (
        <div className={styles.codeBox}>
          <p className={styles.panelHint}>{t.invite.code}</p>
          <div className={styles.codeRow}>
            {/* A one-time secret: shown as text, never kept or written anywhere. */}
            <bdi dir="ltr" className={styles.code}>
              {invite.code}
            </bdi>
            <Button
              size="small"
              onClick={() => {
                void navigator.clipboard
                  .writeText(invite.code)
                  .then(() => {
                    setCopied(true);
                  })
                  .catch(() => undefined);
              }}
            >
              <Icon name={copied ? 'check' : 'copy'} size={16} />
              {copied ? t.invite.copied : t.invite.copy}
            </Button>
          </div>
          <p className={styles.panelHint}>
            {t.invite.hint({
              uses: invite.invite.maxUses,
              date: new Date(invite.invite.expires * 1000).toLocaleString(numberLocale()),
            })}
          </p>
          {people.length > 0 && (
            <div className={styles.inviteForm}>
              <Field label={t.invite.sendTo}>
                {(field) => (
                  <select
                    {...field}
                    className={forms.select}
                    value={chosen}
                    onChange={(e) => {
                      setFriend(e.target.value);
                      setSent(null);
                    }}
                  >
                    {people.map((person) => (
                      <option key={person.id} value={person.id}>
                        {person.nick}
                      </option>
                    ))}
                  </select>
                )}
              </Field>
              <Button
                variant="primary"
                size="small"
                aria-disabled={sending || undefined}
                onClick={() => {
                  send(invite.code);
                }}
              >
                <Icon name="send" size={16} className={styles.mirror} />
                {t.invite.send}
              </Button>
            </div>
          )}
          <p className={styles.live} role="status">
            {sent === null ? '' : t.invite.sent({ nick: isolate(sent) })}
          </p>
        </div>
      )}
    </Panel>
  );
}
