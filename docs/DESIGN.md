# Interface design: concept A «Верстак»

The launcher's interface follows concept A, chosen by the owner on 2026-10-22
([D-072](DECISIONS.md#d-072--interface-concept-a)). This file is the brief for every
UI change. The design skills in `.claude/skills/` help with the work, and this file wins
where they disagree. The old look (glass, gradients, glows, text in capitals) is replaced
screen by screen; new code never adds it back.

## Character

A calm, precise tool for getting into the game: a workbench, not a poster. The launcher is
an app people operate, so clarity, density that stays readable, and familiar patterns come
before expression. The brand lives in the details: the deep green rail, a mint mark on the
current item, skin heads as avatars, Minecraft art only where it is content (skins,
screenshots, mod icons).

- Green palette, paper ground, white surfaces. No gradients, glows, blur or glass.
- No text in capitals, no letter spacing tricks. Sentence case everywhere.
- Stroke icons (`src/components/Icon.tsx`), one weight, `currentColor`.
- One accent action per screen area. Play is the strongest action in the app.
- English, Russian and Arabic: every layout works right to left.

## Shell

- **Left rail** (`--rail-width`, `--color-rail`), the page's `header`: brand; search
  "Find · Ctrl K" (command palette); the main nav with icons (Feed, Installations, Mods,
  Worlds, Skins, Accounts, Settings); then Friends, Downloads, More; update notice; the
  player at the bottom (skin head, name, "plays by default", Quit). The current item has a
  mint mark at its inline start and `--color-rail-active`.
- **Content** scrolls as the page; the rail stays put.
- **Launch bar** at the bottom (`--bar-height`, `--color-bar`): the chosen installation
  (tile, name, version and state), "Play as", and the large Play. It shows install,
  repair, progress or "game running" in place of Play when that is the state.
- Side sheets (Friends) open from the inline end; menus from the rail open beside it.
- Minimum window 820 × 560. Below about 1040 px wide the rail may fold to icons.

## Tokens

All values live in `src/styles/tokens.css`; components never write a color, size or
shadow of their own. Light and dark themes share names; dark follows the system unless the
player picks a theme.

| Role           | Light     | Dark      | Token                    |
| -------------- | --------- | --------- | ------------------------ |
| Ground         | `#f3f5f0` | `#0f1713` | `--color-bg`             |
| Surface        | `#ffffff` | `#16211b` | `--color-surface`        |
| Sunken         | `#eef2ec` | `#121c17` | `--color-surface-sunken` |
| Border         | `#d6ded8` | `#263a2f` | `--color-border`         |
| Text           | `#15231c` | `#e6efe9` | `--color-text`           |
| Muted text     | `#5b6b61` | `#9db3a6` | `--color-text-muted`     |
| Accent         | `#1e7a4c` | `#3dd68c` | `--color-accent`         |
| Accent as text | `#145a36` | `#8fd9b4` | `--color-accent-text`    |
| Mint mark      | `#3dd68c` | `#3dd68c` | `--color-mint`           |
| Rail           | `#123b2a` | `#0b2219` | `--color-rail`           |
| Rail, current  | `#1e5a40` | `#1e4a35` | `--color-rail-active`    |
| Focus ring     | `#0b6bcb` | `#8fd3ff` | `--color-focus`          |

Status colors come in pairs (`--color-*-bg` with `--color-*-text`) for success, warning,
danger, info and neutral. Radii are 8, 10 and 12 px plus pills; spacing is the 4 px scale
`--space-1` … `--space-8`; controls are `--control-height` (36 px) high, the large Play
48 px.

## Type

Golos Text (Latin and Cyrillic) and Noto Sans Arabic, both variable and bundled with
the app through `@fontsource-variable` (the CSP is `font-src 'self'`: never a font CDN).
Sizes: 12, 14, 15 (body), 18, 22, 28 px (`--font-size-*`). Weights 400, 500, 600 and 700
for Play and page titles. Numbers that change (progress, sizes, counts) use tabular
figures.

## Components

- **Buttons:** primary (accent fill), secondary (surface with border), ghost (text);
  `large` only for Play and the one main action of a page. No transforms or glows on
  hover; the color changes.
- **Choices of a few options** (loader, sort, view) are a row of pills, not a select.
- **Lists and cards** sit on white surfaces with a 1 px border; a hovered row takes
  `--color-hover`, a chosen one `--color-selected` and an accent mark.
- **Avatars** are the 8 × 8 face of the skin with its hat layer (`SkinHead`), pixelated,
  20 % rounded. Without a skin, the default face.
- **Icons** come from `Icon`; add a new one there in the same 24 px stroke style.
- **Empty and error states** say what happened and offer the next step.

## Screens (owner's choices, 2026-10-22)

- **Feed:** variant 2 "mosaic": three columns of cards of different heights, vertical
  Shorts, picture, title and source.
- **Installations:** cards of installations with the chosen one in the launch bar; many
  installations get filter, sort and search. New installation: game version, loader as a
  row of pills, then its version.
- **Mods catalog:** variant 1 "tiles", plus collections (adapted from variant 3) and a
  list of categories with icons (from variant 2, the categories of Modrinth and
  CurseForge) in the free column. Infinite scroll that remembers its place; "My
  requests · Add to catalog" at the top end.
- **Mod page:** variant 2 "sticky card": the project card stays at the inline start
  while the description scrolls; the versions panel scrolls inside itself.
- **Command palette** (Ctrl K): pages, installations, mods and actions.

## Accessibility and right to left

- Every control works from the keyboard and shows a 2 px `--color-focus` ring.
- Text contrast at least 4.5:1, large text and icons 3:1, in both themes.
- Logical CSS only (`margin-inline-start`, `inset-inline-end`, `text-align: start`);
  icons that point (back, next, chevrons) mirror in Arabic.
- Motion is short (`--motion-duration`) and stops under `prefers-reduced-motion`.
- Icon-only buttons have an accessible name and a tooltip.

## Checking a change

Look at every UI change in light and dark, in Arabic, and at the 820 px minimum width,
and check window changes in a Windows package (CLAUDE.md). The design skills:
`frontend-design` (direction), `impeccable` (critique, audit, polish, harden; its script
engine is not committed) and `ui-ux-pro-max` (UX rules search,
`python .claude/skills/ui-ux-pro-max/scripts/search.py "<query>" --domain ux` or
`--stack react`).
