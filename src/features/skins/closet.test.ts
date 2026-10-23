import { describe, expect, it } from 'vitest';
import type {
  Closet,
  PickedTexture,
  WardrobeItem,
  WardrobeSet,
  WornTexture,
} from '../../lib/contracts';
import { pickedOf } from './draft';
import {
  chipsOf,
  closetOf,
  displayName,
  firstChars,
  isTried,
  nameFromFile,
  room,
  sortedEntries,
  toggleItem,
  toggleSet,
  trialChanges,
  trialPieces,
  uploadsLine,
  wardrobeName,
} from './closet';

const id = (n: number) => n.toString(16).padStart(32, '0');
const hash = (n: number) => n.toString(16).padStart(64, '0');
const PNG = 'data:image/png;base64,iVBORw0KGgo=';
/** A character by its code, so the test names invisible ones without holding them. */
const char = (code: number) => String.fromCodePoint(code);

function texture(n: number, overrides: Partial<WornTexture> = {}): WornTexture {
  return {
    hash: hash(n),
    slim: false,
    width: 64,
    height: 64,
    frames: 1,
    frameMs: [],
    ...overrides,
  };
}

function item(n: number, overrides: Partial<WardrobeItem> = {}): WardrobeItem {
  return {
    id: id(n),
    kind: 'skin',
    name: `Skin ${String(n)}`,
    favourite: false,
    created: 1_700_000_000 + n,
    worn: 0,
    texture: texture(n),
    ...overrides,
  };
}

function set(n: number, overrides: Partial<WardrobeSet> = {}): WardrobeSet {
  return {
    id: id(100 + n),
    name: `Set ${String(n)}`,
    favourite: false,
    created: 1_700_000_000 + n,
    worn: 0,
    skin: texture(n),
    cape: texture(50 + n, { height: 32 }),
    ...overrides,
  };
}

function closet(overrides: Partial<Closet> = {}): Closet {
  return {
    worn: { skin: null, cape: null, uploads: null },
    items: [],
    sets: [],
    limits: { skins: 30, capes: 10, sets: 20 },
    ...overrides,
  };
}

function picked(overrides: Record<string, unknown> = {}): unknown {
  return {
    kind: 'skin',
    name: 'steve.png',
    width: 64,
    height: 256,
    frames: 4,
    frameMs: [],
    source: 'png',
    notes: [],
    bytes: 3000,
    preview: PNG,
    slim: null,
    ...overrides,
  } satisfies Record<keyof PickedTexture, unknown>;
}

