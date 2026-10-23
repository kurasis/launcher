import { type ReactNode, useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import { type MenuItem, MoreMenu } from '../../app/MoreMenu';
import { Icon } from '../../components/Icon';
import { rowSpan, avatarTone, initial } from './mosaic';
import styles from './Feed.module.css';

/** The cells of the mosaic, and the content each is measured by. */
function contents(list: HTMLElement): Element[] {
  return Array.from(list.children).flatMap((cell) =>
    cell.firstElementChild === null ? [] : [cell.firstElementChild],
  );
}

/** Watches the cards in `now` and stops watching the ones that left. */
function watch(observer: ResizeObserver, now: Element[], watching: Set<Element>) {
  const current = new Set(now);
  for (const content of watching) {
    if (current.has(content)) continue;
    observer.unobserve(content);
    watching.delete(content);
  }
  for (const content of now) {
    if (watching.has(content)) continue;
    observer.observe(content);
    watching.add(content);
  }
}

/**
 * Gives each cell of the mosaic as many of its small rows as its card is tall. The
 * grid then puts each card at the top of the shortest column. Without sizes (no
 * layout yet, or a test) the grid stays a plain one, row by row.
 */
function layOut(list: HTMLElement) {
  const cells = Array.from(list.children).filter(
    (cell): cell is HTMLElement => cell instanceof HTMLElement,
  );
  const heights = cells.map((cell) => cell.firstElementChild?.getBoundingClientRect().height ?? 0);
  if (!heights.some((height) => height > 0)) {
    delete list.dataset.measured;
    return;
  }
  list.dataset.measured = 'true';
  const style = getComputedStyle(list);
  const row = Number.parseFloat(style.gridAutoRows);
  const gap = Number.parseFloat(style.columnGap) || 0;
  if (!(row > 0)) return;
  cells.forEach((cell, index) => {
    const span = `span ${String(rowSpan(heights[index] ?? 0, gap, row))}`;
    if (cell.style.gridRowEnd !== span) cell.style.gridRowEnd = span;
  });
}

/**
 * The mosaic (D-072): one list, three columns on a wide window, two or one when
 * narrow (container queries in the CSS). Cards keep the feed's order in the DOM.
 */
export function Mosaic({
  label,
  busy = false,
  children,
}: {
  label?: string;
  busy?: boolean;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const watcher = useRef<ResizeObserver | null>(null);
  const watched = useRef(new Set<Element>());
  useEffect(() => {
    const list = ref.current;
    if (list === null || typeof ResizeObserver === 'undefined') return;
    // Only the cards are watched: a wider or narrower list resizes every card, and a
    // card's height does not depend on its cell's rows. The rows are written in the
    // next frame, not while the observer delivers: a change there would ask for
    // another delivery in the same frame, which the browser reports as an error.
    let frame = 0;
    const observer = new ResizeObserver(() => {
      if (frame !== 0) return;
      frame = requestAnimationFrame(() => {
        frame = 0;
        layOut(list);
      });
    });
    const watching = watched.current;
    watch(observer, contents(list), watching);
    watcher.current = observer;
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
      watching.clear();
      watcher.current = null;
    };
  }, []);
  // After every render, before it is painted: new cards get their rows at once, and
  // cards that left the list are no longer watched (the list can run long).
  useLayoutEffect(() => {
    const list = ref.current;
    if (list === null) return;
    layOut(list);
    const observer = watcher.current;
    if (observer !== null) watch(observer, contents(list), watched.current);
  });
  return (
    <div className={styles.mosaicFrame}>
      <div
        ref={ref}
        className={styles.mosaic}
        role={label === undefined ? undefined : 'list'}
        aria-label={label}
        aria-busy={busy || undefined}
      >
        {children}
      </div>
    </div>
  );
}

/** One card's place in the mosaic. */
export function Cell({ children }: { children: ReactNode }) {
  return (
    <div role="listitem" className={styles.cell}>
      {children}
    </div>
  );
}

/** Columns a placeholder needs before it shows: the mosaic shows one per column. */
const SKELETON_FROM = [styles.fromOne, styles.fromTwo, styles.fromThree];

/**
 * Grey cards where cards are coming: `rows` for each column. Still, as drawn: a
 * shimmer would be a gradient, a pulse a long motion.
 */
export function Skeletons({ rows, tall = [] }: { rows: number; tall?: number[] }) {
  return Array.from({ length: rows * 3 }, (_, index) => (
    <div
      key={`skeleton-${String(index)}`}
      className={[styles.cell, SKELETON_FROM[Math.floor(index / rows)]].join(' ')}
      aria-hidden="true"
    >
      <div className={styles.skeleton}>
        <div className={tall.includes(index) ? styles.skeletonTall : styles.skeletonWide} />
        <div className={styles.skeletonLine} />
        <div className={styles.skeletonShort} />
      </div>
    </div>
  ));
}

const TONES = [styles.tone0, styles.tone1, styles.tone2, styles.tone3, styles.tone4];

/** A round letter in one of a few colors, chosen from the name. */
export function LetterAvatar({ name, square = false }: { name: string; square?: boolean }) {
  return (
    <span
      className={[styles.avatar, styles.letter, TONES[avatarTone(name)], square && styles.square]
        .filter(Boolean)
        .join(' ')}
      aria-hidden="true"
    >
      {initial(name)}
    </span>
  );
}

/**
 * A channel's or server's picture beside its name; its first letter when it has
 * none or it does not load. Decorative: the name is written next to it.
 */
export function Avatar({
  src,
  name,
  square = false,
}: {
  /** Already checked: a feed picture address or the core's data URL. */
  src: string | null;
  name: string;
  /** A server's icon: pixelated, with rounded corners, like a skin head. */
  square?: boolean;
}) {
  const [failed, setFailed] = useState<string | null>(null);
  if (src === null || failed === src) return <LetterAvatar name={name} square={square} />;
  return (
    <img
      className={[styles.avatar, square && styles.square].filter(Boolean).join(' ')}
      src={src}
      alt=""
      loading="lazy"
      onError={() => {
        setFailed(src);
      }}
    />
  );
}

export type KindTone = 'neutral' | 'success' | 'warning' | 'danger' | 'outline';

const KIND_TONES: Record<KindTone, string | undefined> = {
  neutral: styles.kindNeutral,
  success: styles.kindSuccess,
  warning: styles.kindWarning,
  danger: styles.kindDanger,
  outline: styles.kindOutline,
};

/** What a card is (Release, Video, Stream…), as a small chip in its meta line. */
export function KindChip({ tone, label }: { tone: KindTone; label: string }) {
  return <span className={[styles.kind, KIND_TONES[tone]].join(' ')}>{label}</span>;
}

/** The round Play in the middle of a video's picture. Decorative: the card's name says it. */
export function PlayCircle({ small = false }: { small?: boolean }) {
  return (
    <span className={small ? styles.playSmall : styles.play} aria-hidden="true">
      <Icon name="play" size={small ? 20 : 26} className={styles.filled} />
    </span>
  );
}

interface CardProps {
  /** What is on the picture: the picture itself and its chips. */
  media: ReactNode;
  /** The picture's shape, width over height. */
  aspect: number;
  title: string;
  /** The button's name when the title alone does not say what it does. */
  label?: string | undefined;
  /** What a press on the card does; null for a card that only shows. */
  onClick: (() => void) | null;
  /** The line under the title: source, kind and date. */
  meta: ReactNode;
  /** The ⋯ menu at the picture's top end. */
  menu?: ReactNode;
  /** Under the meta line, above the card's button (Install for a version). */
  extra?: ReactNode;
  /** Seen already: shown fainter. */
  dimmed?: boolean;
}

/**
 * A card of the mosaic: the picture, the title and its source under it, on the
 * paper ground. The title's button covers the whole card.
 */
export function Card({
  media,
  aspect,
  title,
  label,
  onClick,
  meta,
  menu = null,
  extra = null,
  dimmed = false,
}: CardProps) {
  const titleId = useId();
  return (
    <article
      className={[styles.card, dimmed && styles.watched, onClick !== null && styles.pressable]
        .filter(Boolean)
        .join(' ')}
      aria-labelledby={titleId}
    >
      <div className={styles.media} style={{ aspectRatio: aspect }}>
        {media}
      </div>
      <h3 id={titleId} className={styles.cardTitle} dir="auto">
        {onClick === null ? (
          title
        ) : (
          <button type="button" className={styles.cardButton} aria-label={label} onClick={onClick}>
            {title}
          </button>
        )}
      </h3>
      <p className={styles.meta}>{meta}</p>
      {extra !== null && <div className={styles.cardExtra}>{extra}</div>}
      {menu}
    </article>
  );
}

/** A picture filling its card's picture box; nothing when there is none. */
export function Picture({ src, className }: { src: string | null; className?: string }) {
  if (src === null) return null;
  return <img className={className} src={src} alt="" loading="lazy" />;
}

/** A card's ⋯ menu, at the top end of its picture. */
export function CardMenu({
  label,
  entries,
  onOpen,
}: {
  label: string;
  entries: readonly MenuItem[];
  onOpen?: (() => void) | undefined;
}) {
  return (
    <div className={styles.cardMenu}>
      <MoreMenu
        label={label}
        ariaLabel={label}
        icon={<Icon name="more" size={18} />}
        buttonClassName={styles.menuButton}
        caret={false}
        current={false}
        entries={entries}
        {...(onOpen === undefined ? {} : { onOpen })}
      />
    </div>
  );
}
