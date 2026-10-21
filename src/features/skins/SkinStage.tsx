import { type ReactNode, useEffect, useMemo, useState } from 'react';
import { Button } from '../../components/Button';
import { ipc, skinsProblem } from '../../lib/ipc';
import type { SkinsProblem } from '../../lib/contracts';
import { strings } from '../../lib/strings';
import { SkinPreview } from './SkinPreview';
import styles from './Skins.module.css';
import {
  BODY_ANIMATIONS,
  type BodyAnimation,
  type Frames,
  type Scene,
  Stage3D,
  VIEW_ANGLES,
  type ViewAngle,
  webglAvailable,
} from './stage3d';
import { factsOf } from './facts';
import {
  type Texture,
  cutFrames,
  frameDurations,
  loadImage,
  plainSkin,
  reducedMotion,
} from './textures';

const WIDTH = 440;
const HEIGHT = 520;
/** The small figure as it was, in a corner while something is tried on. */
const BEFORE_WIDTH = 120;
const BEFORE_HEIGHT = 152;
/** The saved picture, pixels square. */
const PICTURE = 1024;

type Background = 'theme' | 'day' | 'night' | 'cave';
const BACKGROUNDS: readonly Background[] = ['theme', 'day', 'night', 'cave'];

/** What the character wears. */
export interface Outfit {
  skin: Texture | null;
  cape: Texture | null;
}

let loads = 0;

/** A texture's frames, cut once per file; null while they are cut. */
function useFrames(texture: Texture | null): Frames | null {
  const [cut, setCut] = useState<{ src: string; key: string; frames: HTMLCanvasElement[] } | null>(
    null,
  );
  const src = texture?.src ?? null;
  const kind = texture?.kind;
  const width = texture?.width ?? 0;
  const height = texture?.height ?? 0;
  const count = texture?.frames ?? 1;
  useEffect(() => {
    if (src === null || kind === undefined) return;
    let live = true;
    loadImage(src)
      .then((image) => {
        if (!live) return;
        loads += 1;
        const frames = cutFrames(image, {
          src,
          kind,
          slim: false,
          width,
          height,
          frames: count,
          frameMs: [],
        });
        setCut({ src, key: String(loads), frames });
      })
      .catch(() => {
        if (live) setCut({ src, key: '', frames: [] });
      });
    return () => {
      live = false;
    };
  }, [src, kind, width, height, count]);
  const durations = texture === null ? '' : frameDurations(texture.frames, texture.frameMs).join();
  return useMemo(() => {
    if (cut === null || cut.src !== src || cut.frames.length === 0) return null;
    return { key: cut.key, frames: cut.frames, durations: durations.split(',').map(Number) };
  }, [cut, src, durations]);
}

/** The game's own skin, drawn by us (Mojang's may not be copied); drawn once a
 * character is there to wear it. */
function usePlainSkin(needed: boolean): Frames | null {
  return useMemo(
    () => (needed ? { key: 'plain', frames: [plainSkin()], durations: [100] } : null),
    [needed],
  );
}

function Chips<T extends string>({
  label,
  items,
  value,
  names,
  onPick,
}: {
  label: string;
  items: readonly T[];
  value: T | null;
  names: Record<T, string>;
  onPick: (item: T) => void;
}) {
  return (
    <div className={styles.chips} role="group" aria-label={label}>
      {items.map((item) => (
        <button
          key={item}
          type="button"
          className={styles.chip}
          aria-pressed={value === item}
          onClick={() => {
            onPick(item);
          }}
        >
          {names[item]}
        </button>
      ))}
    </div>
  );
}

function Toggle({
  label,
  on,
  disabled,
  onChange,
}: {
  label: string;
  on: boolean;
  disabled?: boolean;
  onChange: (on: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      className={styles.toggle}
      disabled={disabled}
      onClick={() => {
        onChange(!on);
      }}
    >
      <span className={styles.track} aria-hidden="true" />
      {label}
    </button>
  );
}

/** A character on a canvas, made when the canvas is there; null while it is made,
 * and `failed` when WebGL does not start. */
function useStage(
  canvas: HTMLCanvasElement | null,
  width: number,
  height: number,
): { stage: Stage3D | null; failed: boolean } {
  const [stage, setStage] = useState<Stage3D | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    if (canvas === null) return;
    let live = true;
    let made: Stage3D | null = null;
    Stage3D.create(canvas, width, height)
      .then((s) => {
        if (live) {
          made = s;
          setStage(s);
        } else {
          s.dispose();
        }
      })
      .catch(() => {
        if (live) setFailed(true);
      });
    return () => {
      live = false;
      made?.dispose();
      setStage(null);
    };
  }, [canvas, width, height]);
  return { stage, failed };
}

