import { describe, expect, it } from 'vitest';
import type { ContentItem, ModsHit } from '../lib/contracts';
import { instanceFixture, snapshotFixture } from '../test/fixtures';
import { LOADER_KINDS } from '../features/instances/loaderLabel';
import { menuViews, rowViews } from './navigation';
import {
  MAX_QUERY,
  type PaletteEntry,
  catalogEntries,
  highlightParts,
  holdsMods,
  installationOrder,
  installedModEntries,
  isEmptyQuery,
  matchEntry,
  matchText,
  normalizeQuery,
  pageEntries,
  rankEntries,
} from './palette';

/** The matched part of `text`. */
function matched(text: string, query: string): string | null {
  const found = matchText(text, query);
  return found === null ? null : text.slice(found.range[0], found.range[1]);
}

function entry(label: string, detail = ''): PaletteEntry {
  return {
    key: label,
    label,
    detail,
    action: 'Go',
    lead: { kind: 'icon', icon: 'settings' },
    target: { kind: 'view', view: 'settings' },
  };
}

function contentItem(overrides: Partial<ContentItem>): ContentItem {
  return {
    path: 'mods/sodium.jar',
    folder: 'mods',
    name: 'sodium.jar',
    world: null,
    enabled: true,
    size: 1,
    directory: false,
    projectId: null,
    versionId: null,
    title: 'Sodium',
    versionNumber: null,
    icon: null,
    dependency: false,
    fromPack: false,
    ...overrides,
  };
}

function hit(overrides: Partial<ModsHit>): ModsHit {
  return {
    id: 'p-1',
    kind: 'mod',
    slug: 'fabric-api',
    title: 'Fabric API',
    summary: '',
    icon: null,
    downloads: 28_400_000,
    status: 'ready',
    updated: 0,
    author: null,
    sources: ['modrinth'],
    ...overrides,
  };
}

