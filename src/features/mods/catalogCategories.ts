import type { IconName } from '../../components/Icon';
import type { ModsKind } from '../../lib/contracts';
import { strings } from '../../lib/strings';

/** A category of the catalog's shared list (D-072) and the icon it is drawn with. */
export interface CatalogCategory {
  /** `<kind>.<name>`, as the catalog knows it. */
  id: string;
  icon: IconName;
  /** The icon points somewhere and mirrors right to left. */
  mirror?: boolean;
}

const row = (kind: ModsKind, name: string, icon: IconName, mirror = false): CatalogCategory =>
  mirror ? { id: `${kind}.${name}`, icon, mirror } : { id: `${kind}.${name}`, icon };

/**
 * The categories of each kind, in the order the catalog's list keeps them
 * (services/mods/src/categories.rs): Modrinth's and CurseForge's merged into one.
 */
export const CATEGORIES: Record<ModsKind, readonly CatalogCategory[]> = {
  mod: [
    row('mod', 'optimization', 'gauge'),
    row('mod', 'utility', 'wrench'),
    row('mod', 'maps-info', 'map'),
    row('mod', 'library', 'books'),
    row('mod', 'technology', 'gear'),
    row('mod', 'magic', 'wand'),
    row('mod', 'adventure', 'compass'),
    row('mod', 'worldgen', 'mountains'),
    row('mod', 'mobs', 'paw'),
    row('mod', 'equipment', 'sword'),
    row('mod', 'decoration', 'armchair'),
    row('mod', 'food', 'apple'),
    row('mod', 'storage', 'chest'),
    row('mod', 'transportation', 'cart', true),
    row('mod', 'game-mechanics', 'sliders'),
    row('mod', 'server', 'server'),
    row('mod', 'social', 'chat'),
    row('mod', 'addons', 'mods'),
    row('mod', 'fun', 'die'),
  ],
  datapack: [
    row('datapack', 'adventure', 'compass'),
    row('datapack', 'magic', 'wand'),
    row('datapack', 'game-mechanics', 'sliders'),
    row('datapack', 'worldgen', 'mountains'),
    row('datapack', 'mobs', 'paw'),
    row('datapack', 'equipment', 'sword'),
    row('datapack', 'technology', 'gear'),
    row('datapack', 'decoration', 'armchair'),
    row('datapack', 'utility', 'wrench'),
    row('datapack', 'optimization', 'gauge'),
    row('datapack', 'library', 'books'),
    row('datapack', 'fun', 'die'),
    row('datapack', 'mod-support', 'mods'),
  ],
  resourcepack: [
    row('resourcepack', 'vanilla-like', 'cube'),
    row('resourcepack', 'tweaks', 'edit'),
    row('resourcepack', 'realistic', 'camera'),
    row('resourcepack', 'simplistic', 'shapes'),
    row('resourcepack', 'themed', 'castle'),
    row('resourcepack', 'decoration', 'palette'),
    row('resourcepack', 'combat', 'shield'),
    row('resourcepack', 'utility', 'wrench'),
    row('resourcepack', 'modded', 'mods'),
    row('resourcepack', 'animated', 'animated', true),
    row('resourcepack', 'fonts', 'letters'),
    row('resourcepack', 'cursed', 'ghost'),
  ],
  shader: [
    row('shader', 'vanilla-like', 'cube'),
    row('shader', 'semi-realistic', 'sunHalf'),
    row('shader', 'realistic', 'sun'),
    row('shader', 'fantasy', 'sparkles'),
    row('shader', 'cartoon', 'cloud'),
    row('shader', 'cursed', 'ghost'),
  ],
  modpack: [
    row('modpack', 'adventure', 'compass'),
    row('modpack', 'technology', 'gear'),
    row('modpack', 'magic', 'wand'),
    row('modpack', 'quests', 'checklist'),
    row('modpack', 'challenging', 'flame'),
    row('modpack', 'combat', 'sword'),
    row('modpack', 'kitchen-sink', 'boxes'),
    row('modpack', 'lightweight', 'feather'),
    row('modpack', 'vanilla-plus', 'cube'),
    row('modpack', 'optimization', 'gauge'),
    row('modpack', 'multiplayer', 'users'),
    row('modpack', 'skyblock', 'island'),
  ],
  world: [
    row('world', 'adventure', 'compass'),
    row('world', 'survival', 'tent'),
    row('world', 'creation', 'house'),
    row('world', 'game-map', 'gamepad'),
    row('world', 'parkour', 'jump', true),
    row('world', 'puzzle', 'bulb'),
    row('world', 'modded', 'mods'),
  ],
};

/** At most this many categories at once: the catalog takes no more. */
export const MAX_CATEGORIES = 8;

/** The category of this kind with this ID, or `null` for one the launcher does not know. */
export function categoryOf(kind: ModsKind, id: string): CatalogCategory | null {
  return CATEGORIES[kind].find((category) => category.id === id) ?? null;
}

/** A category's name in the interface language. */
export function categoryName(id: string): string {
  const [kind = '', name = ''] = id.split('.');
  const names = strings.mods.catalog.categoryNames as Record<
    string,
    Record<string, string | undefined> | undefined
  >;
  const ofKind = Object.hasOwn(names, kind) ? names[kind] : undefined;
  const found = ofKind !== undefined && Object.hasOwn(ofKind, name) ? ofKind[name] : undefined;
  return typeof found === 'string' ? found : id;
}
