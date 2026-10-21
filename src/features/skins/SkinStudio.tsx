import { type DragEvent, useEffect, useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type {
  PickedTexture,
  SkinKind,
  SkinsProblem,
  Wardrobe,
  WornTexture,
} from '../../lib/contracts';
import { ipc, skinsProblem } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { type Draft, durationsToSend, newDraft, trialTexture } from './draft';
import { factsOf } from './facts';
import { FrameControls, NewTexture } from './NewTexture';
import { SkinPreview } from './SkinPreview';
import { type Outfit, SkinStage } from './SkinStage';
import styles from './Skins.module.css';
import {
  type Texture,
  cutFrames,
  isPngDataUrl,
  isWardrobe,
  loadImage,
  looksSlim,
} from './textures';

const NOTHING: Wardrobe = { skin: null, cape: null };
/** The largest file the core imports (a GIF or APNG before it becomes a strip). */
const DROP_MAX_BYTES = 8 * 1024 * 1024;
/** Files that can be dropped: the core checks what they really are. */
const DROPPABLE = /\.(png|gif|apng)$/i;

/** The worn texture's PNG as a `data:` URL, fetched by the core; `''` when it could
 * not be fetched, null while it is fetched. */
function useTexture(hash: string | null): string | null {
  const [texture, setTexture] = useState<{ hash: string; url: string } | null>(null);
  useEffect(() => {
    if (hash === null) return;
    let live = true;
    ipc
      .skinsTexture(hash)
      .then((url) => {
        if (live) setTexture({ hash, url: isPngDataUrl(url) ? url : '' });
      })
      .catch(() => {
        if (live) setTexture({ hash, url: '' });
      });
    return () => {
      live = false;
    };
  }, [hash]);
  return texture !== null && texture.hash === hash ? texture.url : null;
}

function wornTexture(kind: SkinKind, worn: WornTexture | null, src: string | null): Texture | null {
  if (worn === null || src === null || src === '') return null;
  return {
    src,
    kind,
    slim: kind === 'skin' && worn.slim,
    width: worn.width,
    height: worn.height,
    frames: worn.frames,
    frameMs: worn.frameMs,
  };
}

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

interface PieceProps {
  kind: SkinKind;
  worn: WornTexture | null;
  texture: Texture | null;
  loading: boolean;
  busy: boolean;
  onPick: () => void;
  onDrop: (file: File) => void;
  onRemove: () => void;
}

/** What is worn of one kind, with ways to change it: a file from the dialog or
 * dropped here, or nothing. */
function Piece({ kind, worn, texture, loading, busy, onPick, onDrop, onRemove }: PieceProps) {
  const s = strings.accounts.skins;
  const skin = kind === 'skin';
  const [over, setOver] = useState(false);
  const accepts = (event: DragEvent) => !busy && event.dataTransfer.types.includes('Files');
  return (
    <div
      className={styles.piece}
      data-over={over}
      onDragOver={(event) => {
        if (!accepts(event)) return;
        event.preventDefault();
        event.dataTransfer.dropEffect = 'copy';
        setOver(true);
      }}
      onDragLeave={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setOver(false);
      }}
      onDrop={(event) => {
        event.preventDefault();
        setOver(false);
        const file = event.dataTransfer.files[0];
        if (!busy && file !== undefined) onDrop(file);
      }}
    >
      <div className={styles.pieceThumb}>
        {texture !== null ? (
          <SkinPreview {...texture} small label={skin ? s.previewSkin : s.previewCape} />
        ) : (
          <span className={styles.hint}>
            {loading ? s.loading : worn !== null ? s.notShown : skin ? s.noSkin : s.noCape}
          </span>
        )}
      </div>
      <div className={styles.pieceBody}>
        <h4 className={styles.subtitle}>{skin ? s.skin : s.cape}</h4>
        {texture !== null && <p className={styles.factsLine}>{factsOf(texture)}</p>}
        <div className={forms.actions}>
          <Button disabled={busy} onClick={onPick}>
            {skin ? s.uploadSkin : s.uploadCape}
          </Button>
          {worn !== null && (
            <Button disabled={busy} onClick={onRemove}>
              {skin ? s.removeSkin : s.removeCape}
            </Button>
          )}
        </div>
        <p className={styles.hint}>{s.dropHere}</p>
      </div>
    </div>
  );
}

interface Props {
  /** The launcher account's nick, over the character's head. */
  nick: string | null;
}

/** The launcher account's skin and cape at skins.lapi.sh (D-065): the character in
 * 3D wearing them, a file picked or dropped tried on before it is worn, removal. */
