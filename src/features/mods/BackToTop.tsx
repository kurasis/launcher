import { type RefObject, useEffect, useState } from 'react';
import { Icon } from '../../components/Icon';
import { strings } from '../../lib/strings';
import styles from './Catalog.module.css';

/** Scrolled this far, the way back up shows. */
const SHOWN_FROM = 600;

/**
 * "Back to top" over the launch bar once the catalog is scrolled down: it scrolls to
 * the top and puts the focus in the search box, so the keyboard starts there too.
 */
export function BackToTop({ target }: { target: RefObject<HTMLElement | null> }) {
  const [shown, setShown] = useState(false);
  useEffect(() => {
    let frame = 0;
    const check = () => {
      frame = 0;
      setShown(window.scrollY > SHOWN_FROM);
    };
    const onScroll = () => {
      if (frame === 0) frame = requestAnimationFrame(check);
    };
    check();
    window.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      window.removeEventListener('scroll', onScroll);
      cancelAnimationFrame(frame);
    };
  }, []);
  if (!shown) return null;
  return (
    <div className={styles.toTopDock}>
      <button
        type="button"
        className={styles.toTop}
        onClick={() => {
          const still = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
          window.scrollTo({ top: 0, behavior: still ? 'auto' : 'smooth' });
          target.current?.focus({ preventScroll: true });
        }}
      >
        <Icon name="up" size={18} />
        {strings.mods.catalog.backToTop}
      </button>
    </div>
  );
}