describe('the wardrobe as the core answers it', () => {
  it('reads a wardrobe and leaves out entries that are not the core’s', () => {
    const read = closetOf({
      worn: { skin: texture(1), cape: null, uploads: { left: 7, resetSeconds: 999_999 } },
      items: [
        item(1),
        item(2, { id: 'not-an-id' }),
        item(3, { name: 'x'.repeat(33) }),
        item(4, { texture: texture(4, { hash: 'javascript:alert(1)' }) }),
        item(5, { kind: 'elytra' as 'skin' }),
        item(6, { created: -1 }),
        item(7, { texture: texture(7, { frames: 33 }) }),
        item(8, { texture: texture(8, { width: 96 }) }),
        null,
        'item',
      ],
      sets: [set(1), set(2, { skin: null, cape: null }), set(3, { cape: { hash: 1 } as never })],
      limits: { skins: 30, capes: 10, sets: 20 },
    });
    expect(read?.items.map((i) => i.id)).toEqual([id(1)]);
    expect(read?.sets.map((s) => s.id)).toEqual([id(101)]);
    // A reset further than a day is not believed.
    expect(read?.worn.uploads).toEqual({ left: 7, resetSeconds: 24 * 3600 });
  });

  it('is not a wardrobe without its parts, or with limits past the rule', () => {
    for (const junk of [
      null,
      undefined,
      'closet',
      [],
      {},
      { ...closet(), items: 'x' },
      { ...closet(), sets: {} },
      { ...closet(), worn: null },
      { ...closet(), worn: { skin: { hash: 'x' }, cape: null } },
      { ...closet(), limits: { skins: 30, capes: 10 } },
      { ...closet(), limits: { skins: 1e9, capes: 10, sets: 20 } },
      { ...closet(), limits: { skins: -1, capes: 10, sets: 20 } },
      { ...closet(), limits: { skins: 1.5, capes: 10, sets: 20 } },
    ]) {
      expect(closetOf(junk)).toBeNull();
    }
    // Uploads the core could not tell are unknown, not wrong.
    expect(
      closetOf({ ...closet(), worn: { skin: null, cape: null, uploads: { left: 'x' } } })?.worn
        .uploads,
    ).toBeNull();
  });

  it('keeps no more than the core passes on', () => {
    const many = Array.from({ length: 200 }, (_, n) => item(n + 1));
    expect(closetOf(closet({ items: many }))?.items).toHaveLength(128);
    const sets = Array.from({ length: 200 }, (_, n) => set(n + 1));
    expect(closetOf(closet({ sets }))?.sets).toHaveLength(128);
  });

  it('leaves out entries with times or frames past the rule', () => {
    const read = closetOf(
      closet({
        items: [
          item(1, { worn: -1 }),
          item(2, { worn: 1.5 }),
          item(3, { created: Number.MAX_SAFE_INTEGER + 2 }),
          item(4, { texture: texture(4, { frameMs: Array.from({ length: 33 }, () => 100) }) }),
          item(5, { texture: texture(5, { frameMs: [100, Number.POSITIVE_INFINITY] }) }),
          item(6, { favourite: 'yes' as unknown as boolean }),
          item(7),
        ],
        sets: [set(1, { worn: -5 }), set(2, { name: 7 as unknown as string }), set(3)],
      }),
    );
    expect(read?.items.map((i) => i.id)).toEqual([id(7)]);
    expect(read?.sets.map((s) => s.id)).toEqual([id(103)]);
  });

  it('does not believe uploads left past the rule', () => {
    for (const left of [-1, 1e9, 1.5, '7']) {
      expect(
        closetOf({
          ...closet(),
          worn: { skin: null, cape: null, uploads: { left, resetSeconds: 5 } },
        })?.worn.uploads,
      ).toBeNull();
    }
  });
});

describe('names', () => {
  it('takes up to 32 characters, counted as the service counts them', () => {
    expect(wardrobeName('  Neon  ')).toBe('Neon');
    expect(wardrobeName('')).toBe('');
    expect(wardrobeName('я'.repeat(32))).toBe('я'.repeat(32));
    expect(wardrobeName('я'.repeat(33))).toBeNull();
    // An emoji is one character, though two UTF-16 units.
    expect(wardrobeName('😀'.repeat(32))).toBe('😀'.repeat(32));
    expect(firstChars('😀😀😀', 2)).toBe('😀😀');
  });

  it('refuses control, invisible and direction characters', () => {
    for (const code of [0x00, 0x0a, 0x1b, 0x7f, 0x85, 0x200b, 0x200e, 0x202e, 0x2066, 0xfeff]) {
      expect(wardrobeName(`a${char(code)}b`)).toBeNull();
    }
    // Markup is a name like any other: it is shown as text.
    expect(wardrobeName('<img src=x onerror=alert(1)>')).toBe('<img src=x onerror=alert(1)>');
  });

  it('names a file by its name without the ending, cleaned', () => {
    expect(nameFromFile('neon_walk.gif')).toBe('neon_walk');
    expect(nameFromFile('Steve.PNG')).toBe('Steve');
    expect(nameFromFile(`${char(0x202e)}gnp.exe.png`)).toBe('gnp.exe');
    expect(nameFromFile(`${'a'.repeat(40)}.png`)).toBe('a'.repeat(32));
    expect(nameFromFile('.png')).toBe('');
  });

  it('gives an unnamed entry the kind and the day it was kept', () => {
    expect(displayName(item(1, { name: '' }))).toMatch(/^Skin of /);
    expect(displayName(item(1, { name: '   ', kind: 'cape' }))).toMatch(/^Cape of /);
    expect(displayName(set(1, { name: '' }))).toMatch(/^Set of /);
    expect(displayName(item(1, { name: ' Neon ' }))).toBe('Neon');
  });
});

