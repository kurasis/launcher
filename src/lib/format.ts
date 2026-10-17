import { strings } from './strings';

/** Binary-unit size for display in the current language, e.g. `877.2 MiB`. */
export function formatBytes(bytes: number): string {
  const units = strings.units;
  let value = bytes;
  let unit = -1;
  while (value >= 1024 && unit < units.binary.length - 1) {
    value /= 1024;
    unit += 1;
  }
  if (unit < 0) return units.bytes(bytes);
  return `${value.toFixed(1).replace('.', units.decimalSeparator)} ${units.binary[unit] ?? ''}`;
}
