import { useCallback, useEffect, useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import { StatusBadge } from '../../components/StatusBadge';
import forms from '../../components/forms.module.css';
import type {
  LocalWorld,
  WorldsMember,
  WorldsNewInvite,
  WorldsRoomPage,
  WorldsState,
} from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { numberLocale, strings } from '../../lib/strings';
import { Dialog } from '../mods/Dialog';
import { RoomActions } from './RoomActions';
import { worldsErrorText } from './useWorlds';
import styles from './Worlds.module.css';

/** How long an invite is valid, in hours: an hour, a day, a week. */
const HOURS = [1, 24, 168] as const;
const USES = [1, 3, 10] as const;

interface Confirming {
  text: string;
  yes: string;
  run: () => void;
}

interface Props {
  room: string;
  local: LocalWorld[];
  state: WorldsState;
  onBack: () => void;
  onLocal: (world: LocalWorld) => void;
  /** The account left the room. */
  onLeft: () => void;
}

/** One room: what this computer can do with it, its members and invites. */
export function RoomPage({ room, local, state, onBack, onLocal, onLeft }: Props) {
  const t = strings.worlds;
  const [page, setPage] = useState<WorldsRoomPage | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [hours, setHours] = useState<number>(24);
  const [uses, setUses] = useState<number>(1);
  const [invite, setInvite] = useState<WorldsNewInvite | null>(null);
  const [copied, setCopied] = useState(false);
  const [confirming, setConfirming] = useState<Confirming | null>(null);

  const load = useCallback(() => {
    ipc
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
    load();
  }, [load]);

  const act = (action: () => Promise<unknown>, after: () => void = load) => {
    setError(null);
    action()
      .then(after)
      .catch((e: unknown) => {
        setError(worldsErrorText(e));
      });
  };

  const moderate = (member: WorldsMember, action: 'remove' | 'ban') => {
    const ask = action === 'remove' ? t.removeConfirm : t.banConfirm;
    setConfirming({
      text: ask({ nick: member.nick }),
      yes: action === 'remove' ? t.remove : t.ban,
      run: () => {
        act(() => ipc.worldsModerate(room, member.account, action));
      },
    });
  };

  const found = page?.room ?? null;
  const manages = found !== null && (found.role === 'owner' || found.role === 'manager');
  const world = found === null ? null : (local.find((w) => w.id === found.world) ?? null);

  return (
    <div>
      <div className={styles.toolbar}>
        <Button onClick={onBack}>{t.back}</Button>
      </div>
      {error !== null && <Notice tone="danger">{error}</Notice>}
      {found === null ? (
        error === null && <p className={styles.muted}>{t.loading}</p>
      ) : (
        <>
          <h3 className={styles.roomName}>{found.name}</h3>
          {found.description !== '' && <p>{found.description}</p>}
          <p className={styles.muted}>
            {t.roomMeta({
              owner: found.ownerNick,
              version: found.minecraft,
              members: found.members,
            })}
          </p>
          <div className={styles.badges}>
            <StatusBadge
              tone={found.hosting ? 'success' : 'neutral'}
              label={
                found.hosting
                  ? t.openNow({ online: found.online, max: found.maxPlayers })
                  : t.closedNow
              }
            />
            <StatusBadge tone="neutral" label={t.roles[found.role]} />
          </div>
          <RoomActions room={found} local={world} state={state} onLocal={onLocal} />

          <section className={styles.section} aria-label={t.members}>
            <h4 className={styles.sectionTitle}>{t.members}</h4>
            <ul className={styles.members}>
              {page?.members.map((member) => (
                <li key={member.account} className={styles.member}>
                  <span>
                    <span className={styles.nick}>{member.nick}</span>{' '}
                    <span className={styles.muted}>{t.roles[member.role]}</span>
                  </span>
                  {manages && member.role !== 'owner' && member.account !== found.owner && (
                    <span className={styles.actions}>
                      <Button
                        onClick={() => {
                          moderate(member, 'remove');
                        }}
                      >
                        {t.remove}
                      </Button>
                      <Button
                        onClick={() => {
                          moderate(member, 'ban');
                        }}
                      >
                        {t.ban}
                      </Button>
                    </span>
                  )}
                </li>
              ))}
            </ul>
          </section>

          {manages && (
            <section className={styles.section} aria-label={t.invite.title}>
              <h4 className={styles.sectionTitle}>{t.invite.title}</h4>
              <div className={forms.row}>
                <label>
                  {t.invite.hours}{' '}
                  <select
                    className={forms.select}
                    value={hours}
                    onChange={(e) => {
                      setHours(Number(e.target.value));
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
                </label>
                <label>
                  {t.invite.uses}{' '}
                  <select
                    className={forms.select}
                    value={uses}
                    onChange={(e) => {
                      setUses(Number(e.target.value));
                    }}
                  >
                    {USES.map((u) => (
                      <option key={u} value={u}>
                        {u}
                      </option>
                    ))}
                  </select>
                </label>
                <Button
                  variant="primary"
                  onClick={() => {
                    setCopied(false);
                    act(
                      () => ipc.worldsInvite(room, hours, uses).then(setInvite),
                      () => undefined,
                    );
                  }}
                >
                  {t.invite.make}
                </Button>
              </div>
              {invite !== null && (
                <div className={styles.status}>
                  <p className={styles.muted}>{t.invite.code}</p>
                  <p className={styles.code}>{invite.code}</p>
                  <p className={styles.muted}>
                    {t.invite.hint({
                      uses: invite.invite.maxUses,
                      date: new Date(invite.invite.expires * 1000).toLocaleString(numberLocale()),
                    })}
                  </p>
                  <div className={styles.actions}>
                    <Button
                      onClick={() => {
                        void navigator.clipboard
                          .writeText(invite.code)
                          .then(() => {
                            setCopied(true);
                          })
                          .catch(() => undefined);
                      }}
                    >
                      {copied ? t.invite.copied : t.invite.copy}
                    </Button>
                  </div>
                </div>
              )}
            </section>
          )}

          {found.role !== 'owner' && (
            <div className={styles.section}>
              <div className={styles.actions}>
                <Button
                  onClick={() => {
                    setConfirming({
                      text: t.leaveConfirm({ room: found.name }),
                      yes: t.leave,
                      run: () => {
                        act(() => ipc.worldsLeaveRoom(room), onLeft);
                      },
                    });
                  }}
                >
                  {t.leave}
                </Button>
              </div>
            </div>
          )}
        </>
      )}
      {confirming !== null && (
        <Dialog
          title={found?.name ?? t.title}
          onClose={() => {
            setConfirming(null);
          }}
        >
          <p>{confirming.text}</p>
          <div className={forms.actions}>
            <Button
              onClick={() => {
                setConfirming(null);
              }}
            >
              {t.cancel}
            </Button>
            <Button
              variant="primary"
              onClick={() => {
                confirming.run();
                setConfirming(null);
              }}
            >
              {confirming.yes}
            </Button>
          </div>
        </Dialog>
      )}
    </div>
  );
}