interface Props {
  /** What is worn on the account now. */
  worn: Outfit;
  /** What is being tried on (a picked file), or null. */
  trial: Outfit | null;
  /** The nick over the head and in the picture's name. */
  nick: string | null;
  /** A frame held still by the frame editor, or null. */
  frame: number | null;
  /** Shown over the bottom of the character (the frame editor's buttons). */
  overlay?: ReactNode;
}

/** The character and how it is shown (D-065): pose, angle, background, layers,
 * before and after, how players without the mod see it, a picture of it. */
export function SkinStage({ worn, trial, nick, frame, overlay }: Props) {
  const s = strings.accounts.skins;
  const [animation, setAnimation] = useState<BodyAnimation>('idle');
  const [view, setView] = useState<ViewAngle | null>('spin');
  const [background, setBackground] = useState<Background>('theme');
  const [layer2, setLayer2] = useState(true);
  const [nameTag, setNameTag] = useState(false);
  const [compare, setCompare] = useState(true);
  const [noMod, setNoMod] = useState(false);
  const [back, setBack] = useState<'cape' | 'elytra'>('cape');
  const [canvas, setCanvas] = useState<HTMLCanvasElement | null>(null);
  const [beforeCanvas, setBeforeCanvas] = useState<HTMLCanvasElement | null>(null);
  const [lost, setLost] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState<string | null>(null);
  const [problem, setProblem] = useState<SkinsProblem | null>(null);
  const can3d = useMemo(() => webglAvailable(), []);
  const moving = !reducedMotion();

  const shown: Outfit = noMod ? { skin: null, cape: null } : (trial ?? worn);
  const skinFrames = useFrames(shown.skin);
  const capeFrames = useFrames(shown.cape);
  const wornSkin = useFrames(worn.skin);
  const wornCape = useFrames(worn.cape);
  const showBefore = compare && trial !== null && !noMod;

  const main = useStage(can3d && !lost ? canvas : null, WIDTH, HEIGHT);
  const before = useStage(
    can3d && !lost && showBefore ? beforeCanvas : null,
    BEFORE_WIDTH,
    BEFORE_HEIGHT,
  );
  const flat = !can3d || lost || main.failed;
  const plain = usePlainSkin(main.stage !== null);

  const scene: Scene = {
    skin: shown.skin === null ? plain : skinFrames,
    slim: shown.skin?.slim ?? false,
    cape: capeFrames,
    back,
    animation,
    layer2,
    nameTag: nameTag && nick !== null ? nick : null,
    frame,
    moving,
  };
  useEffect(() => {
    main.stage?.set(scene);
  });
  useEffect(() => {
    before.stage?.set({
      ...scene,
      skin: worn.skin === null ? plain : wornSkin,
      slim: worn.skin?.slim ?? false,
      cape: wornCape,
      nameTag: null,
      frame: null,
    });
  });

  // A lost WebGL context (a driver reset, sleep) gives way to the flat view.
  useEffect(() => {
    if (canvas === null) return;
    const onLost = () => {
      setLost(true);
    };
    canvas.addEventListener('webglcontextlost', onLost);
    return () => {
      canvas.removeEventListener('webglcontextlost', onLost);
    };
  }, [canvas]);

  const pickView = (next: ViewAngle) => {
    setView(next);
    main.stage?.setView(next);
  };

  const savePicture = async () => {
    if (main.stage === null) return;
    setSaving(true);
    setSaved(null);
    setProblem(null);
    try {
      const url = main.stage.snapshot(PICTURE);
      const data = url.slice(url.indexOf(',') + 1);
      const name = await ipc.skinsSavePicture(
        data,
        nick ?? 'skin',
        strings.windowTitles.saveSkinPicture,
      );
      if (name !== null) setSaved(s.pictureSaved({ name }));
    } catch (e) {
      setProblem(skinsProblem(e));
    } finally {
      setSaving(false);
    }
  };

  const tag = noMod ? s.tag.noMod : trial !== null ? s.tag.trying : s.tag.worn;
  const showsCape = shown.cape !== null;

  return (
    <div className={styles.stage}>
      <span className={styles.stageTag} data-trying={trial !== null && !noMod}>
        {tag}
      </span>
      <div className={styles.view} data-background={background}>
        {flat ? (
          <div className={styles.flat}>
            {shown.skin !== null ? (
              <>
                <SkinPreview {...shown.skin} label={s.flatSkinFront} frame={frame ?? undefined} />
                <SkinPreview
                  {...shown.skin}
                  label={s.flatSkinBack}
                  side="back"
                  frame={frame ?? undefined}
                />
              </>
            ) : (
              <p className={styles.hint}>{noMod ? s.plainFacts : s.noSkin}</p>
            )}
            {shown.cape !== null && (
              <SkinPreview {...shown.cape} label={s.flatCape} frame={frame ?? undefined} />
            )}
            <p className={styles.hint}>{s.flat}</p>
          </div>
        ) : (
          <canvas
            ref={setCanvas}
            className={styles.canvas}
            width={WIDTH}
            height={HEIGHT}
            tabIndex={0}
            role="img"
            aria-label={s.stageLabel}
            onPointerEnter={() => main.stage?.hover(true)}
            onPointerLeave={() => main.stage?.hover(false)}
            onDoubleClick={() => main.stage?.reset()}
            onKeyDown={(event) => {
              if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
              event.preventDefault();
              setView(null);
              main.stage?.turn((event.key === 'ArrowLeft' ? -1 : 1) * (Math.PI / 12));
            }}
          />
        )}
        {showBefore && !flat && (
          <figure className={styles.before}>
            <canvas
              ref={setBeforeCanvas}
              width={BEFORE_WIDTH}
              height={BEFORE_HEIGHT}
              aria-hidden="true"
            />
            <figcaption>{s.before}</figcaption>
          </figure>
        )}
        {overlay !== undefined && <div className={styles.overlay}>{overlay}</div>}
      </div>

      <Chips
        label={s.animations.label}
        items={BODY_ANIMATIONS}
        value={animation}
        names={s.animations}
        onPick={setAnimation}
      />
      <div className={styles.chipRows}>
        <Chips
          label={s.views.label}
          items={VIEW_ANGLES}
          value={view}
          names={s.views}
          onPick={pickView}
        />
        <Chips
          label={s.backgrounds.label}
          items={BACKGROUNDS}
          value={background}
          names={s.backgrounds}
          onPick={setBackground}
        />
      </div>
      <div className={styles.chips}>
        <Toggle label={s.layer2} on={layer2} onChange={setLayer2} />
        <Toggle label={s.nameTag} on={nameTag} disabled={nick === null} onChange={setNameTag} />
        <Toggle label={s.compare} on={compare} onChange={setCompare} />
        <Toggle label={s.noMod} on={noMod} onChange={setNoMod} />
      </div>
      {showsCape && (
        <Chips
          label={s.backLabel}
          items={['cape', 'elytra'] as const}
          value={back}
          names={{ cape: s.backCape, elytra: s.backElytra }}
          onPick={setBack}
        />
      )}
      <div className={styles.facts}>
        {noMod ? (
          <p>{s.noModHint}</p>
        ) : (
          <>
            <p>
              <span className={styles.factsKind}>{s.skin}</span>{' '}
              {shown.skin !== null ? factsOf(shown.skin) : s.plainFacts}
            </p>
            {shown.cape !== null && (
              <p>
                <span className={styles.factsKind}>{s.cape}</span> {factsOf(shown.cape)}
              </p>
            )}
          </>
        )}
      </div>
      {!flat && (
        <div className={styles.pictureRow}>
          <Button disabled={saving || main.stage === null} onClick={() => void savePicture()}>
            {s.picture}
          </Button>
          {saved !== null && (
            <p role="status" dir="auto">
              {saved}
            </p>
          )}
          {problem !== null && <p role="alert">{s.problems[problem]}</p>}
        </div>
      )}
    </div>
  );
}
