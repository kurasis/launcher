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
  gauge: (
    <>
      <path d="M6 19a8.5 8.5 0 1 1 12 0" />
      <path d="m12 13 4-4" />
      <circle cx="12" cy="13" r="1" />
    </>
  ),
  wrench: (
    <>
      <path d="M12.69 13.43 6.51 19.61a1.5 1.5 0 0 1-2.12-2.12l6.18-6.18a5 5 0 0 1 6.5-6.86l-2.95 2.96 2.47 2.47 2.96-2.95a5 5 0 0 1-6.86 6.5z" />
    </>
  ),
  books: (
    <>
      <path d="M3.5 19.5h17" />
      <path d="M4.5 19.5V4.5h4v15M8.5 19.5V7.5h4v12" />
      <path d="M16.86 19.5l3.38-.91-3.24-12.07-3.38.91z" />
    </>
  ),
  gear: (
    <>
      <path d="M10.13 5.88 10.38 3.66A8.5 8.5 0 0 1 13.62 3.66L13.87 5.88A6.4 6.4 0 0 1 15 6.35L16.75 4.95A8.5 8.5 0 0 1 19.05 7.25L17.65 9A6.4 6.4 0 0 1 18.12 10.13L20.34 10.38A8.5 8.5 0 0 1 20.34 13.62L18.12 13.87A6.4 6.4 0 0 1 17.65 15L19.05 16.75A8.5 8.5 0 0 1 16.75 19.05L15 17.65A6.4 6.4 0 0 1 13.87 18.12L13.62 20.34A8.5 8.5 0 0 1 10.38 20.34L10.13 18.12A6.4 6.4 0 0 1 9 17.65L7.25 19.05A8.5 8.5 0 0 1 4.95 16.75L6.35 15A6.4 6.4 0 0 1 5.88 13.87L3.66 13.62A8.5 8.5 0 0 1 3.66 10.38L5.88 10.13A6.4 6.4 0 0 1 6.35 9L4.95 7.25A8.5 8.5 0 0 1 7.25 4.95L9 6.35A6.4 6.4 0 0 1 10.13 5.88z" />
      <circle cx="12" cy="12" r="2.5" />
    </>
  ),
  compass: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="m15.5 8.5-2 5-5 2 2-5z" />
    </>
  ),
  mountains: (
    <>
      <path d="M3 19h18" />
      <path d="m4 19 5.5-11L15 19M12.9 14.8l2.6-3.3 5 7.5" />
      <circle cx="17" cy="6" r="1.5" />
    </>
  ),
  paw: (
    <>
      <path d="M12 12c-2.5 0-5 2.8-5.5 5-.4 1.6.8 2.8 2.3 2.6 1.2-.2 2.2-.6 3.2-.6s2 .4 3.2.6c1.5.2 2.7-1 2.3-2.6-.5-2.2-3-5-5.5-5z" />
      <ellipse cx="9.25" cy="6.75" rx="1.35" ry="1.85" />
      <ellipse cx="14.75" cy="6.75" rx="1.35" ry="1.85" />
      <path d="M4.23 11.27A1.35 1.85 150 0 1 6.57 9.93 1.35 1.85 150 0 1 4.23 11.27zM19.77 11.27a1.35 1.85 30 0 1-2.34-1.34 1.35 1.85 30 0 1 2.34 1.34z" />
    </>
  ),
  sword: (
    <>
      <path d="m10.74 15.74 8.23-8.24L20 4l-3.5 1.03-8.24 8.23" />
      <path d="m6.85 11.85 5.3 5.3M8.37 15.63 4.6 19.4" />
    </>
  ),
  armchair: (
    <>
      <path d="M6.5 12V7.5A2.5 2.5 0 0 1 9 5h6a2.5 2.5 0 0 1 2.5 2.5V12" />
      <path d="M5 10.5A1.5 1.5 0 0 0 3.5 12v5h17v-5a1.5 1.5 0 0 0-3 0v1.5h-11V12A1.5 1.5 0 0 0 5 10.5z" />
      <path d="M5.5 17v2.5M18.5 17v2.5" />
    </>
  ),
  apple: (
    <>
      <path d="M12 9c-1.4-1.1-3.2-1.4-4.7-.8C5 9.1 4.2 11.3 4.6 13.9c.5 3.3 2.8 6.6 5.1 6.6.9 0 1.5-.5 2.3-.5s1.4.5 2.3.5c2.3 0 4.6-3.3 5.1-6.6.4-2.6-.4-4.8-2.7-5.7-1.5-.6-3.3-.3-4.7.8z" />
      <path d="M12 9c0-1.5.3-2.8 1-4" />
      <path d="M12.6 6.4c.6-1.8 2.3-2.9 4.9-2.9-.4 2-2.2 3.1-4.9 2.9z" />
    </>
  ),
  chest: (
    <>
      <rect x="4" y="5.5" width="16" height="13.5" rx="1.5" />
      <path d="M4 10.5h6.5M13.5 10.5H20" />
      <rect x="10.5" y="9" width="3" height="4" rx="0.75" />
    </>
  ),
  cart: (
    <>
      <path d="M4.5 5h15l-2 7h-11z" />
      <circle cx="9" cy="15" r="1.5" />
      <circle cx="15" cy="15" r="1.5" />
      <path d="M3 19.5h18" />
    </>
  ),
  sliders: (
    <>
      <path d="M6 4v2M6 10v10M12 4v9M12 17v3M18 4v4M18 12v8" />
      <circle cx="6" cy="8" r="2" />
      <circle cx="12" cy="15" r="2" />
      <circle cx="18" cy="10" r="2" />
    </>
  ),
  server: (
    <>
      <rect x="4" y="4" width="16" height="6.5" rx="1.5" />
      <rect x="4" y="13.5" width="16" height="6.5" rx="1.5" />
      <circle cx="7.5" cy="7.25" r="0.75" />
      <circle cx="7.5" cy="16.75" r="0.75" />
      <path d="M11.5 7.25h5M11.5 16.75h5" />
    </>
  ),
  chat: (
    <>
      <path d="M14.5 8V5.5A1.5 1.5 0 0 0 13 4H5a1.5 1.5 0 0 0-1.5 1.5v10l3-3H8" />
      <path d="M11 9.5h8a1.5 1.5 0 0 1 1.5 1.5v9.5l-3-3H11a1.5 1.5 0 0 1-1.5-1.5V11A1.5 1.5 0 0 1 11 9.5z" />
    </>
  ),
  die: (
    <>
      <rect x="4" y="4" width="16" height="16" rx="3" />
      <circle cx="8" cy="8" r="1" />
      <circle cx="12" cy="12" r="1" />
      <circle cx="16" cy="16" r="1" />
    </>
  ),
  cube: (
    <>
      <path d="m12 3.5 7.36 4.25v8.5L12 20.5l-7.36-4.25v-8.5z" />
      <path d="M4.64 7.75 12 12l7.36-4.25M12 12v8.5" />
      <path d="M4.64 11 12 15.25l7.36-4.25" />
    </>
  ),
  shapes: (
    <>
      <circle cx="7.75" cy="7.75" r="3.75" />
      <rect x="12.5" y="12.5" width="7.5" height="7.5" />
    </>
  ),
  castle: (
    <>
      <path d="M10 20H5V3.5h2.5v3h3.25v-3h2.5v3h3.25v-3H19V20h-5" />
      <path d="M10 20v-3a2 2 0 0 1 4 0v3" />
    </>
  ),
  letters: (
    <>
      <path d="m3.5 19 4-14 4 14M4.9 14.5h5.2" />
      <circle cx="16.5" cy="16" r="3" />
      <path d="M19.5 13v6" />
    </>
  ),
  ghost: (
    <>
      <path d="M5.5 20v-9a6.5 6.5 0 0 1 13 0v9l-2.17-1.5-2.16 1.5L12 18.5 9.83 20l-2.16-1.5z" />
      <circle cx="9.5" cy="11" r="1" />
      <circle cx="14.5" cy="11" r="1" />
    </>
  ),
  sunHalf: (
    <>
      <circle cx="9.75" cy="12" r="4.5" />
      <path d="M9.75 7.5v9" />
      <path d="M16.5 12h2.25M15.6 8.63l1.94-1.13M15.6 15.38l1.94 1.12M13.13 6.15l1.12-1.94M13.13 17.85l1.12 1.94" />
    </>
  ),
  cloud: (
    <>
      <path d="M7 18.5a4 4 0 0 1 0-8 5 5 0 0 1 10 0 4 4 0 0 1 0 8z" />
    </>
  ),
  checklist: (
    <>
      <path d="M9 4.5H6.5A1.5 1.5 0 0 0 5 6v13a1.5 1.5 0 0 0 1.5 1.5h11A1.5 1.5 0 0 0 19 19V6a1.5 1.5 0 0 0-1.5-1.5H15" />
      <rect x="9" y="3" width="6" height="3" rx="1" />
      <path d="m8 11.5 1.25 1.25 2.5-2.5M8 16.5l1.25 1.25 2.5-2.5M14.5 11.5h2M14.5 16.5h2" />
    </>
  ),
  flame: (
    <>
      <path d="M6 14.5a6 6 0 0 0 12 0c0-3-1.3-5-2.8-6.8-1.3-1.6-1.8-2.7-1.7-4.2-2.5 1.5-3.5 4-3.2 6.5-1.1-.6-1.7-1.4-1.9-2.4C7 9.3 6 11.6 6 14.5z" />
      <path d="M12.5 9.45c-.2 1.3-.9 2-1.6 2.7-.6.6-1.4 1.4-1.4 2.6a2.5 2.5 0 0 0 5 0c0-1.4-.8-2.5-1.3-3.3-.4-.6-.7-1.2-.7-2z" />
    </>
  ),
  boxes: (
    <>
      <path d="M8 12.5V5h8v7.5" />
      <path d="M4 12.5h16V20H4zM12 12.5V20" />
    </>
  ),
  feather: (
    <>
      <path d="M8 16C7.25 13.5 7.63 10.88 9.44 8.69L11.42 9.82 10.7 7.43C12.62 5.84 15.5 4.6 19.5 4c-.3 3.5-1.4 6-3.3 8.1l-2.6-.4 1.3 2.1C13.3 15 10.9 16 8 16z" />
      <path d="M4.5 19.5 14.5 9.5" />
    </>
  ),
  island: (
    <>
      <path d="M3.5 12.5h17L18 16l-3 .5-2.5 4.5-2-4-4-.5z" />
      <path d="M15.5 12.5V10" />
      <path d="M13.25 10a2.25 2.25 0 0 1-.25-4.48 2.5 2.5 0 0 1 5 0 2.25 2.25 0 0 1-.25 4.48z" />
    </>
  ),
  tent: (
    <>
      <path d="M3 20h18" />
      <path d="M4 20 13.24 3.83M20 20 10.76 3.83" />
      <path d="m8.5 20 3.5-6.5 3.5 6.5" />
    </>
  ),
  house: (
    <>
      <path d="M3.5 12.5 12 4.5l8.5 8" />
      <path d="M6 10.2V20h12v-9.8" />
      <path d="M10 20v-5h4v5" />
      <path d="M14.5 6.85V4.5H17v4.7" />
    </>
  ),
  gamepad: (
    <>
      <rect x="3" y="6.5" width="18" height="11" rx="5.5" />
      <path d="M8 9.75v4.5M5.75 12h4.5" />
      <circle cx="14.5" cy="13.75" r="0.75" />
      <circle cx="17.5" cy="10.25" r="0.75" />
    </>
  ),
  jump: (
    <>
      <rect x="3.5" y="16.5" width="3.5" height="3.5" />
      <rect x="10.25" y="13" width="3.5" height="3.5" />
      <rect x="17" y="9.5" width="3.5" height="3.5" />
      <path d="M5.25 13.5Q9 2.5 18.5 7" />
      <path d="M17.46 4.73 18.5 7l-2.42.63" />
    </>
  ),
  bulb: (
    <>
      <path d="M9.5 15c0-.88-1-1.17-1.1-1.77a5.75 5.75 0 1 1 7.2 0c-.1.6-1.1.89-1.1 1.77z" />
      <path d="M9.5 18h5M10.5 21h3" />
    </>
  ),
  animated: (
    <>
      <rect x="7.5" y="5.5" width="13" height="13" rx="2.5" />
      <path d="M12 9v6l5-3z" />
      <path d="M3.5 10h2M3.5 14h2" />
    </>
  ),
} satisfies Record<string, ReactElement>;

export type IconName = keyof typeof paths;

interface IconProps {
  name: IconName;
  /** Pixels; the icon is square. */
  size?: number;
  /** An arrow that points along the text: turned round in a right-to-left language. */
  mirror?: boolean;
  className?: string | undefined;
}

/** A decorative icon: always hidden from assistive technology, the text next to it names the action. */
export function Icon({ name, size = 20, mirror = false, className }: IconProps) {
  return (
    <svg
      className={[styles.icon, mirror && styles.mirror, className].filter(Boolean).join(' ')}
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
