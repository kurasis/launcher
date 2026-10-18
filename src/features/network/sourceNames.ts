// Download hosts as players know them: whose site it is (D-062).
const NAMES: readonly (readonly [RegExp, string])[] = [
  [/(^|\.)mojang\.com$|(^|\.)minecraft\.net$/, 'Mojang'],
  [/(^|\.)adoptium\.net$|^github\.com$/, 'Adoptium (Java)'],
  [/(^|\.)fabricmc\.net$/, 'Fabric'],
  [/(^|\.)quiltmc\.org$/, 'Quilt'],
  [/(^|\.)minecraftforge\.net$/, 'Forge'],
  [/(^|\.)neoforged\.net$/, 'NeoForge'],
];

/** Each site once, in the order first seen; an unknown host by its name. */
export function sourceNames(hosts: readonly string[]): string[] {
  const names = hosts.map((host) => NAMES.find(([pattern]) => pattern.test(host))?.[1] ?? host);
  return [...new Set(names)];
}
