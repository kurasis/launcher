import type { ReactElement } from 'react';
import styles from './Icon.module.css';

// Concept A's icons: simple 24×24 strokes in the text color, drawn for the launcher.
const paths = {
  feed: (
    <>
      <path d="M4 5h13v14H4z" />
      <path d="M17 8h3v9a2 2 0 0 1-2 2" />
      <path d="M7 9h7M7 12h7M7 15h4" />
    </>
  ),
  home: (
    <>
      <path d="M12 3 4 7v10l8 4 8-4V7z" />
      <path d="M4 7l8 4 8-4M12 11v10" />
    </>
  ),
  mods: (
    <>
      <path d="M10 3.5a2 2 0 0 1 4 0V5h3a1 1 0 0 1 1 1v3h1.5a2 2 0 0 1 0 4H18v3a1 1 0 0 1-1 1h-3v-1.5a2 2 0 0 0-4 0V17H7a1 1 0 0 1-1-1v-3H4.5a2 2 0 0 1 0-4H6V6a1 1 0 0 1 1-1h3z" />
    </>
  ),
  worlds: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M3.5 12h17M12 3.5c3 3 3 14 0 17M12 3.5c-3 3-3 14 0 17" />
    </>
  ),
  skins: (
    <>
      <path d="M8 4 5 6l-2 4 3 1.5V20h12v-8.5L21 10l-2-4-3-2a4 4 0 0 1-8 0z" />
    </>
  ),
  account: (
    <>
      <circle cx="12" cy="8" r="4" />
      <path d="M4 20a8 8 0 0 1 16 0" />
    </>
  ),
  settings: (
    <>
      <path d="M4 7h10M18 7h2M4 17h4M12 17h8" />
      <circle cx="16" cy="7" r="2" />
      <circle cx="10" cy="17" r="2" />
    </>
  ),
  friends: (
    <>
      <path d="M5 5h14a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1H9l-4 3.5V6a1 1 0 0 1 1-1z" />
    </>
  ),
  downloads: (
    <>
      <path d="M12 4v11M7 10l5 5 5-5M5 20h14" />
    </>
  ),
  more: (
    <>
      <circle cx="6" cy="12" r="1.5" />
      <circle cx="12" cy="12" r="1.5" />
      <circle cx="18" cy="12" r="1.5" />
    </>
  ),
  search: (
    <>
      <circle cx="11" cy="11" r="6.5" />
      <path d="M20 20l-4-4" />
    </>
  ),
  power: (
    <>
      <path d="M12 4v8M6.3 7.3a8 8 0 1 0 11.4 0" />
    </>
  ),
  plus: (
    <>
      <path d="M12 5v14M5 12h14" />
    </>
  ),
  play: (
    <>
      <path d="M8 5.5v13l10-6.5z" />
    </>
  ),
  check: (
    <>
      <path d="M4 12.5 9.5 18 20 7" />
    </>
  ),
  up: (
    <>
      <path d="M6 15l6-6 6 6" />
    </>
  ),
  chev: (
    <>
      <path d="M6 9l6 6 6-6" />
    </>
  ),
  chevr: (
    <>
      <path d="M9 6l6 6-6 6" />
    </>
  ),
  down: (
    <>
      <path d="M6 9l6 6 6-6" />
    </>
  ),
  pin: (
    <>
      <path d="M12 21s-6.5-5.6-6.5-11a6.5 6.5 0 0 1 13 0c0 5.4-6.5 11-6.5 11z" />
      <circle cx="12" cy="10" r="2.3" />
    </>
  ),
  users: (
    <>
      <circle cx="9" cy="8" r="3.5" />
      <path d="M2.5 20a6.5 6.5 0 0 1 13 0" />
      <path d="M16 4.5a3.5 3.5 0 0 1 0 7M21.5 20a6.5 6.5 0 0 0-4.5-6.2" />
    </>
  ),
  sun: (
    <>
      <circle cx="12" cy="12" r="4" />
      <path d="M12 2.5v3M12 18.5v3M2.5 12h3M18.5 12h3M5.3 5.3l2.1 2.1M16.6 16.6l2.1 2.1M18.7 5.3l-2.1 2.1M7.4 16.6l-2.1 2.1" />
    </>
  ),
  palette: (
    <>
      <path d="M12 3.5a8.5 8.5 0 1 0 0 17c1.2 0 1.6-.8 1.3-1.8-.3-1 .2-1.9 1.3-1.9H16a4.5 4.5 0 0 0 4.5-4.5C20.5 7.6 16.7 3.5 12 3.5z" />
      <circle cx="8" cy="11" r="1" />
      <circle cx="11" cy="7.5" r="1" />
      <circle cx="15.5" cy="8.5" r="1" />
    </>
  ),
  db: (
    <>
      <ellipse cx="12" cy="6" rx="7.5" ry="3" />
      <path d="M4.5 6v12c0 1.7 3.4 3 7.5 3s7.5-1.3 7.5-3V6" />
      <path d="M4.5 12c0 1.7 3.4 3 7.5 3s7.5-1.3 7.5-3" />
    </>
  ),
  layers: (
    <>
      <path d="m12 4 8.5 4.5L12 13 3.5 8.5z" />
      <path d="m3.5 12.5 8.5 4.5 8.5-4.5M3.5 16.5 12 21l8.5-4.5" />
    </>
  ),
  map: (
    <>
      <path d="m3.5 6 5.5-2 6 2 5.5-2v14l-5.5 2-6-2-5.5 2z" />
      <path d="M9 4v14M15 6v14" />
    </>
  ),
  grid: (
    <>
      <rect x="4" y="4" width="6.5" height="6.5" rx="1.5" />
      <rect x="13.5" y="4" width="6.5" height="6.5" rx="1.5" />
      <rect x="4" y="13.5" width="6.5" height="6.5" rx="1.5" />
      <rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1.5" />
    </>
  ),
  list: (
    <>
      <path d="M8 6.5h12M8 12h12M8 17.5h12" />
      <circle cx="4.5" cy="6.5" r="1" />
      <circle cx="4.5" cy="12" r="1" />
      <circle cx="4.5" cy="17.5" r="1" />
    </>
  ),
  star: (
    <>
      <path d="M12 3.5l2.6 5.6 6 .7-4.4 4.2 1.1 6-5.3-3-5.3 3 1.1-6L3.4 9.8l6-.7z" />
    </>
  ),
  heart: (
    <>
      <path d="M12 20s-7.5-4.6-7.5-10A4.2 4.2 0 0 1 12 7.6 4.2 4.2 0 0 1 19.5 10c0 5.4-7.5 10-7.5 10z" />
    </>
  ),
  eye: (
    <>
      <path d="M2.5 12s3.5-6.5 9.5-6.5S21.5 12 21.5 12s-3.5 6.5-9.5 6.5S2.5 12 2.5 12z" />
      <circle cx="12" cy="12" r="3" />
    </>
  ),
  link: (
    <>
      <path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1.5 1.5" />
      <path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1.5-1.5" />
    </>
  ),
  folder: (
    <>
      <path d="M3 6h6l2 2h10v11H3z" />
    </>
  ),
  refresh: (
    <>
      <path d="M20 11a8 8 0 1 0-2.3 5.7" />
      <path d="M20 5v6h-6" />
    </>
  ),
  upload: (
    <>
      <path d="M12 16V5M7 10l5-5 5 5M5 20h14" />
    </>
  ),
  bell: (
    <>
      <path d="M6 16V11a6 6 0 0 1 12 0v5l1.5 2h-15z" />
      <path d="M10 20a2 2 0 0 0 4 0" />
    </>
  ),
  x: (
    <>
      <path d="M6 6l12 12M18 6 6 18" />
    </>
  ),
  back: (
    <>
      <path d="M15 6l-6 6 6 6" />
    </>
  ),
  prev: (
    <>
      <path d="m14.5 6-6 6 6 6" />
    </>
  ),
  next: (
    <>
      <path d="m9.5 6 6 6-6 6" />
    </>
  ),
  stop: (
    <>
      <path d="M6 6h12v12H6z" />
    </>
  ),
  trash: (
    <>
      <path d="M5 7h14M9 7V4h6v3M7 7l1 13h8l1-13" />
    </>
  ),
  edit: (
    <>
      <path d="M4 20h4L19 9l-4-4L4 16z" />
    </>
  ),
  copy: (
    <>
      <path d="M9 9h11v11H9z" />
      <path d="M5 15V4h11" />
    </>
  ),
  filter: (
    <>
      <path d="M4 6h16M7 12h10M10 18h4" />
    </>
  ),
  sort: (
    <>
      <path d="M7 4v16M4 17l3 3 3-3M17 20V4M14 7l3-3 3 3" />
    </>
  ),
  clock: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 7v5l3 2" />
    </>
  ),
  shield: (
    <>
      <path d="M12 3l8 3v6c0 4.5-3.5 7.8-8 9-4.5-1.2-8-4.5-8-9V6z" />
    </>
  ),
  info: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 11v5M12 8h.01" />
    </>
  ),
  warn: (
    <>
      <path d="M12 4l9 16H3z" />
      <path d="M12 10v4M12 17h.01" />
    </>
  ),
  cape: (
    <>
      <path d="M7 4.5h10l1.5 15.5H5.5z" />
      <path d="M9 4.5v15.5M15 4.5v15.5" />
    </>
  ),
  wand: (
    <>
      <path d="m4 20 11-11M13 4.5l1 2.2 2.2 1-2.2 1-1 2.2-1-2.2-2.2-1 2.2-1zM18.5 12l.6 1.4 1.4.6-1.4.6-.6 1.4-.6-1.4-1.4-.6 1.4-.6z" />
    </>
  ),
  sparkles: (
    <>
      <path d="M12 3.5 13.9 9.6 20 11.5l-6.1 1.9L12 19.5l-1.9-6.1L4 11.5l6.1-1.9z" />
      <path d="M19 3v3M17.5 4.5h3" />
    </>
  ),
  bookmark: (
    <>
      <path d="M6.5 4.5h11v15L12 15.5 6.5 19.5z" />
    </>
  ),
  share: (
    <>
      <circle cx="18" cy="5.5" r="2.5" />
      <circle cx="6" cy="12" r="2.5" />
      <circle cx="18" cy="18.5" r="2.5" />
      <path d="m8.2 10.8 7.6-4.1M8.2 13.2l7.6 4.1" />
    </>
  ),
  comment: (
    <>
      <path d="M4 6.5A2.5 2.5 0 0 1 6.5 4h11A2.5 2.5 0 0 1 20 6.5v8a2.5 2.5 0 0 1-2.5 2.5H10l-5 4z" />
    </>
  ),
  inbox: (
    <>
      <path d="M4 13.5V18a1.5 1.5 0 0 0 1.5 1.5h13A1.5 1.5 0 0 0 20 18v-4.5" />
      <path d="M4 13.5h4.5l1.5 2.5h4l1.5-2.5H20" />
      <path d="M6.5 13.5 8.5 5h7l2 8.5" />
    </>
  ),
  pause: (
    <>
      <path d="M8 5.5v13M16 5.5v13" />
    </>
  ),
  image: (
    <>
      <path d="M4 5h16v14H4z" />
      <path d="M4 16l5-5 4 4 3-3 4 4" />
      <circle cx="16" cy="9" r="1.5" />
    </>
  ),
  camera: (
    <>
      <path d="M4 8h3l2-3h6l2 3h3v11H4z" />
      <circle cx="12" cy="13" r="3.5" />
    </>
  ),
  console: (
    <>
      <path d="M4 5h16v14H4z" />
      <path d="M8 10l3 2-3 2M13 14h4" />
    </>
  ),
  copies: (
    <>
      <path d="M8 8h12v12H8z" />
      <path d="M4 16V4h12" />
    </>
  ),
  stethoscope: (
    <>
      <path d="M6 4v6a4 4 0 0 0 8 0V4" />
      <path d="M10 14v2a5 5 0 0 0 10 0v-2" />
      <circle cx="20" cy="12" r="2" />
    </>
  ),
  update: (
    <>
      <path d="M12 20V9M7 14l5-5 5 5M5 4h14" />
    </>
  ),
  video: (
    <>
      <path d="M4 6h12v12H4z" />
      <path d="M16 10l4-2v8l-4-2" />
    </>
  ),
  live: (
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M6.5 6.5a8 8 0 0 0 0 11M17.5 6.5a8 8 0 0 1 0 11" />
    </>
  ),
  key: (
    <>
      <circle cx="8" cy="15" r="4" />
      <path d="M11 12l9-9M16 7l3 3M18.5 4.5l1.5 1.5" />
    </>
  ),
  import: (
    <>
      <path d="M20 12H9M13 8l-4 4 4 4" />
      <path d="M4 4v16" />
    </>
  ),
  send: (
    <>
      <path d="M4 12l16-8-6 16-2-7z" />
    </>
  ),
} satisfies Record<string, ReactElement>;

export type IconName = keyof typeof paths;

interface IconProps {
  name: IconName;
  /** Pixels; the icon is square. */
  size?: number;
  className?: string | undefined;
}

/** A decorative icon: always hidden from assistive technology, the text next to it names the action. */
export function Icon({ name, size = 20, className }: IconProps) {
  return (
    <svg
      className={[styles.icon, className].filter(Boolean).join(' ')}
      viewBox="0 0 24 24"
      width={size}
      height={size}
      aria-hidden="true"
      focusable="false"
    >
      {paths[name]}
    </svg>
  );
}
