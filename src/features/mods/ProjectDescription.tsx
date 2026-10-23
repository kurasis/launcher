import { useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import { Icon } from '../../components/Icon';
import type { ModsProject } from '../../lib/contracts';
import { strings } from '../../lib/strings';
import { Dialog } from './Dialog';
import { ourPicture } from './helpers';
import { RichText } from './RichText';
import catalog from './Catalog.module.css';
import styles from './Project.module.css';

/** Pictures drawn at most; the core keeps no more. */
const PICTURES = 64;

interface Picture {
  thumb: string;
  full: string;
  title: string;
  description: string;
}

/** The gallery's pictures the interface may draw: ours only (D-066). */
function picturesOf(project: ModsProject): Picture[] {
  const pictures: Picture[] = [];
  for (const picture of project.gallery.slice(0, PICTURES)) {
    const full = ourPicture(picture.url) ?? ourPicture(picture.thumb);
    const thumb = ourPicture(picture.thumb) ?? full;
    if (full === null || thumb === null) continue;
    pictures.push({ thumb, full, title: picture.title, description: picture.description });
  }
  return pictures;
}

/** One picture at full size, with the previous and next on the arrow keys. */
function PictureDialog({
  pictures,
  index,
  onIndex,
  onClose,
}: {
  pictures: Picture[];
  index: number;
  onIndex: (index: number) => void;
  onClose: () => void;
}) {
  const t = strings.mods.project;
  const picture = pictures[index];
  const count = pictures.length;
  const step = (by: number) => {
    onIndex((index + by + count) % count);
  };
  const stepRef = useRef(step);
  useLayoutEffect(() => {
    stepRef.current = step;
  });
  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
      event.preventDefault();
      // The picture after this one is to the right, or to the left in Arabic.
      const rtl = document.documentElement.dir === 'rtl';
      stepRef.current((event.key === 'ArrowRight') !== rtl ? 1 : -1);
    };
    window.addEventListener('keydown', listener);
    return () => {
      window.removeEventListener('keydown', listener);
    };
  }, []);
  if (picture === undefined) return null;
  const counter = t.picture({ index: index + 1, count });
  return (
    <Dialog title={picture.title === '' ? counter : picture.title} onClose={onClose} wide>
      <figure className={styles.figure}>
        <img
          className={styles.fullPicture}
          src={picture.full}
          alt={picture.description || picture.title || counter}
          referrerPolicy="no-referrer"
        />
        {picture.description !== '' && (
          <figcaption className={styles.hint} dir="auto">
            {picture.description}
          </figcaption>
        )}
      </figure>
      <div className={styles.dialogActions}>
        {count > 1 && (
          <>
            <button
              type="button"
              className={[catalog.smallButton, styles.ghostButton].join(' ')}
              onClick={() => {
                step(-1);
              }}
            >
              <Icon name="prev" size={18} className={catalog.mirror} />
              {t.previousPicture}
            </button>
            <span className={styles.hint} aria-live="polite">
              {counter}
            </span>
            <button
              type="button"
              className={[catalog.smallButton, styles.ghostButton].join(' ')}
              onClick={() => {
                step(1);
              }}
            >
              {t.nextPicture}
              <Icon name="next" size={18} className={catalog.mirror} />
            </button>
          </>
        )}
        <span className={catalog.spacer} />
        <button
          type="button"
          className={[catalog.smallButton, styles.ghostButton].join(' ')}
          onClick={onClose}
        >
          {t.close}
        </button>
      </div>
    </Dialog>
  );
}

/**
 * The description, shortened until the player asks for all of it, and the gallery as
 * a strip of pictures that open at full size.
 */
export function ProjectDescription({
  project,
  onLink,
}: {
  project: ModsProject;
  onLink: (url: string) => void;
}) {
  const t = strings.mods.project;
  const headingId = useId();
  const textId = useId();
  const box = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const [whole, setWhole] = useState(false);
  // The text is taller than the shortened box.
  const [long, setLong] = useState(false);
  const [open, setOpen] = useState<number | null>(null);
  const pictures = picturesOf(project);

  // Measured against the shortened box after the browser lays it out, and again when
  // pictures in it load; not while it is open, where the box is as tall as the text.
  useEffect(() => {
    const outer = box.current;
    const inner = content.current;
    if (whole || outer === null || inner === null) return;
    let frame = 0;
    const measure = () => {
      if (frame !== 0) return;
      frame = requestAnimationFrame(() => {
        frame = 0;
        setLong(inner.offsetHeight > outer.clientHeight + 1);
      });
    };
    measure();
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure);
    observer?.observe(inner);
    return () => {
      cancelAnimationFrame(frame);
      observer?.disconnect();
    };
  }, [project.body, whole]);

  return (
    <section className={styles.panel} aria-labelledby={headingId}>
      <h3 id={headingId} className={styles.panelTitle}>
        {t.description}
      </h3>
      <div
        ref={box}
        id={textId}
        className={whole ? styles.text : [styles.text, styles.short].join(' ')}
        onFocus={() => {
          // A link further down the shortened text is shown when it takes the focus.
          if (long) setWhole(true);
        }}
      >
        <div ref={content}>
          {project.body.length > 0 ? (
            <RichText nodes={project.body} onLink={onLink} />
          ) : (
            <p dir="auto">{project.summary}</p>
          )}
        </div>
      </div>
      {(long || whole) && (
        <button
          type="button"
          className={catalog.linkButton}
          aria-expanded={whole}
          aria-controls={textId}
          onClick={() => {
            setWhole(!whole);
          }}
        >
          {whole ? t.showLess : t.showWhole}
        </button>
      )}
      {pictures.length > 0 && (
        <ul className={styles.gallery} aria-label={t.gallery}>
          {pictures.map((picture, index) => (
            <li key={picture.full}>
              <button
                type="button"
                className={styles.thumb}
                aria-label={
                  picture.title === ''
                    ? t.picture({ index: index + 1, count: pictures.length })
                    : t.pictureNamed({
                        index: index + 1,
                        count: pictures.length,
                        title: picture.title,
                      })
                }
                onClick={() => {
                  setOpen(index);
                }}
              >
                <img src={picture.thumb} alt="" loading="lazy" referrerPolicy="no-referrer" />
              </button>
            </li>
          ))}
        </ul>
      )}
      {open !== null && (
        <PictureDialog
          pictures={pictures}
          index={open}
          onIndex={setOpen}
          onClose={() => {
            setOpen(null);
          }}
        />
      )}
    </section>
  );
}