describe('palette matching', () => {
  it('ranks the start of a name, then the start of a word, then the inside', () => {
    expect(matchText('Fabric 1.21.1', 'fab')).toEqual({ rank: 0, range: [0, 3] });
    expect(matchText('Sodium for Fabric', 'fab')).toEqual({ rank: 1, range: [11, 14] });
    expect(matchText('Prefabricated', 'fab')).toEqual({ rank: 2, range: [3, 6] });
    // A word's start wins over an earlier match inside a word.
    expect(matchText('Prefab Fabric', 'fab')).toEqual({ rank: 1, range: [7, 10] });
    expect(matched('FABRIC', 'fAb')).toBe('FAB');
    expect(matchText('Forge', 'fab')).toBeNull();
  });

  it('matches Cyrillic regardless of case, and ё as е', () => {
    expect(matchText('Выживание с друзьями', 'ДРУЗ')).toEqual({ rank: 1, range: [12, 16] });
    expect(matched('Ёлка', 'елк')).toBe('Ёлк');
    expect(matched('Сервер друзей', 'сер')).toBe('Сер');
  });

  it('matches Arabic without splitting a letter from its marks', () => {
    expect(matchText('عالم البناء', 'البناء')).toEqual({ rank: 1, range: [5, 11] });
    // Hamza and short vowels are not needed to find a word.
    expect(matched('أحمد', 'احمد')).toBe('أحمد');
    const voweled = 'عَالَم';
    const found = matchText(voweled, 'عال');
    expect(found).not.toBeNull();
    // The fatha on the last matched letter stays inside the highlight.
    expect(found?.range).toEqual([0, 5]);
    expect(voweled.slice(0, 5).endsWith('\u064e')).toBe(true);
    // Tatweel only stretches a word.
    expect(matched('عـــالم', 'عالم')).toBe('عـــالم');
  });

  it("keeps positions right when lower case changes the length ('İ')", () => {
    expect('İ'.toLowerCase()).toHaveLength(2);
    expect(matchText('İstanbul server', 'ist')).toEqual({ rank: 0, range: [0, 3] });
    expect(matched('Mİnecraft İstanbul', 'stan')).toBe('stan');
    expect(matched('Mİnecraft İstanbul', 'ser')).toBeNull();
    expect(matched('istanbul', 'İST')).toBe('ist');
    expect(matched('Server İİİ end', 'end')).toBe('end');
  });

  it('takes regular expression characters as plain text', () => {
    expect(matched('Mod (beta) [1.0] *.jar', '(beta)')).toBe('(beta)');
    expect(matched('a.b', '.')).toBe('.');
    expect(matchText('abc', '(.*[')).toBeNull();
    expect(matched('x (.*[ y', '(.*[')).toBe('(.*[');
    expect(matched('C:\\games\\$HOME', '\\$h')).toBe('\\$H');
  });

  it('cuts long queries and stays fast on long texts', () => {
    expect(normalizeQuery('a'.repeat(500))).toHaveLength(MAX_QUERY);
    expect(Array.from(normalizeQuery('🙂'.repeat(300)))).toHaveLength(MAX_QUERY);
    expect(matchText('a'.repeat(1000), 'a'.repeat(300))).toEqual({ rank: 0, range: [0, 200] });
    const started = performance.now();
    expect(matchText(`b${'a'.repeat(20_000)}`, `${'a'.repeat(199)}c`)).toBeNull();
    expect(performance.now() - started).toBeLessThan(1000);
  });

  it('drops control and direction characters from the query', () => {
    expect(normalizeQuery('\u202efab\u202c')).toBe('fab');
    expect(normalizeQuery('fa\u0000bric\n\tapi')).toBe('fa bric api');
    expect(normalizeQuery('<script>alert(1)</script>')).toBe('<script>alert(1)</script>');
    expect(matched('Fab\u200dric', 'fabric')).toBe('Fab\u200dric');
    expect(matched('Fabric API', 'fabric\u0007')).toBe('Fabric');
  });

  it('asks for nothing with an empty or blank query', () => {
    expect(normalizeQuery('   \t\n ')).toBe('');
    expect(isEmptyQuery('')).toBe(true);
    expect(isEmptyQuery('  \u00a0 ')).toBe(true);
    expect(isEmptyQuery('\u0301')).toBe(true);
    expect(isEmptyQuery(' a ')).toBe(false);
    expect(matchEntry('Survival', 'Ready', '   ')).toEqual({ rank: 0, label: [], detail: [] });
    const entries = ['A', 'B', 'C'].map((label) => entry(label));
    expect(rankEntries(entries, ' ', 2).map((row) => row.entry.label)).toEqual(['A', 'B']);
  });

  it('matches the second line after the name, then every word anywhere', () => {
    const detail = 'Minecraft 1.21.1 · Fabric 0.16.9 · Ready';
    expect(matchEntry('Survival', detail, 'fab')).toEqual({
      rank: 4,
      label: [],
      detail: [[19, 22]],
    });
    expect(matchEntry('Survival', detail, 'surv fabric')).toEqual({
      rank: 6,
      label: [[0, 4]],
      detail: [[19, 25]],
    });
    expect(matchEntry('Survival', detail, 'surv forge')).toBeNull();
  });

  it('ranks rows and keeps their order within a rank', () => {
    const rows = [
      entry('Prefabricated'),
      entry('Survival', 'Fabric 0.16.9'),
      entry('Sodium for Fabric'),
      entry('Fabric 1.21.1'),
      entry('Fabric API'),
      entry('Forge'),
    ];
    expect(rankEntries(rows, 'fab', 10).map((row) => row.entry.label)).toEqual([
      'Fabric 1.21.1',
      'Fabric API',
      'Sodium for Fabric',
      'Prefabricated',
      'Survival',
    ]);
    expect(rankEntries(rows, 'fab', 2)).toHaveLength(2);
  });

  it('cuts a text into highlighted parts', () => {
    expect(highlightParts('Fabric API', [[0, 3]])).toEqual([
      { text: 'Fab', mark: true },
      { text: 'ric API', mark: false },
    ]);
    expect(
      highlightParts('abc', [
        [2, 10],
        [1, 2],
      ]),
    ).toEqual([
      { text: 'a', mark: false },
      { text: 'bc', mark: true },
    ]);
    expect(highlightParts('abc', [])).toEqual([{ text: 'abc', mark: false }]);
  });
});

