import type { OperationProgress } from './contracts';
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

/** A whole number typed in a form, or `NaN` for anything else. */
export function toInt(text: string): number {
  return /^\d+$/.test(text.trim()) ? Number(text.trim()) : Number.NaN;
}

/**
 * How far an install or repair is, 0–100, or null when unknown: files checked
 * while checking what is present, bytes while downloading.
 */
export function progressPercent(progress: OperationProgress | null): number | null {
  if (progress === null) return null;
  const [done, all] =
    progress.phase === 'checking_files'
      ? [progress.verifiedFiles, progress.totalFiles]
      : [progress.downloadedBytes, progress.totalDownloadBytes];
  return all !== null && all > 0 ? Math.min(100, Math.floor((done / all) * 100)) : null;
}