describe('order', () => {
  const a = item(1, { name: 'beta', worn: 10, created: 3 });
  const b = item(2, { name: 'Alpha', worn: 30, created: 1, favourite: true });
  const c = item(3, { name: 'gamma', worn: 0, created: 2 });
  const ids = (list: WardrobeItem[]) => list.map((i) => i.id);

  it('sorts by worn, newest, name and favourites', () => {
    expect(ids(sortedEntries([a, b, c], 'worn'))).toEqual([b.id, a.id, c.id]);
    expect(ids(sortedEntries([a, b, c], 'new'))).toEqual([a.id, c.id, b.id]);
    expect(ids(sortedEntries([a, b, c], 'name'))).toEqual([b.id, a.id, c.id]);
    expect(ids(sortedEntries([a, c, b], 'favourite'))).toEqual([b.id, a.id, c.id]);
  });

  it('never changes the list it is given', () => {
    const list = [a, b, c];
    sortedEntries(list, 'name');
    expect(ids(list)).toEqual([a.id, b.id, c.id]);
  });

  it('counts by name numbers as numbers', () => {
    const ten = item(4, { name: 'Skin 10' });
    const two = item(5, { name: 'Skin 2' });
    expect(ids(sortedEntries([ten, two], 'name'))).toEqual([two.id, ten.id]);
  });
});

describe('trying on', () => {
  const skin = item(1);
  const other = item(2);
  const cape = item(3, { kind: 'cape', texture: texture(3, { height: 32 }) });
  const both = set(1);
  const capeOnly = set(2, { skin: null });
  const wardrobe = closet({
    worn: { skin: texture(9), cape: texture(8, { height: 32 }), uploads: null },
    items: [skin, other, cape],
    sets: [both, capeOnly],
  });

  it('tries a skin and a cape together, and takes either off the trial', () => {
    let trial = toggleItem(null, skin);
    trial = toggleItem(trial, cape);
    expect(trial).toEqual({ of: 'items', skin: skin.id, cape: cape.id });
    expect(isTried(trial, skin) && isTried(trial, cape)).toBe(true);
    // Another skin takes the first one's place.
    trial = toggleItem(trial, other);
    expect(trial).toEqual({ of: 'items', skin: other.id, cape: cape.id });
    trial = toggleItem(toggleItem(trial, other), cape);
    expect(trial).toBeNull();
  });

  it('tries a set instead of items, and the same set again ends it', () => {
    const trial = toggleSet(toggleItem(null, skin), both);
    expect(trial).toEqual({ of: 'set', id: both.id });
    expect(isTried(trial, skin)).toBe(false);
    expect(toggleSet(trial, both)).toBeNull();
    expect(toggleItem(trial, skin)).toEqual({ of: 'items', skin: skin.id, cape: null });
  });

  it('says what the trial changes, and wears it from the wardrobe', () => {
    const trial = toggleItem(toggleItem(null, skin), cape);
    expect(trialPieces(wardrobe, trial)).toEqual({
      skin: skin.texture,
      cape: cape.texture,
      names: { skin: 'Skin 1', cape: 'Skin 3', set: null },
    });
    expect(trialChanges(wardrobe, trial)).toEqual([
      { action: 'wear', of: 'item', id: skin.id },
      { action: 'wear', of: 'item', id: cape.id },
    ]);
    // A set without a skin takes the worn one off.
    const pieces = trialPieces(wardrobe, { of: 'set', id: capeOnly.id });
    expect(pieces?.skin).toBeNull();
    expect(trialChanges(wardrobe, { of: 'set', id: capeOnly.id })).toEqual([
      { action: 'wear', of: 'set', id: capeOnly.id },
    ]);
  });

  it('changes nothing for what is worn already or no longer kept', () => {
    const worn = closet({ ...wardrobe, worn: { ...wardrobe.worn, skin: skin.texture } });
    expect(trialPieces(worn, toggleItem(null, skin))).toBeNull();
    expect(trialChanges(worn, toggleItem(null, skin))).toEqual([]);
    expect(trialPieces(wardrobe, { of: 'items', skin: id(77), cape: null })).toBeNull();
    expect(trialPieces(wardrobe, { of: 'set', id: id(77) })).toBeNull();
    // An id of a cape tried as a skin is not that skin.
    expect(trialPieces(wardrobe, { of: 'items', skin: cape.id, cape: null })).toBeNull();
    expect(trialChanges(wardrobe, null)).toEqual([]);
  });
});

