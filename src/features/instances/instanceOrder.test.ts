import { describe, expect, it } from 'vitest';
import type { InstanceSummary, LoaderKind } from '../../lib/contracts';
import { instanceFixture } from '../../test/fixtures';
import {
  compareVersions,
  normalizeQuery,
  orderInstances,
  parseVersion,
  QUERY_MAX_LENGTH,
  type OrderOptions,
} from './instanceOrder';

const loaderNames: Record<LoaderKind, string> = {
  fabric: 'Fabric',
  quilt: 'Quilt',
  forge: 'Forge',
  neoforge: 'NeoForge',
  optifine: 'OptiFine',
};

let next = 0;
function make(overrides: Partial<InstanceSummary>): InstanceSummary {
  next += 1;
  return instanceFixture({ id: `id-${String(next).padStart(3, '0')}`, ...overrides });
}

function names(list: InstanceSummary[]): string[] {
  return list.map((i) => i.displayName);
}

const options = (overrides: Partial<OrderOptions> = {}): OrderOptions => ({
  query: '',
  sort: 'name',
  locale: 'en',
  loaderName: (kind) => loaderNames[kind],
  ...overrides,
});

const text = new Intl.Collator('en', { numeric: true }).compare;

describe('orderInstances', () => {
  const survival = make({ displayName: 'Survival', versionId: '1.21.1' });
  const fabric = make({
    displayName: 'Shaders',
    versionId: '1.21.1',
    loader: { kind: 'fabric', version: '0.16.9' },
  });
  const old = make({
    displayName: 'Old server',
    versionId: '1.12.2',
    loader: { kind: 'forge', version: '14.23.5.2860' },
  });
  const russian = make({ displayName: 'Ёлка и Творчество', versionId: '26.3' });
  const arabic = make({ displayName: 'البقاء مع الأصدقاء', versionId: '1.20.1' });
  const all = [survival, fabric, old, russian, arabic];

  it('finds by name, version and loader name, ignoring case and spaces around', () => {
    expect(names(orderInstances(all, options({ query: '  SURV ' })))).toEqual(['Survival']);
    expect(names(orderInstances(all, options({ query: '1.21.1' })))).toEqual([
      'Shaders',
      'Survival',
    ]);
    expect(names(orderInstances(all, options({ query: 'fabric' })))).toEqual(['Shaders']);
    expect(names(orderInstances(all, options({ query: 'Forge' })))).toEqual(['Old server']);
  });

  it('finds Cyrillic and Arabic names without minding case, ё or hamza', () => {
    expect(names(orderInstances(all, options({ query: 'елка' })))).toEqual(['Ёлка и Творчество']);
    expect(names(orderInstances(all, options({ query: 'ТВОРЧЕСТВО' })))).toEqual([
      'Ёлка и Творчество',
    ]);
    expect(names(orderInstances(all, options({ query: 'الاصدقاء' })))).toEqual([
      'البقاء مع الأصدقاء',
    ]);
  });

  it('shows everything for an empty search and nothing for junk', () => {
    expect(orderInstances(all, options({ query: '' }))).toHaveLength(all.length);
    expect(orderInstances(all, options({ query: ' \t\n ' }))).toHaveLength(all.length);
    for (const junk of [
      '<script>alert(1)</script>',
      '.*',
      '(',
      '%s%n',
      '\u0000',
      '🙂',
      'a'.repeat(5000),
    ]) {
      expect(orderInstances(all, options({ query: junk }))).toEqual([]);
    }
  });

  it('cuts a very long search to the field length', () => {
    expect(normalizeQuery('x'.repeat(100_000))).toHaveLength(QUERY_MAX_LENGTH);
    expect(normalizeQuery(`   ${'y'.repeat(70)}`)).toHaveLength(QUERY_MAX_LENGTH);
    const long = make({ displayName: 'z'.repeat(64), versionId: '1.21.1' });
    expect(names(orderInstances([long], options({ query: 'z'.repeat(500) })))).toEqual([
      'z'.repeat(64),
    ]);
  });

  it('orders by name with numbers and the interface language', () => {
    const packs = [
      make({ displayName: 'pack 10' }),
      make({ displayName: 'Pack 2' }),
      make({ displayName: 'Яблоко' }),
      make({ displayName: 'Арбуз' }),
    ];
    expect(names(orderInstances(packs, options({ sort: 'name', locale: 'ru' })))).toEqual([
      'Арбуз',
      'Яблоко',
      'Pack 2',
      'pack 10',
    ]);
    // A broken locale tag still orders the list.
    expect(
      orderInstances(packs, options({ sort: 'name', locale: '!!not a locale!!' })),
    ).toHaveLength(4);
  });

  it('orders by the last game, then by when the installation was made', () => {
    const list = [
      make({ displayName: 'Never, old', createdAt: '2026-01-01T00:00:00Z' }),
      make({ displayName: 'Played long ago', lastPlayedAt: '2026-02-01T00:00:00Z' }),
      make({ displayName: 'Never, new', createdAt: '2026-09-01T00:00:00Z' }),
      make({ displayName: 'Played today', lastPlayedAt: '2026-10-22T08:00:00Z' }),
      make({ displayName: 'Broken date', lastPlayedAt: 'not a date', createdAt: 'nope' }),
    ];
    expect(names(orderInstances(list, options({ sort: 'recent' })))).toEqual([
      'Played today',
      'Played long ago',
      'Never, new',
      'Never, old',
      'Broken date',
    ]);
  });

  it('orders by game version, newest first', () => {
    const versions = [
      '1.21.1',
      'b1.7.3',
      '26.3',
      'junk!',
      '1.20.1',
      '26.4-snapshot-4',
      '1.21.10',
      '26.3-rc1',
      '24w14a',
      '1.21',
      '',
      '26.4-snapshot-10',
    ];
    const list = versions.map((versionId) => make({ displayName: versionId, versionId }));
    expect(names(orderInstances(list, options({ sort: 'version' })))).toEqual([
      '26.4-snapshot-10',
      '26.4-snapshot-4',
      '26.3',
      '26.3-rc1',
      '1.21.10',
      '1.21.1',
      '1.21',
      '1.20.1',
      '24w14a',
      'b1.7.3',
      '',
      'junk!',
    ]);
  });

  it('never changes the list it is given', () => {
    const list = [make({ displayName: 'B' }), make({ displayName: 'A' })];
    const copy = [...list];
    orderInstances(list, options({ sort: 'name' }));
    expect(list).toEqual(copy);
  });
});

describe('parseVersion', () => {
  it('reads releases, snapshots and old versions', () => {
    expect(parseVersion('1.21.1')).toEqual({ era: 6, numbers: [1, 21, 1], suffix: '' });
    expect(parseVersion('26.4-snapshot-4')?.suffix).toBe('-snapshot-4');
    expect(parseVersion('b1.7.3')?.numbers).toEqual([1, 7, 3]);
    expect(parseVersion('rd-132211')?.era).toBe(0);
    expect(parseVersion('Name')).toBeNull();
    expect(parseVersion('')).toBeNull();
  });

  it('puts a release before its snapshots and pre-releases', () => {
    expect(compareVersions('1.21', '1.21-pre1', text)).toBeLessThan(0);
    expect(compareVersions('1.21-pre1', '1.21', text)).toBeGreaterThan(0);
    expect(compareVersions('1.21.0', '1.21', text)).toBe(0);
    expect(compareVersions('???', '1.0', text)).toBeGreaterThan(0);
  });
});