export function SkinStudio({ nick }: Props) {
  const s = strings.accounts.skins;
  const [wardrobe, setWardrobe] = useState<Wardrobe>(NOTHING);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<SkinsProblem | null>(null);
  const [done, setDone] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const skinUrl = useTexture(wardrobe.skin?.hash ?? null);
  const capeUrl = useTexture(wardrobe.cape?.hash ?? null);
  const worn: Outfit = {
    skin: wornTexture('skin', wardrobe.skin, skinUrl),
    cape: wornTexture('cape', wardrobe.cape, capeUrl),
  };

  useEffect(() => {
    let live = true;
    ipc
      .skinsMine()
      .then((mine: unknown) => {
        if (live) setWardrobe(isWardrobe(mine) ? mine : NOTHING);
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
  }, []);

  // A picked skin's arms: what the file looks like it has.
  const pickedSrc = draft?.picked.kind === 'skin' ? draft.picked.preview : null;
  useEffect(() => {
    if (pickedSrc === null) return;
    let live = true;
    loadImage(pickedSrc)
      .then((image) => {
        if (!live) return;
        setDraft((old) => {
          if (old?.picked.preview !== pickedSrc) return old;
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
  }, [pickedSrc]);

  const begin = (picked: PickedTexture | null) => {
    if (picked === null || typeof picked !== 'object') return;
    setDraft(newDraft(picked, picked.kind === 'skin' && (wardrobe.skin?.slim ?? false)));
  };

  const pick = async (kind: SkinKind) => {
    setBusy(true);
    setDone(null);
    setProblem(null);
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
    setBusy(true);
    try {
      begin(await ipc.skinsTake(kind, file.name, await base64Of(file)));
    } catch (e) {
      setProblem(e instanceof Error ? 'unreadable' : skinsProblem(e));
    } finally {
      setBusy(false);
    }
  };

  const act = async (action: () => Promise<Wardrobe>, message: string) => {
    setBusy(true);
    setDone(null);
    setProblem(null);
    try {
      const next: unknown = await action();
      setWardrobe(isWardrobe(next) ? next : NOTHING);
      setDone(message);
      return true;
    } catch (e) {
      setProblem(skinsProblem(e));
      return false;
    } finally {
      setBusy(false);
    }
  };

  const wear = async () => {
    if (draft === null) return;
    const frameMs = durationsToSend(draft);
    if (frameMs === undefined) return;
    const { kind } = draft.picked;
    const ok = await act(
      () => ipc.skinsUpload(kind, kind === 'skin' && draft.slim, frameMs),
      `${kind === 'skin' ? s.skinUploaded : s.capeUploaded} ${s.inGame}`,
    );
    if (ok) setDraft(null);
  };

  const trial: Outfit | null =
    draft === null
      ? null
      : draft.picked.kind === 'skin'
        ? { skin: trialTexture(draft), cape: worn.cape }
        : { skin: worn.skin, cape: trialTexture(draft) };

  return (
    <section className={styles.studio} aria-label={s.title}>
      <div className={styles.studioHead}>
        <h3 className={styles.subtitle}>{s.title}</h3>
        <p className={styles.hint}>{s.hint}</p>
      </div>
      <div className={styles.columns}>
        <SkinStage
          worn={worn}
          trial={trial}
          nick={nick}
          frame={draft?.frame ?? null}
          overlay={draft !== null ? <FrameControls draft={draft} onChange={setDraft} /> : undefined}
        />
        <div className={styles.side}>
          {draft !== null ? (
            <NewTexture
              draft={draft}
              busy={busy}
              onChange={setDraft}
              onWear={() => void wear()}
              onCancel={() => {
                setDraft(null);
                setProblem(null);
              }}
            />
          ) : (
            (['skin', 'cape'] as const).map((kind) => (
              <Piece
                key={kind}
                kind={kind}
                worn={wardrobe[kind]}
                texture={worn[kind]}
                loading={
                  loading ||
                  (wardrobe[kind] !== null && (kind === 'skin' ? skinUrl : capeUrl) === null)
                }
                busy={busy}
                onPick={() => void pick(kind)}
                onDrop={(file) => void take(kind, file)}
                onRemove={() =>
                  void act(
                    () => ipc.skinsRemove(kind),
                    kind === 'skin' ? s.skinRemoved : s.capeRemoved,
                  )
                }
              />
            ))
          )}
          {done !== null && (
            <p role="status" className={styles.done}>
              {done}
            </p>
          )}
          {problem !== null && <Notice tone="warning">{s.problems[problem]}</Notice>}
        </div>
      </div>
    </section>
  );
}
