import { useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Icon } from '../../components/Icon';
import { Notice } from '../../components/Notice';
import { Screen } from '../../components/Screen';
import { Segmented } from '../../components/Segmented';
import type {
  Closet,
  PickedTexture,
  SkinKind,
  SkinsProblem,
  WardrobeChange,
  WardrobeItem,
  WardrobeSet,
} from '../../lib/contracts';
import { ipc, skinsProblem } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { ByNick } from './ByNick';
import { type Draft, durationsToSend, newDraft, pickedOf, trialTexture } from './draft';
import { NewSkin } from './NewSkin';
import { ownSkinChanged } from './ownSkin';
import { type Back, type Outfit, SkinStage } from './SkinStage';
import styles from './Skins.module.css';
import { type Texture, cutFrames, loadImage, looksSlim } from './textures';
import { useTextures } from './useTextures';
import { Picture, UploadCard, WardrobeCard } from './WardrobeCard';
import {
  type Trial,
  type WardrobeSort,
  type WardrobeTab,
  WARDROBE_SORTS,
  WARDROBE_TABS,
  chipsOf,
  closetOf,
  displayName,
  isTried,
  room,
  sameTexture,
  sortedEntries,
  textureOf,
  toggleItem,
  toggleSet,
  trialChanges,
  trialPieces,
  uploadsLine,
  wardrobeName,
} from './closet';

/** The largest file the core imports (a GIF or APNG before it becomes a strip). */
const DROP_MAX_BYTES = 8 * 1024 * 1024;
/** Files that can be dropped: the core checks what they really are. */
const DROPPABLE = /\.(png|gif|apng)$/i;

/** A file's contents as base64, read by the page (the window hands dropped files to
 * the page, never their paths). */
function base64Of(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const url = typeof reader.result === 'string' ? reader.result : '';
      resolve(url.slice(url.indexOf(',') + 1));
    };
    reader.onerror = () => {
      reject(new Error('The file could not be read.'));
    };
    reader.readAsDataURL(file);
  });
}

/** The hashes whose pictures the screen shows: the character's first, then the tab's
 * cards in their order, then the rest. */
function picturesWanted(closet: Closet, tab: WardrobeTab, sort: WardrobeSort): string[] {
  const hashes: string[] = [];
  const add = (texture: { hash: string } | null) => {
    if (texture !== null) hashes.push(texture.hash);
  };
  add(closet.worn.skin);
  add(closet.worn.cape);
  const items = closet.items.filter((item) => item.kind === tab);
  for (const item of sortedEntries(items, sort)) add(item.texture);
  if (tab === 'set') {
    for (const set of sortedEntries(closet.sets, sort)) {
      add(set.skin);
      add(set.cape);
    }
  }
  for (const item of closet.items) add(item.texture);
  for (const set of closet.sets) {
    add(set.skin);
    add(set.cape);
  }
  return hashes;
}

interface Props {
  /** The launcher account's nick, over the character's head. */
  nick: string;
}

/**
 * The Skins tab for a signed-in launcher account (board A-Wardrobe, D-065): the
 * character on the left; on the right the wardrobe the account keeps on every computer,
 * skins, capes and sets to try on and wear (never an upload), and new ones from a file
 * or another player's nick, looked at on their own screen (board A-SkinNew).
 */