describe('palette sources', () => {
  const settings = snapshotFixture().settings;

  it('lists every screen, the console, new installations and sections', () => {
    const keys = pageEntries(settings).map((page) => page.key);
    for (const view of [...rowViews, ...menuViews]) expect(keys).toContain(`view:${view}`);
    expect(keys).toContain('console');
    expect(keys).toContain('new');
    const loaders = pageEntries(settings).filter((page) => page.key.startsWith('new:'));
    expect(loaders.map((page) => page.target)).toEqual(
      LOADER_KINDS.map((loader) => ({ kind: 'newInstallation', loader })),
    );
    expect(loaders.map((page) => page.label)).toContain('New installation with Fabric');
    expect(keys.filter((key) => key.startsWith('settings:'))).toHaveLength(8);
    expect(keys.filter((key) => key.startsWith('mods:'))).toHaveLength(8);
    expect(new Set(keys).size).toBe(keys.length);
    const console = pageEntries(settings).find((page) => page.key === 'console');
    expect(console).toMatchObject({ label: 'Open console', detail: 'Diagnostics' });
  });

  it('leaves the feed out while it is switched off', () => {
    const off = { ...settings, feed: { ...settings.feed, enabled: false } };
    expect(pageEntries(off).map((page) => page.key)).not.toContain('view:feed');
    expect(pageEntries(settings).map((page) => page.key)).toContain('view:feed');
  });

  it('puts the chosen installation first, then the last played', () => {
    const a = instanceFixture({ id: 'a', displayName: 'A' });
    const b = instanceFixture({ id: 'b', displayName: 'B', lastPlayedAt: '2026-10-01T10:00:00Z' });
    const c = instanceFixture({ id: 'c', displayName: 'C', lastPlayedAt: '2026-10-20T10:00:00Z' });
    const d = instanceFixture({ id: 'd', displayName: 'D', lastPlayedAt: 'not a date' });
    const snapshot = snapshotFixture({
      instances: [a, b, c, d],
      settings: { ...settings, selectedInstanceId: 'b' },
    });
    expect(installationOrder(snapshot).map((i) => i.id)).toEqual(['b', 'c', 'a', 'd']);
  });

  it('reads mods only of installations with a loader that runs them', () => {
    expect(holdsMods(instanceFixture())).toBe(false);
    expect(holdsMods(instanceFixture({ loader: { kind: 'optifine', version: 'x' } }))).toBe(false);
    expect(holdsMods(instanceFixture({ loader: { kind: 'fabric', version: '0.16.9' } }))).toBe(
      true,
    );
  });

  it('lists installed mods with their installation, and catalog hits not installed', () => {
    const instance = instanceFixture({ displayName: 'Fabric world' });
    const rows = installedModEntries(
      [instance],
      new Map([
        [
          instance.id,
          [
            contentItem({}),
            contentItem({ path: 'mods/lib.jar', title: null, name: 'lib.jar', dependency: true }),
            contentItem({ path: 'resourcepacks/x.zip', folder: 'resourcepacks', title: 'Pack' }),
          ],
        ],
      ]),
    );
    expect(rows.map((row) => [row.label, row.detail])).toEqual([
      ['Sodium', 'Installed in “Fabric world”'],
      ['lib.jar', 'Installed in “Fabric world” · Needed by another mod'],
    ]);

    const hits = [
      hit({}),
      hit({ id: 'p-2', title: '', slug: 'kotlin', icon: 'https://files-mods.lapi.sh/k.png' }),
      hit({ id: 'p-3', title: 'Evil', icon: 'https://example.com/x.png' }),
      // A repeated project shows once.
      hit({ id: 'p-2', title: 'Kotlin again' }),
    ];
    const found = catalogEntries(hits, new Set(['p-1']));
    expect(found.map((row) => row.label)).toEqual(['kotlin', 'Evil']);
    expect(found[0]?.lead).toEqual({
      kind: 'picture',
      url: 'https://files-mods.lapi.sh/k.png',
      letter: 'k',
    });
    // Pictures come only from the catalog's own host.
    expect(found[1]?.lead).toEqual({ kind: 'picture', url: null, letter: 'E' });
    expect(found[1]?.detail).toBe('In the catalog · 28.4M downloads');
  });
});
