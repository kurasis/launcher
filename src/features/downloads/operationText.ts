// What Downloads and the launch bar say about an install or repair (D-049): its kind
// and phase, how far it got, and how a finished one ended. Pure, for both places.
import type { OperationKind, OperationProgress, OperationResult } from '../../lib/contracts';
import { formatBytes, progressPercent } from '../../lib/format';
import { localizeError, strings } from '../../lib/strings';
import { daysAgo } from '../../lib/time';

const t = strings.downloads;

/** "Installation: downloading". */
export function stateLine(kind: OperationKind, progress: OperationProgress | null): string {
  return t.state({ kind, phase: progress?.phase ?? 'resolving' });
}

/** ", 44%" after the state, once the percent is known; else nothing. */
export function percentTail(progress: OperationProgress | null): string {
  const percent = progressPercent(progress);
  return percent === null ? '' : t.percentTail({ percent });
}

/** "203.1 MiB of 457.8 MiB, 11.9 MiB/s." and "1,240 of 3,120 files.", as far as known. */
export function detailSentences(progress: OperationProgress | null): string[] {
  if (progress === null) return [];
  const sentences: string[] = [];
  const total = progress.totalDownloadBytes;
  if (total !== null && total > 0) {
    const done = formatBytes(progress.downloadedBytes);
    const all = formatBytes(total);
    sentences.push(
      progress.bytesPerSecond === null
        ? t.transferNoSpeed({ done, total: all })
        : t.transfer({ done, total: all, rate: formatBytes(progress.bytesPerSecond) }),
    );
  }
  if (progress.totalFiles !== null) {
    sentences.push(t.filesDone({ done: progress.verifiedFiles, total: progress.totalFiles }));
  }
  return sentences;
}

/** "Installation finished yesterday. Downloaded 457.8 MiB, 3,120 files." */
export function resultLine(result: OperationResult, now = Date.now()): string {
  const at = Date.parse(result.finishedAt);
  const when = daysAgo(Number.isNaN(at) ? now : at, now);
  const sentences = [t.result[result.outcome]({ kind: result.kind, when })];
  if (result.downloadedFiles > 0) {
    sentences.push(
      t.downloaded({ bytes: formatBytes(result.networkBytes), files: result.downloadedFiles }),
    );
  }
  if (result.outcome === 'failed' && result.error !== null) {
    sentences.push(localizeError(result.error));
  }
  return sentences.join(' ');
}

/**
 * A message around one value drawn apart (a file name in mono): the text before it and
 * after it, in the language's own order.
 */
export function around(format: (value: string) => string): [string, string] {
  const mark = '@@value@@';
  const whole = format(mark);
  const at = whole.indexOf(mark);
  return at === -1 ? [whole, ''] : [whole.slice(0, at), whole.slice(at + mark.length)];
}
