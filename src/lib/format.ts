const UNITS = ['bytes', 'KiB', 'MiB', 'GiB', 'TiB'] as const;

/** Binary-unit size for display, e.g. `877.2 MiB`. */
export function formatBytes(bytes: number): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return unit === 0 ? `${String(bytes)} bytes` : `${value.toFixed(1)} ${UNITS[unit] ?? ''}`;
}
