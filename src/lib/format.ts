import { numberLocale, strings } from './strings';

const UNITS = ['KiB', 'MiB', 'GiB', 'TiB'] as const;

/** Binary-unit size in the current language, e.g. `877.2 MiB` or `877,2 МиБ`. */
export function formatBytes(bytes: number): string {
  let value = bytes;
  let unit = -1;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const name = UNITS[unit];
  if (name === undefined) return strings.units.bytes({ count: bytes });
  const number = new Intl.NumberFormat(numberLocale(), {
    minimumFractionDigits: 1,
    maximumFractionDigits: 1,
    useGrouping: false,
  }).format(value);
  return strings.units[name]({ value: number });
}
