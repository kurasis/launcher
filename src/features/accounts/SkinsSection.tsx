import { useEffect, useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
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
import { SkinPreview } from './SkinPreview';
import styles from './Accounts.module.css';
import { FRAME_MS_MAX, FRAME_MS_MIN, isWardrobe, parseFrameMs } from './skinTextures';

const NOTHING: Wardrobe = { skin: null, cape: null };

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
        if (!live) return;
        const png = typeof url === 'string' && url.startsWith('data:image/png;base64,');
        setTexture({ hash, url: png ? url : '' });
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

interface PieceProps {
  kind: SkinKind;
  worn: WornTexture | null;
  loading: boolean;
  busy: boolean;
  /** Runs an action; its wardrobe replaces the shown one and `done` is said. */
  act: (action: () => Promise<Wardrobe>, done: string) => Promise<boolean>;
  onProblem: (problem: SkinsProblem | null) => void;
}

/** One of the two: what is worn, and picking, checking and uploading a new one. */
function Piece({ kind, worn, loading, busy, act, onProblem }: PieceProps) {
  const s = strings.accounts.skins;
  const skin = kind === 'skin';
  const url = useTexture(worn?.hash ?? null);
  const [picked, setPicked] = useState<PickedTexture | null>(null);
  const [slim, setSlim] = useState(false);
  const [frameMsText, setFrameMsText] = useState('100');
  const [picking, setPicking] = useState(false);
  const frameMs = parseFrameMs(frameMsText);
  const animated = picked !== null && picked.frames > 1;

  const pick = async () => {
    setPicking(true);
    onProblem(null);
    try {
      const file = await ipc.skinsPick(
        kind,
        skin ? strings.windowTitles.pickSkin : strings.windowTitles.pickCape,
      );
      if (file !== null && typeof file === 'object') {
        setPicked(file);
        setSlim(worn?.slim ?? false);
      }
    } catch (e) {
      setPicked(null);
      onProblem(skinsProblem(e));
    } finally {
      setPicking(false);
    }
  };

  const upload = async () => {
    if (picked === null || (animated && frameMs === null)) return;
    const ok = await act(
      () => ipc.skinsUpload(kind, skin && slim, animated && frameMs !== null ? [frameMs] : null),
      skin ? s.skinUploaded : s.capeUploaded,
    );
    if (ok) setPicked(null);
  };

  const disabled = busy || picking;
  return (
    <div className={styles.row}>
      <h4 className={styles.subtitle}>{skin ? s.skin : s.cape}</h4>
      <div className={styles.previews}>
        {worn !== null && url !== null && url !== '' && (
          <SkinPreview
            src={url}
            kind={kind}
            slim={worn.slim}
            width={worn.width}
            height={worn.height}
            frames={worn.frames}
            frameMs={worn.frameMs}
            label={skin ? s.previewSkin : s.previewCape}
          />
        )}
        {worn === null && !loading && <p className={styles.hint}>{skin ? s.noSkin : s.noCape}</p>}
        {(loading || (worn !== null && url === null)) && <p className={styles.hint}>{s.loading}</p>}
      </div>
      {picked === null ? (
        <div className={forms.actions}>
          <Button disabled={disabled} onClick={() => void pick()}>
            {skin ? s.uploadSkin : s.uploadCape}
          </Button>
          {worn !== null && (
            <Button
              disabled={disabled}
              onClick={() =>
                void act(() => ipc.skinsRemove(kind), skin ? s.skinRemoved : s.capeRemoved)
              }
            >
              {skin ? s.removeSkin : s.removeCape}
            </Button>
          )}
        </div>
      ) : (
        <form
          className={forms.form}
          aria-label={skin ? s.uploadSkin : s.uploadCape}
          onSubmit={(event) => {
            event.preventDefault();
            void upload();
          }}
        >
          <p dir="auto">
            {s.picked({
              name: picked.name,
              width: picked.width,
              height: picked.height,
              frames: picked.frames,
            })}
          </p>
          <div className={styles.previews}>
            <SkinPreview
              src={picked.preview}
              kind={kind}
              slim={skin && slim}
              width={picked.width}
              height={picked.height}
              frames={picked.frames}
              frameMs={frameMs === null ? [] : [frameMs]}
              label={s.previewPicked}
            />
          </div>
          {skin && (
            <fieldset className={forms.choices} disabled={disabled}>
              <legend>{s.model}</legend>
              <label className={forms.choice}>
                <input
                  type="radio"
                  name="skin-model"
                  checked={!slim}
                  onChange={() => {
                    setSlim(false);
                  }}
                />
                {s.classic}
              </label>
              <label className={forms.choice}>
                <input
                  type="radio"
                  name="skin-model"
                  checked={slim}
                  onChange={() => {
                    setSlim(true);
                  }}
                />
                {s.slim}
              </label>
            </fieldset>
          )}
          {animated && (
            <Field label={s.frameMs} hint={s.frameMsHint}>
              {(props) => (
                <input
                  {...props}
                  className={forms.input}
                  type="number"
                  inputMode="numeric"
                  min={FRAME_MS_MIN}
                  max={FRAME_MS_MAX}
                  step={10}
                  dir="ltr"
                  disabled={disabled}
                  aria-invalid={frameMs === null}
                  value={frameMsText}
                  onChange={(event) => {
                    setFrameMsText(event.target.value.slice(0, 6));
                  }}
                />
              )}
            </Field>
          )}
          <p className={styles.hint}>{s.limit}</p>
          <div className={forms.actions}>
            <Button
              type="submit"
              variant="primary"
              disabled={disabled || (animated && frameMs === null)}
            >
              {s.send}
            </Button>
            <Button
              disabled={disabled}
              onClick={() => {
                setPicked(null);
              }}
            >
              {s.cancel}
            </Button>
          </div>
        </form>
      )}
    </div>
  );
}

/** The launcher account's skin and cape at skins.lapi.sh (D-065): what is worn, a
 * preview, upload of a checked PNG, removal. */
export function SkinsSection() {
  const s = strings.accounts.skins;
  const [wardrobe, setWardrobe] = useState<Wardrobe>(NOTHING);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<SkinsProblem | null>(null);
  const [done, setDone] = useState<string | null>(null);

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

  const onProblem = (next: SkinsProblem | null) => {
    setProblem(next);
    if (next === null) setDone(null);
  };

  return (
    <section className={styles.skins} aria-label={s.title}>
      <h3 className={styles.subtitle}>{s.title}</h3>
      <p className={styles.hint}>{s.hint}</p>
      <div className={styles.pieces}>
        <Piece
          kind="skin"
          worn={wardrobe.skin}
          loading={loading}
          busy={busy}
          act={act}
          onProblem={onProblem}
        />
        <Piece
          kind="cape"
          worn={wardrobe.cape}
          loading={loading}
          busy={busy}
          act={act}
          onProblem={onProblem}
        />
      </div>
      {done !== null && <p role="status">{done}</p>}
      {problem !== null && <Notice tone="warning">{s.problems[problem]}</Notice>}
    </section>
  );
}