describe('room and facts', () => {
  it('counts each kind against its limit', () => {
    const full = closet({
      items: [item(1), item(2, { kind: 'cape' })],
      sets: [set(1)],
      limits: { skins: 2, capes: 1, sets: 5 },
    });
    expect(room(full, 'skin')).toEqual({ count: 1, limit: 2 });
    expect(room(full, 'cape')).toEqual({ count: 1, limit: 1 });
    expect(room(full, 'set')).toEqual({ count: 1, limit: 5 });
  });

  it('marks HD, frames and slim arms, or says the size', () => {
    expect(
      chipsOf('skin', texture(1, { width: 256, height: 1024, frames: 4, slim: true })),
    ).toEqual(['HD ×4', '4 frames', 'Slim arms']);
    expect(chipsOf('skin', texture(1))).toEqual(['64×64']);
    // A cape's arms are not a thing.
    expect(chipsOf('cape', texture(1, { height: 32, slim: true }))).toEqual(['64×32']);
  });

  it('says the uploads left, and when they come back', () => {
    expect(uploadsLine(null)).toBe('Up to 10 uploads an hour.');
    expect(uploadsLine({ left: 7, resetSeconds: 1200 })).toBe('Uploads left this hour: 7 of 10.');
    expect(uploadsLine({ left: 0, resetSeconds: 61 })).toBe(
      'Uploads left this hour: 0 of 10. The count starts over in 2 min.',
    );
    expect(uploadsLine({ left: 0, resetSeconds: 1 })).toMatch(/in 1 min\.$/);
  });
});

describe('a picked file', () => {
  it('is what the core describes', () => {
    expect(pickedOf(picked())).not.toBeNull();
    expect(
      pickedOf(
        picked({
          source: 'gif',
          notes: [
            { kind: 'merged', from: 8, to: 4 },
            { kind: 'clamped', frame: 0, was: 20, now: 50 },
          ],
          frameMs: [50, 100, 100, 100],
          slim: true,
        }),
      ),
    ).not.toBeNull();
  });

  it('is nothing else', () => {
    for (const junk of [
      null,
      'picked',
      [],
      picked({ kind: 'elytra' }),
      picked({ name: 7 }),
      picked({ name: 'x'.repeat(1025) }),
      picked({ width: 100 }),
      picked({ height: 0 }),
      picked({ height: 1.5 }),
      picked({ frames: 0 }),
      picked({ frames: 33 }),
      picked({ frameMs: 'x' }),
      picked({ frameMs: Array.from({ length: 33 }, () => 100) }),
      picked({ frameMs: [100, -1] }),
      picked({ source: 'svg' }),
      picked({ notes: [{ kind: 'script' }] }),
      picked({ notes: Array.from({ length: 65 }, () => ({ kind: 'merged', from: 2, to: 1 })) }),
      picked({ bytes: 9 * 1024 * 1024 }),
      picked({ preview: 'https://example.com/skin.png' }),
      picked({ preview: 'javascript:alert(1)' }),
      picked({ preview: 'data:image/svg+xml;base64,PHN2Zz4=' }),
      picked({ slim: 'yes' }),
    ]) {
      expect(pickedOf(junk)).toBeNull();
    }
  });
});