export function Wardrobe({ nick }: Props) {
  const s = strings.accounts.skins;
  const w = s.wardrobe;
  const [closet, setCloset] = useState<Closet | null>(null);
  const [loading, setLoading] = useState(true);
  const [attempt, setAttempt] = useState(0);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<SkinsProblem | null>(null);
  const [done, setDone] = useState<string | null>(null);
  const [tab, setTab] = useState<WardrobeTab>('skin');
  const [sort, setSort] = useState<WardrobeSort>('worn');
  const [trial, setTrial] = useState<Trial | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [back, setBack] = useState<Back>('cape');
  /** The by-nick panel: what it takes first, and a count that opens it anew. */
  const [byNick, setByNick] = useState<{ kind: SkinKind; opened: number } | null>(null);
  const [toHeading, setToHeading] = useState(0);
  const heading = useRef<HTMLHeadingElement>(null);
  const tabs = useRef<HTMLDivElement>(null);
  const byNickButton = useRef<HTMLButtonElement>(null);
  /** What had the focus when an action started, to give it back if the action moved
   * or removed it. */
  const focusBack = useRef<HTMLElement | null>(null);
  /** Counts the by-nick panel's openings and closings: an answer to a panel since closed
   * or opened again is not opened. */
  const byNickAsk = useRef(0);
  const tabsId = useId();
  const sortId = useId();

  const urls = useTextures(closet === null ? [] : picturesWanted(closet, tab, sort));

  useEffect(() => {
    let live = true;
    ipc
      .skinsWardrobe()
      .then((answer: unknown) => {
        if (!live) return;
        const read = closetOf(answer);
        if (read === null) setProblem('server');
        else setCloset(read);
      })
      .catch((e: unknown) => {
        if (live) setProblem(skinsProblem(e));
      })
      .finally(() => {
        if (live) setLoading(false);
      });
    return () => {
      live = false;
    };
  }, [attempt]);

  // A new view (the new skin's screen, or back to the wardrobe) starts at its title.
  useEffect(() => {
    if (toHeading > 0) heading.current?.focus();
  }, [toHeading]);

  // An action that moved the focused card keeps its focus; one that removed it (or
  // closed the fitting) sends it to the tab shown.
  useLayoutEffect(() => {
    const was = focusBack.current;
    if (was === null || busy) return;
    focusBack.current = null;
    const active = document.activeElement;
    if (active !== null && active !== document.body) return;
    if (was.isConnected) was.focus();
    else tabs.current?.querySelector<HTMLElement>('[aria-selected="true"]')?.focus();
  }, [closet, trial, busy]);

  // A picked skin's arms: what the file looks like it has (one taken by nick comes
  // with its owner's).
  const guessSrc =
    draft !== null && draft.picked.kind === 'skin' && draft.picked.slim === null
      ? draft.picked.preview
      : null;
  useEffect(() => {
    if (guessSrc === null) return;
    let live = true;
    loadImage(guessSrc)
      .then((image) => {
        if (!live) return;
        setDraft((old) => {
          if (old?.picked.preview !== guessSrc) return old;
          const [first] = cutFrames(image, trialTexture(old));
          if (first === undefined) return old;
          const slim = looksSlim(first);
          return { ...old, slim, slimGuess: slim };
        });
      })
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [guessSrc]);

  const remember = () => {
    const active = document.activeElement;
    focusBack.current = active instanceof HTMLElement && active !== document.body ? active : null;
  };

  const start = () => {
    setBusy(true);
    setDone(null);
    setProblem(null);
  };

  /** Opens a picked or taken file on its own screen. */
  const begin = (answer: PickedTexture | null) => {
    if (answer === null) return;
    const picked = pickedOf(answer);
    if (picked === null) {
      setProblem('server');
      return;
    }
    const slim = picked.kind === 'skin' && (picked.slim ?? closet?.worn.skin?.slim ?? false);
    setDraft(newDraft(picked, slim));
    setByNick(null);
    setTrial(null);
    setToHeading((n) => n + 1);
  };

  const pick = async (kind: SkinKind) => {
    start();
    try {
      begin(
        await ipc.skinsPick(
          kind,
          kind === 'skin' ? strings.windowTitles.pickSkin : strings.windowTitles.pickCape,
        ),
      );
    } catch (e) {
      setProblem(skinsProblem(e));
    } finally {
      setBusy(false);
    }
  };

  const take = async (kind: SkinKind, file: File) => {
    setDone(null);
    setProblem(null);
    if (!DROPPABLE.test(file.name)) {
      setProblem('bad_png');
      return;
    }
    if (file.size > DROP_MAX_BYTES) {
      setProblem('too_large');
      return;
    }
    start();
    try {
      begin(await ipc.skinsTake(kind, file.name, await base64Of(file)));
    } catch (e) {
      setProblem(e instanceof Error ? 'unreadable' : skinsProblem(e));
    } finally {
      setBusy(false);
    }
  };

  const openByNick = (kind: SkinKind) => {
    byNickAsk.current += 1;
    setByNick((old) => ({ kind, opened: (old?.opened ?? 0) + 1 }));
  };

  const closeByNick = () => {
    byNickAsk.current += 1;
    setByNick(null);
  };

  /** Asks what a nick wears and opens it; the screen waits meanwhile, so nothing else is
   * picked while the core keeps the answer as this kind's file. */
  const takeByNick = async (kind: SkinKind, nick: string): Promise<SkinsProblem | null> => {
    if (busy) return null;
    const ask = byNickAsk.current;
    start();
    try {
      const answer: unknown = await ipc.skinsByNick(kind, nick);
      if (ask !== byNickAsk.current) return null;
      const picked = pickedOf(answer);
      if (picked === null) return 'server';
      begin(picked);
      return null;
    } catch (e) {
      return ask === byNickAsk.current ? skinsProblem(e) : null;
    } finally {
      setBusy(false);
    }
  };

  /** Sends a change; the wardrobe as it is then. */
  const change = async (next: WardrobeChange, message: string | null): Promise<boolean> => {
    if (busy) return false;
    remember();
    start();
    try {
      const read = closetOf(await ipc.skinsWardrobeChange(next));
      if (read === null) {
        setProblem('server');
        return false;
      }
      setCloset(read);
      if (next.action === 'wear') ownSkinChanged();
      setDone(message);
      return true;
    } catch (e) {
      setProblem(skinsProblem(e));
      return false;
    } finally {
      setBusy(false);
    }
  };

  const takeOff = async (kind: SkinKind) => {
    if (busy || closet === null) return;
    remember();
    start();
    try {
      const read = closetOf({ ...closet, worn: await ipc.skinsRemove(kind) });
      if (read === null) {
        setProblem('server');
        return;
      }
      setCloset(read);
      ownSkinChanged();
      setDone(kind === 'skin' ? s.skinRemoved : s.capeRemoved);
    } catch (e) {
      setProblem(skinsProblem(e));
    } finally {
      setBusy(false);
    }
  };

  /** Puts on what is tried on: from the wardrobe, so never an upload. */
  const wearTrial = async () => {
    if (busy || closet === null) return;
    const changes = trialChanges(closet, trial);
    remember();
    start();
    let latest = closet;
    let failed = false;
    try {
      for (const next of changes) {
        const read = closetOf(await ipc.skinsWardrobeChange(next));
        if (read === null) {
          setProblem('server');
          failed = true;
          break;
        }
        latest = read;
      }
    } catch (e) {
      setProblem(skinsProblem(e));
      failed = true;
    } finally {
      setBusy(false);
    }
    if (latest !== closet) {
      setCloset(latest);
      ownSkinChanged();
    }
    if (!failed) {
      setTrial(null);
      setDone(w.worn);
    }
  };

  /** Keeps the new file in the wardrobe, and wears it if asked (an upload either way). */
  const keepDraft = async (wear: boolean) => {
    if (busy || closet === null || draft === null) return;
    const frameMs = durationsToSend(draft);
    const name = wardrobeName(draft.name);
    if (frameMs === undefined || name === null) return;
    const { kind } = draft.picked;
    const slim = kind === 'skin' && draft.slim;
    const space = room(closet, kind);
    start();
    try {
      let next: Closet | null;
      let message: string = w.kept;
      if (!wear) {
        next = closetOf(await ipc.skinsKeep(kind, slim, frameMs, name));
      } else {
        // Worn first: the upload is what counts; keeping what is worn is not one.
        next = closetOf({ ...closet, worn: await ipc.skinsUpload(kind, slim, frameMs) });
        ownSkinChanged();
        message = w.worn;
        if (next !== null && space.count < space.limit) {
          try {
            const kept = closetOf(
              await ipc.skinsWardrobeChange({ action: 'keepWorn', kind, name }),
            );
            if (kept !== null) next = kept;
            else message = s.newSkin.wornNotSaved;
          } catch (e) {
            message = s.newSkin.wornNotSaved;
            setProblem(skinsProblem(e));
          }
        }
      }
      if (next === null) {
        setProblem('server');
        return;
      }
      setCloset(next);
      setDraft(null);
      setTab(kind);
      setDone(message);
      setToHeading((n) => n + 1);
    } catch (e) {
      setProblem(skinsProblem(e));
    } finally {
      setBusy(false);
    }
  };

  // One status region for every view, so a message that comes with a new view (a skin
  // kept, back in the wardrobe) is said, not mounted with it.
  const status = (
    <p role="status" className={styles.visuallyHidden}>
      {done}
    </p>
  );

  if (closet === null) {
    return (
      <>
        <Screen title={strings.nav.skins} headingRef={heading}>
          {loading ? (
            <p className={styles.hint}>{s.loading}</p>
          ) : (
            <div className={styles.signIn}>
              <Notice tone="warning">{s.problems[problem ?? 'server']}</Notice>
              <Button
                onClick={() => {
                  setLoading(true);
                  setProblem(null);
                  setAttempt((n) => n + 1);
                }}
              >
                {w.retry}
              </Button>
            </div>
          )}
        </Screen>
        {status}
      </>
    );
  }

  const textureFor = (kind: SkinKind, piece: Closet['worn']['skin']) =>
    textureOf(kind, piece, piece === null ? undefined : urls.get(piece.hash));
  const worn: Outfit = {
    skin: textureFor('skin', closet.worn.skin),
    cape: textureFor('cape', closet.worn.cape),
  };

  if (draft !== null) {
    const space = room(closet, draft.picked.kind);
    return (
      <>
        <NewSkin
          draft={draft}
          worn={worn}
          nick={nick}
          uploads={closet.worn.uploads}
          full={space.count >= space.limit}
          busy={busy}
          problem={problem}
          headingRef={heading}
          onChange={setDraft}
          onCancel={() => {
            setDraft(null);
            setProblem(null);
            setToHeading((n) => n + 1);
          }}
          onKeep={() => void keepDraft(false)}
          onKeepAndWear={() => void keepDraft(true)}
        />
        {status}
      </>
    );
  }

  const pieces = trialPieces(closet, trial);
  const shown = (kind: SkinKind): Texture | null => {
    const piece = pieces?.[kind];
    if (piece === undefined) return worn[kind];
    // A picture still on its way: what is worn stays until it comes.
    return piece === null ? null : (textureFor(kind, piece) ?? worn[kind]);
  };
  const trialOutfit: Outfit | null =
    pieces === null ? null : { skin: shown('skin'), cape: shown('cape') };
  const capeShown = (trialOutfit ?? worn).cape !== null;
  const nothingWorn = closet.worn.skin === null && closet.worn.cape === null;
  const sets = room(closet, 'set');
  const space = room(closet, tab);
  const wornLoose =
    tab !== 'set' &&
    closet.worn[tab] !== null &&
    !closet.items.some((item) => item.kind === tab && item.texture.hash === closet.worn[tab]?.hash);

  const trialTitle =
    pieces === null
      ? null
      : pieces.names.set !== null
        ? w.trying.set({ name: pieces.names.set })
        : pieces.names.skin !== null && pieces.names.cape !== null
          ? w.trying.both({ skin: pieces.names.skin, cape: pieces.names.cape })
          : pieces.names.skin !== null
            ? w.trying.skin({ skin: pieces.names.skin })
            : w.trying.cape({ cape: pieces.names.cape ?? '' });

  const itemCard = (item: WardrobeItem) => {
    const name = displayName(item);
    const wornNow = item.texture.hash === closet.worn[item.kind]?.hash;
    return (
      <WardrobeCard
        key={item.id}
        name={name}
        kept={item.name}
        picture={
          <Picture kind={item.kind} texture={item.texture} src={urls.get(item.texture.hash)} />
        }
        chips={chipsOf(item.kind, item.texture)}
        favourite={item.favourite}
        worn={wornNow}
        trying={isTried(trial, item) && pieces?.[item.kind] !== undefined}
        busy={busy}
        entries={[
          wornNow
            ? {
                kind: 'item',
                key: 'off',
                label: w.takeOff,
                onSelect: () => void takeOff(item.kind),
              }
            : {
                kind: 'item',
                key: 'wear',
                label: w.wear,
                onSelect: () => void change({ action: 'wear', of: 'item', id: item.id }, w.worn),
              },
        ]}
        onTry={() => {
          setTrial((old) => toggleItem(old, item));
        }}
        onFavourite={() =>
          void change(
            { action: 'favourite', of: 'item', id: item.id, favourite: !item.favourite },
            null,
          )
        }
        onRename={(next) =>
          change({ action: 'rename', of: 'item', id: item.id, name: next }, w.renamed)
        }
        onRemove={() => void change({ action: 'delete', of: 'item', id: item.id }, w.removed)}
      />
    );
  };

  const setCard = (set: WardrobeSet) => {
    const name = displayName(set);
    return (
      <WardrobeCard
        key={set.id}
        name={name}
        kept={set.name}
        picture={
          <span className={styles.setPicture}>
            <Picture
              kind="skin"
              texture={set.skin}
              src={set.skin === null ? undefined : urls.get(set.skin.hash)}
            />
            <Picture
              kind="cape"
              texture={set.cape}
              src={set.cape === null ? undefined : urls.get(set.cape.hash)}
            />
          </span>
        }
        chips={[
          set.skin !== null && set.cape !== null
            ? w.setOf.both
            : set.skin !== null
              ? w.setOf.skin
              : w.setOf.cape,
        ]}
        favourite={set.favourite}
        worn={sameTexture(set.skin, closet.worn.skin) && sameTexture(set.cape, closet.worn.cape)}
        trying={isTried(trial, set) && pieces !== null}
        busy={busy}
        entries={[
          {
            kind: 'item',
            key: 'wear',
            label: w.wear,
            onSelect: () => void change({ action: 'wear', of: 'set', id: set.id }, w.worn),
          },
        ]}
        onTry={() => {
          setTrial((old) => toggleSet(old, set));
        }}
        onFavourite={() =>
          void change(
            { action: 'favourite', of: 'set', id: set.id, favourite: !set.favourite },
            null,
          )
        }
        onRename={(next) =>
          change({ action: 'rename', of: 'set', id: set.id, name: next }, w.renamed)
        }
        onRemove={() => void change({ action: 'delete', of: 'set', id: set.id }, w.removed)}
      />
    );
  };

  const cards =
    tab === 'set'
      ? sortedEntries(closet.sets, sort).map(setCard)
      : sortedEntries(
          closet.items.filter((item) => item.kind === tab),
          sort,
        ).map(itemCard);

  return (
    <>
      <Screen
        title={strings.nav.skins}
        headingRef={heading}
        actions={
          <>
            <span className={styles.accountChip} dir="auto">
              {w.account({ nick })}
            </span>
            <Button
              ref={byNickButton}
              aria-expanded={byNick !== null}
              onClick={() => {
                if (byNick === null) openByNick(tab === 'cape' ? 'cape' : 'skin');
                else closeByNick();
              }}
            >
              <Icon name="search" size={18} />
              {w.byNick}
            </Button>
          </>
        }
      >
        {byNick !== null && (
          <ByNick
            key={byNick.opened}
            kind={byNick.kind}
            busy={busy}
            onTake={takeByNick}
            onClose={() => {
              closeByNick();
              byNickButton.current?.focus();
            }}
          />
        )}
        <div className={styles.wardrobeColumns}>
          <SkinStage worn={worn} trial={trialOutfit} nick={nick} frame={null} back={back} />
          <div className={styles.closet}>
            <div className={styles.closetHead}>
              <div
                ref={tabs}
                role="tablist"
                aria-label={w.tabsLabel}
                className={styles.tabs}
                onKeyDown={(event) => {
                  const keys: Record<string, number> = { ArrowLeft: -1, ArrowRight: 1 };
                  let step = keys[event.key];
                  const at = WARDROBE_TABS.indexOf(tab);
                  let next: WardrobeTab | undefined;
                  if (event.key === 'Home') next = WARDROBE_TABS[0];
                  else if (event.key === 'End') next = WARDROBE_TABS[WARDROBE_TABS.length - 1];
                  else if (step !== undefined) {
                    if (getComputedStyle(event.currentTarget).direction === 'rtl') step = -step;
                    next = WARDROBE_TABS[(at + step + WARDROBE_TABS.length) % WARDROBE_TABS.length];
                  }
                  if (next === undefined) return;
                  event.preventDefault();
                  setTab(next);
                  event.currentTarget.querySelector<HTMLElement>(`[data-tab="${next}"]`)?.focus();
                }}
              >
                {WARDROBE_TABS.map((item) => (
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
                      setTab(item);
                    }}
                  >
                    {w.tabs[item]}{' '}
                    <span className={styles.tabCount}>{room(closet, item).count}</span>
                  </button>
                ))}
              </div>
            </div>
            <div className={styles.sort} role="group" aria-labelledby={sortId}>
              <span id={sortId} className={styles.hint}>
                {w.sort}
              </span>
              <div className={styles.chips}>
                {WARDROBE_SORTS.map((item) => (
                  <button
                    key={item}
                    type="button"
                    className={styles.chip}
                    aria-pressed={sort === item}
                    onClick={() => {
                      setSort(item);
                    }}
                  >
                    {w.sorts[item]}
                  </button>
                ))}
              </div>
            </div>
            <div className={styles.closetTools}>
              {capeShown && (
                <div className={styles.backChoice}>
                  <span className={styles.hint}>{s.backLabel}</span>
                  <Segmented
                    label={s.backLabel}
                    options={[
                      { value: 'cape', label: s.backCape },
                      { value: 'elytra', label: s.backElytra },
                    ]}
                    value={back}
                    onChange={setBack}
                  />
                </div>
              )}
              <Button
                className={styles.saveSet}
                disabled={nothingWorn || sets.count >= sets.limit}
                aria-disabled={busy || undefined}
                onClick={() => {
                  void change({ action: 'keepSet', name: '' }, w.setSaved).then((ok) => {
                    if (!ok) return;
                    // The sets are shown, and this button may close when they are full.
                    setTab('set');
                    tabs.current?.querySelector<HTMLElement>('[data-tab="set"]')?.focus();
                  });
                }}
              >
                <Icon name="bookmark" size={18} />
                {w.saveSet}
              </Button>
            </div>
            <div
              role="tabpanel"
              id={`${tabsId}-panel`}
              aria-labelledby={`${tabsId}-${tab}`}
              className={styles.closetPanel}
            >
              <ul className={styles.grid}>
                {tab !== 'set' && (
                  <UploadCard
                    kind={tab}
                    busy={busy}
                    onPick={() => void pick(tab)}
                    onDrop={(file) => void take(tab, file)}
                    onByNick={() => {
                      openByNick(tab);
                    }}
                  />
                )}
                {wornLoose && (
                  <li className={styles.card}>
                    <span className={styles.cardTry}>
                      <span className={styles.cardPicture} aria-hidden="true">
                        <Picture
                          kind={tab}
                          texture={closet.worn[tab]}
                          src={urls.get(closet.worn[tab]?.hash ?? '')}
                        />
                      </span>
                      <span className={styles.cardName}>{w.wornNotKept}</span>
                    </span>
                    <span className={styles.wornTag}>{w.wornTag}</span>
                    <div className={styles.cardFoot}>
                      <Button
                        size="small"
                        disabled={space.count >= space.limit}
                        aria-disabled={busy || undefined}
                        onClick={() =>
                          void change({ action: 'keepWorn', kind: tab, name: '' }, w.kept)
                        }
                      >
                        {w.keepWorn}
                      </Button>
                      <Button
                        size="small"
                        aria-disabled={busy || undefined}
                        onClick={() => void takeOff(tab)}
                      >
                        {w.takeOff}
                      </Button>
                    </div>
                  </li>
                )}
                {cards}
              </ul>
              {cards.length === 0 && <p className={styles.hint}>{w.empty[tab]}</p>}
              {space.count >= space.limit && <p className={styles.hint}>{w.full[tab]}</p>}
            </div>
            <p className={styles.hint}>
              {uploadsLine(closet.worn.uploads)} {w.wearFree}{' '}
              {w.limits({
                skins: closet.limits.skins,
                capes: closet.limits.capes,
                sets: closet.limits.sets,
              })}
            </p>
            {pieces !== null && trialTitle !== null && (
              <section className={styles.tryBar} aria-label={w.trying.label}>
                <div className={styles.tryText}>
                  <p className={styles.tryTitle} dir="auto">
                    {trialTitle}
                  </p>
                  <p className={styles.hint}>{w.trying.text}</p>
                  {pieces.skin === null && closet.worn.skin !== null && (
                    <p className={styles.hint}>{w.trying.noSkin}</p>
                  )}
                  {pieces.cape === null && closet.worn.cape !== null && (
                    <p className={styles.hint}>{w.trying.noCape}</p>
                  )}
                </div>
                <div className={styles.tryActions}>
                  <Button
                    onClick={() => {
                      remember();
                      setTrial(null);
                    }}
                  >
                    {w.trying.cancel}
                  </Button>
                  <Button
                    variant="primary"
                    aria-disabled={busy || undefined}
                    onClick={() => void wearTrial()}
                  >
                    <Icon name="check" size={18} />
                    {w.wear}
                  </Button>
                </div>
              </section>
            )}
            {done !== null && (
              // Said by the status region, which stays from one view to the next.
              <p className={styles.done} aria-hidden="true">
                {done}
              </p>
            )}
            {problem !== null && <Notice tone="warning">{s.problems[problem]}</Notice>}
          </div>
        </div>
      </Screen>
      {status}
    </>
  );
}
