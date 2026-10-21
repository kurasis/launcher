import { useCallback, useEffect, useId, useRef, useState, type ClipboardEvent } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { StatusBadge, type StatusTone } from '../../components/StatusBadge';
import forms from '../../components/forms.module.css';
import type {
  AppSnapshot,
  Delivery,
  Gallery,
  MyReviews,
  ReviewFile,
  ReviewKind,
  ReviewStatus,
  ScrubCounts,
  SentReview,
} from '../../lib/contracts';
import { describeIpcError, ipc } from '../../lib/ipc';
import { numberLocale, strings } from '../../lib/strings';
import { gameLabel } from '../instances/loaderLabel';
import { galleryUrl } from '../gallery/galleryUrl';
import tree from '../../../services/reviews/categories.json';
import type { ReviewPrefill } from './openReview';
import styles from './Reviews.module.css';

const t = strings.reviews;
const KINDS = tree.kinds as ReviewKind[];
const MIN_MESSAGE = 20;
const MAX_MESSAGE = 5000;
const MAX_IMAGES = 5;
const MAX_TEXTS = 5;
/** A picture file is read up to this size; it is scaled down before sending. */
const MAX_PICTURE_BYTES = 15 * 1024 * 1024;
/** The newest part of a text file read; the core keeps its last 512 KiB. */
const MAX_TEXT_READ = 2 * 1024 * 1024;
/** Longest side of a picture sent. */
const MAX_SIDE = 2560;
const TEXT_ACCEPT =
  '.log,.txt,.json,.json5,.toml,.cfg,.conf,.properties,.yml,.yaml,.xml,.csv,.ini,text/plain';
const SHOWN_CHARS = 100_000;
const DRAFT_KEY = 'launcher.reviewDraft';

interface Attachment {
  key: string;
  file: ReviewFile;
  /** A picture to show. */
  preview?: string;
}

interface SavedDraft {
  kind: ReviewKind;
  category: string;
  subcategory: string;
  message: string;
}

function loadDraft(): SavedDraft | null {
  try {
    const raw = window.localStorage.getItem(DRAFT_KEY);
    return raw === null ? null : (JSON.parse(raw) as SavedDraft);
  } catch {
    return null;
  }
}

function saveDraft(draft: SavedDraft | null) {
  try {
    if (draft === null) window.localStorage.removeItem(DRAFT_KEY);
    else window.localStorage.setItem(DRAFT_KEY, JSON.stringify(draft));
  } catch {
    // Only a convenience.
  }
}

function subsOf(category: string): string[] {
  return tree.categories.find((c) => c.id === category)?.sub ?? [];
}

function categoryLabel(category: string): string {
  return (t.categories as Record<string, string | undefined>)[category] ?? category;
}

function subLabel(sub: string): string {
  return (t.subcategories as Record<string, string | undefined>)[sub] ?? sub;
}

function base64Of(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const url = typeof reader.result === 'string' ? reader.result : '';
      resolve(url.slice(url.indexOf(',') + 1));
    };
    reader.onerror = () => {
      reject(new Error('read'));
    };
    reader.readAsDataURL(blob);
  });
}

/**
 * A picture as it is sent: drawn again at 2560 pixels at most, so nothing of
 * the file but its pixels leaves (no place, camera or comments). A photo stays
 * JPEG; anything else becomes PNG.
 */
async function redraw(file: Blob): Promise<{ data: string; preview: string }> {
  const bitmap = await createImageBitmap(file);
  const scale = Math.min(1, MAX_SIDE / Math.max(bitmap.width, bitmap.height));
  const canvas = document.createElement('canvas');
  canvas.width = Math.max(1, Math.round(bitmap.width * scale));
  canvas.height = Math.max(1, Math.round(bitmap.height * scale));
  canvas.getContext('2d')?.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
  bitmap.close();
  const type = file.type === 'image/jpeg' ? 'image/jpeg' : 'image/png';
  const blob = await new Promise<Blob | null>((resolve) => {
    canvas.toBlob(resolve, type, 0.88);
  });
  if (blob === null) throw new Error('encode');
  const data = await base64Of(blob);
  return { data, preview: `data:${type};base64,${data}` };
}

let attachmentKey = 0;
function nextKey(): string {
  attachmentKey += 1;
  return `a${String(attachmentKey)}`;
}

function sum(a: ScrubCounts, b: ScrubCounts): ScrubCounts {
  return {
    chatLines: a.chatLines + b.chatLines,
    names: a.names + b.names,
    addresses: a.addresses + b.addresses,
    worlds: a.worlds + b.worlds,
    ids: a.ids + b.ids,
  };
}

const NO_COUNTS: ScrubCounts = { chatLines: 0, names: 0, addresses: 0, worlds: 0, ids: 0 };

interface ReviewsViewProps {
  snapshot: AppSnapshot;
  /** What the screen that opened this one knew. */
  prefill: ReviewPrefill | null;
  /** How many answers are not read yet (the dot on More). */
  onUnseen: (count: number) => void;
}

/** More → Leave a review (D-070): a new review, and the player's reviews with answers. */
export function ReviewsView({ snapshot, prefill, onUnseen }: ReviewsViewProps) {
  const [tab, setTab] = useState<'new' | 'mine'>('new');
  const [mine, setMine] = useState<MyReviews | null>(null);
  const refreshMine = useCallback(
    (refresh: boolean) =>
      ipc
        .reviewsMine(refresh)
        .then((found) => {
          setMine(found);
          onUnseen(found.unseen);
          return found;
        })
        .catch(() => null),
    [onUnseen],
  );
  useEffect(() => {
    void refreshMine(false);
  }, [refreshMine]);
  const unseen = mine?.unseen ?? 0;

  return (
    <Panel title={t.title}>
      <div className={styles.tabs} role="tablist" aria-label={t.title}>
        <button
          type="button"
          role="tab"
          aria-selected={tab === 'new'}
          className={styles.tab}
          onClick={() => {
            setTab('new');
          }}
        >
          {t.newTab}
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={tab === 'mine'}
          className={styles.tab}
          onClick={() => {
            setTab('mine');
          }}
        >
          {unseen > 0 ? t.mineTabNew({ count: unseen }) : t.mineTab}
        </button>
      </div>
      {tab === 'new' ? (
        <ReviewForm
          snapshot={snapshot}
          prefill={prefill}
          onSent={() => {
            void refreshMine(false);
          }}
          onShowMine={() => {
            setTab('mine');
          }}
        />
      ) : (
        <MyReviewsList mine={mine} refresh={refreshMine} onUnseen={onUnseen} />
      )}
    </Panel>
  );
}

interface ReviewFormProps {
  snapshot: AppSnapshot;
  prefill: ReviewPrefill | null;
  onSent: () => void;
  onShowMine: () => void;
}

function ReviewForm({ snapshot, prefill, onSent, onShowMine }: ReviewFormProps) {
  const ids = {
    category: useId(),
    sub: useId(),
    instance: useId(),
    message: useId(),
    rating: useId(),
  };
  const saved = prefill === null ? loadDraft() : null;
  const [kind, setKind] = useState<ReviewKind>(prefill?.kind ?? saved?.kind ?? 'bug');
  const [category, setCategory] = useState(prefill?.category ?? saved?.category ?? 'launch');
  const [subcategory, setSubcategory] = useState(
    prefill?.subcategory ?? saved?.subcategory ?? subsOf(prefill?.category ?? 'launch')[0] ?? '',
  );
  const [message, setMessage] = useState(saved?.message ?? '');
  const [rating, setRating] = useState<number | null>(null);
  const instances = snapshot.instances;
  const [instanceId, setInstanceId] = useState<string | null>(
    prefill?.instanceId ?? snapshot.settings.selectedInstanceId,
  );
  const runId = prefill?.runId ?? null;
  const [includeLogs, setIncludeLogs] = useState((prefill?.kind ?? kind) === 'bug');
  /** The logs found, and for which installation and run. */
  const [found, setFound] = useState<{ key: string; files: ReviewFile[] } | null>(null);
  const logsKey = `${instanceId ?? ''}/${runId ?? ''}`;
  const logs = found?.key === logsKey ? found.files : null;
  const [logCounts, setLogCounts] = useState<ScrubCounts>(NO_COUNTS);
  const [keptLogs, setKeptLogs] = useState<Set<string>>(new Set());
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [fileCounts, setFileCounts] = useState<ScrubCounts>(NO_COUNTS);
  const [gallery, setGallery] = useState<Gallery | null>(null);
  const signedIn = snapshot.account.launcherAccount.id !== null;
  const [linkAccount, setLinkAccount] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [delivery, setDelivery] = useState<Delivery | null>(null);
  const pictureInput = useRef<HTMLInputElement>(null);
  const textInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    saveDraft(message.trim() === '' ? null : { kind, category, subcategory, message });
  }, [kind, category, subcategory, message]);

  // The logs: found again when the installation changes.
  useEffect(() => {
    if (!includeLogs) return;
    let current = true;
    const key = `${instanceId ?? ''}/${runId ?? ''}`;
    ipc
      .reviewsLogs(instanceId, runId)
      .then((result) => {
        if (!current) return;
        setFound({ key, files: result.files });
        setLogCounts(result.removed);
        setKeptLogs(new Set(result.files.map((f) => f.name)));
      })
      .catch((reason: unknown) => {
        if (current) setError(describeIpcError(reason));
      });
    return () => {
      current = false;
    };
  }, [includeLogs, instanceId, runId]);

  const images = attachments.filter((a) => a.file.kind === 'image');
  const texts = attachments.filter((a) => a.file.kind === 'text');

  const addPictures = async (files: readonly Blob[]) => {
    setError(null);
    for (const file of files) {
      if (images.length >= MAX_IMAGES) {
        setError(t.tooManyPictures({ count: MAX_IMAGES }));
        return;
      }
      if (file.size > MAX_PICTURE_BYTES) {
        setError(t.pictureTooLarge);
        continue;
      }
      try {
        const { data, preview } = await redraw(file);
        setAttachments((old) =>
          old.filter((a) => a.file.kind === 'image').length >= MAX_IMAGES
            ? old
            : [
                ...old,
                { key: nextKey(), file: { name: 'screenshot.png', kind: 'image', data }, preview },
              ],
        );
      } catch {
        setError(t.pictureUnreadable);
      }
    }
  };

  const addTexts = async (files: readonly File[]) => {
    setError(null);
    for (const file of files) {
      if (texts.length >= MAX_TEXTS) {
        setError(t.tooManyFiles({ count: MAX_TEXTS }));
        return;
      }
      try {
        const part = file.size > MAX_TEXT_READ ? file.slice(file.size - MAX_TEXT_READ) : file;
        const found = await ipc.reviewsTextFile(file.name, await part.text());
        const scrubbed = found.files[0];
        if (scrubbed === undefined) continue;
        setFileCounts((old) => sum(old, found.removed));
        setAttachments((old) => [...old, { key: nextKey(), file: scrubbed }]);
      } catch (reason: unknown) {
        setError(describeIpcError(reason));
      }
    }
  };

  const addScreenshot = (instance: string, file: string, preview: string) => {
    setError(null);
    if (images.length >= MAX_IMAGES) {
      setError(t.tooManyPictures({ count: MAX_IMAGES }));
      return;
    }
    ipc
      .reviewsScreenshot(instance, file)
      .then((data) => {
        setAttachments((old) => [
          ...old,
          { key: nextKey(), file: { name: 'screenshot.png', kind: 'image', data }, preview },
        ]);
        setGallery(null);
      })
      .catch((reason: unknown) => {
        setError(describeIpcError(reason));
      });
  };

  const onPaste = (event: ClipboardEvent<HTMLDivElement>) => {
    const pictures = Array.from(event.clipboardData.files).filter((f) =>
      f.type.startsWith('image/'),
    );
    if (pictures.length === 0) return;
    event.preventDefault();
    void addPictures(pictures);
  };

  const chars = message.trim().length;
  const valid = chars >= MIN_MESSAGE && chars <= MAX_MESSAGE && category !== '';

  const send = () => {
    setSending(true);
    setError(null);
    const logFiles = includeLogs ? (logs ?? []).filter((f) => keptLogs.has(f.name)) : [];
    ipc
      .reviewsSend({
        kind,
        category,
        subcategory: subcategory === '' ? null : subcategory,
        message: message.trim(),
        rating: kind === 'impression' ? rating : null,
        linkAccount: signedIn && linkAccount,
        diagnosticId: prefill?.diagnosticId ?? null,
        ad: prefill?.ad ?? null,
        files: [...attachments.map((a) => a.file), ...logFiles],
      })
      .then((result) => {
        setDelivery(result);
        saveDraft(null);
        onSent();
      })
      .catch((reason: unknown) => {
        setError(describeIpcError(reason));
      })
      .finally(() => {
        setSending(false);
      });
  };

  if (delivery !== null) {
    return (
      <div className={styles.done}>
        <Notice tone="info">{delivery === 'sent' ? t.sent : t.queued}</Notice>
        <div className={forms.actions}>
          <Button variant="primary" onClick={onShowMine}>
            {t.showMine}
          </Button>
        </div>
      </div>
    );
  }

  const removed = sum(includeLogs ? logCounts : NO_COUNTS, fileCounts);
  const anyRemoved = Object.values(removed).some((n) => n > 0);

  return (
    <div className={forms.form} onPaste={onPaste}>
      <p className={styles.muted}>{t.intro}</p>
      <fieldset className={styles.kinds} disabled={sending}>
        <legend>{t.kindLegend}</legend>
        {KINDS.map((choice) => (
          <label key={choice} className={styles.kind}>
            <input
              type="radio"
              name="review-kind"
              value={choice}
              checked={kind === choice}
              onChange={() => {
                setKind(choice);
                setIncludeLogs(choice === 'bug');
              }}
            />
            <span>{t.kinds[choice]}</span>
          </label>
        ))}
      </fieldset>

      <div className={forms.row}>
        <div className={styles.field}>
          <label htmlFor={ids.category}>{t.category}</label>
          <select
            id={ids.category}
            className={forms.select}
            value={category}
            disabled={sending}
            onChange={(event) => {
              setCategory(event.target.value);
              setSubcategory(subsOf(event.target.value)[0] ?? '');
            }}
          >
            {tree.categories.map((c) => (
              <option key={c.id} value={c.id}>
                {categoryLabel(c.id)}
              </option>
            ))}
          </select>
        </div>
        {subsOf(category).length > 0 && (
          <div className={styles.field}>
            <label htmlFor={ids.sub}>{t.subcategory}</label>
            <select
              id={ids.sub}
              className={forms.select}
              value={subcategory}
              disabled={sending}
              onChange={(event) => {
                setSubcategory(event.target.value);
              }}
            >
              {subsOf(category).map((sub) => (
                <option key={sub} value={sub}>
                  {subLabel(sub)}
                </option>
              ))}
            </select>
          </div>
        )}
        {kind === 'bug' && instances.length > 0 && runId === null && (
          <div className={styles.field}>
            <label htmlFor={ids.instance}>{t.installation}</label>
            <select
              id={ids.instance}
              className={forms.select}
              value={instanceId ?? ''}
              disabled={sending}
              onChange={(event) => {
                setInstanceId(event.target.value === '' ? null : event.target.value);
              }}
            >
              <option value="">{t.noInstallation}</option>
              {instances.map((instance) => (
                <option key={instance.id} value={instance.id}>
                  {instance.displayName} · {gameLabel(instance)}
                </option>
              ))}
            </select>
          </div>
        )}
      </div>

      {kind === 'impression' && (
        <fieldset className={styles.kinds} disabled={sending}>
          <legend id={ids.rating}>{t.rating}</legend>
          {[1, 2, 3, 4, 5].map((n) => (
            <label key={n} className={styles.kind}>
              <input
                type="radio"
                name="review-rating"
                checked={rating === n}
                onChange={() => {
                  setRating(n);
                }}
              />
              <span>{t.stars({ count: n })}</span>
            </label>
          ))}
        </fieldset>
      )}

      <div className={styles.field}>
        <label htmlFor={ids.message}>{t.message}</label>
        <textarea
          id={ids.message}
          className={styles.message}
          value={message}
          rows={7}
          maxLength={MAX_MESSAGE}
          disabled={sending}
          placeholder={kind === 'bug' ? t.messageBugHint : t.messageHint}
          onChange={(event) => {
            setMessage(event.target.value);
          }}
        />
        <p className={styles.muted}>
          {chars < MIN_MESSAGE
            ? t.messageShort({ count: MIN_MESSAGE - chars })
            : t.messageCount({ count: chars, max: MAX_MESSAGE })}{' '}
          {t.noSecrets}
        </p>
      </div>

      <div className={styles.field}>
        <span className={styles.label}>{t.files}</span>
        <p className={styles.muted}>{t.filesHint}</p>
        {attachments.length > 0 && (
          <ul className={styles.attachments}>
            {attachments.map((a) => (
              <li key={a.key} className={styles.attachment}>
                {a.preview !== undefined ? (
                  <img src={a.preview} alt={t.picture} className={styles.thumb} />
                ) : (
                  <details>
                    <summary dir="ltr">{a.file.name}</summary>
                    <pre className={styles.text} dir="ltr">
                      {(a.file.text ?? '').slice(0, SHOWN_CHARS)}
                    </pre>
                  </details>
                )}
                <Button
                  disabled={sending}
                  onClick={() => {
                    setAttachments((old) => old.filter((b) => b.key !== a.key));
                  }}
                >
                  {t.remove}
                </Button>
              </li>
            ))}
          </ul>
        )}
        <div className={forms.actions}>
          <Button
            disabled={sending || images.length >= MAX_IMAGES}
            onClick={() => {
              pictureInput.current?.click();
            }}
          >
            {t.addPicture}
          </Button>
          <Button
            disabled={sending || images.length >= MAX_IMAGES}
            onClick={() => {
              ipc
                .listScreenshots()
                .then(setGallery)
                .catch((reason: unknown) => {
                  setError(describeIpcError(reason));
                });
            }}
          >
            {t.fromGallery}
          </Button>
          <Button
            disabled={sending || texts.length >= MAX_TEXTS}
            onClick={() => {
              textInput.current?.click();
            }}
          >
            {t.addFile}
          </Button>
        </div>
        <input
          ref={pictureInput}
          type="file"
          accept="image/png,image/jpeg,image/webp"
          multiple
          hidden
          aria-label={t.addPicture}
          onChange={(event) => {
            const files = Array.from(event.target.files ?? []);
            event.target.value = '';
            void addPictures(files);
          }}
        />
        <input
          ref={textInput}
          type="file"
          accept={TEXT_ACCEPT}
          multiple
          hidden
          aria-label={t.addFile}
          onChange={(event) => {
            const files = Array.from(event.target.files ?? []);
            event.target.value = '';
            void addTexts(files);
          }}
        />
        {gallery !== null && (
          <GalleryPicker
            gallery={gallery}
            snapshot={snapshot}
            onPick={addScreenshot}
            onClose={() => {
              setGallery(null);
            }}
          />
        )}
      </div>

      <div className={styles.field}>
        <label className={styles.check}>
          <input
            type="checkbox"
            checked={includeLogs}
            disabled={sending}
            onChange={(event) => {
              setIncludeLogs(event.target.checked);
            }}
          />
          <span>{t.includeLogs}</span>
        </label>
        {includeLogs && logs === null && error === null && (
          <p role="status" className={styles.muted}>
            {t.findingLogs}
          </p>
        )}
        {includeLogs && logs !== null && (
          <fieldset className={styles.logs} disabled={sending}>
            <legend className={styles.muted}>{t.logsFound}</legend>
            {logs.map((file) => (
              <div key={file.name} className={styles.log}>
                <label className={styles.check}>
                  <input
                    type="checkbox"
                    checked={keptLogs.has(file.name)}
                    onChange={(event) => {
                      const on = event.target.checked;
                      setKeptLogs((old) => {
                        const next = new Set(old);
                        if (on) next.add(file.name);
                        else next.delete(file.name);
                        return next;
                      });
                    }}
                  />
                  <span dir="ltr">{file.name}</span>
                </label>
                <details>
                  <summary>{t.show}</summary>
                  <pre className={styles.text} dir="ltr">
                    {(file.text ?? '').slice(0, SHOWN_CHARS)}
                  </pre>
                </details>
              </div>
            ))}
          </fieldset>
        )}
        {anyRemoved && <p className={styles.muted}>{strings.reports.removed(removed)}</p>}
      </div>

      {signedIn && (
        <label className={styles.check}>
          <input
            type="checkbox"
            checked={linkAccount}
            disabled={sending}
            onChange={(event) => {
              setLinkAccount(event.target.checked);
            }}
          />
          <span>
            {t.linkAccount}
            <span className={styles.muted}> {t.linkAccountHint}</span>
          </span>
        </label>
      )}

      <p className={styles.muted}>{t.privacy}</p>
      {error !== null && <Notice tone="danger">{error}</Notice>}
      <div className={forms.actions}>
        <Button variant="primary" onClick={send} disabled={!valid || sending}>
          {sending ? t.sending : t.send}
        </Button>
      </div>
    </div>
  );
}

interface GalleryPickerProps {
  gallery: Gallery;
  snapshot: AppSnapshot;
  onPick: (instanceId: string, file: string, preview: string) => void;
  onClose: () => void;
}

/** The newest screenshots of every installation, to pick one. */
function GalleryPicker({ gallery, snapshot, onPick, onClose }: GalleryPickerProps) {
  const shots = gallery.installations
    .flatMap((group) => group.screenshots.map((shot) => ({ group, shot })))
    .sort((a, b) => b.shot.takenAt.localeCompare(a.shot.takenAt))
    .slice(0, 24);
  const nameOf = (id: string) => snapshot.instances.find((i) => i.id === id)?.displayName ?? '';
  return (
    <div className={styles.picker}>
      {shots.length === 0 ? (
        <p className={styles.muted}>{t.noScreenshots}</p>
      ) : (
        <ul className={styles.shots}>
          {shots.map(({ group, shot }) => {
            const thumb = galleryUrl('thumb', group.instanceId, shot);
            return (
              <li key={`${group.instanceId}/${shot.file}`}>
                <button
                  type="button"
                  className={styles.shot}
                  onClick={() => {
                    onPick(group.instanceId, shot.file, thumb);
                  }}
                >
                  <img src={thumb} alt={t.screenshotOf({ name: nameOf(group.instanceId) })} />
                </button>
              </li>
            );
          })}
        </ul>
      )}
      <div className={forms.actions}>
        <Button onClick={onClose}>{t.closeGallery}</Button>
      </div>
    </div>
  );
}

const STATUS_TONES: Record<ReviewStatus, StatusTone> = {
  sent: 'neutral',
  read: 'neutral',
  answered: 'success',
  closed: 'neutral',
  gone: 'warning',
};

interface MyReviewsListProps {
  mine: MyReviews | null;
  refresh: (refresh: boolean) => Promise<MyReviews | null>;
  onUnseen: (count: number) => void;
}

function MyReviewsList({ mine, refresh, onUnseen }: MyReviewsListProps) {
  const [checking, setChecking] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<number | null>(null);

  // Opening the list asks the service; the answers shown count as read.
  useEffect(() => {
    let current = true;
    void refresh(true).then((found) => {
      if (!current) return;
      setChecking(false);
      if (found !== null && found.unseen > 0) {
        void ipc
          .reviewsSeen()
          .then(() => {
            onUnseen(0);
          })
          .catch(() => undefined);
      }
    });
    return () => {
      current = false;
    };
  }, [refresh, onUnseen]);

  const date = (iso: string) =>
    new Date(iso).toLocaleDateString(numberLocale(), { dateStyle: 'medium' });

  const remove = (review: SentReview) => {
    setError(null);
    ipc
      .reviewsDelete(review.id)
      .then(() => refresh(false))
      .catch((reason: unknown) => {
        setError(describeIpcError(reason));
      })
      .finally(() => {
        setDeleting(null);
      });
  };

  if (mine === null) return <p role="status">{t.loading}</p>;
  return (
    <div className={styles.mine}>
      {checking && (
        <p role="status" className={styles.muted}>
          {t.checking}
        </p>
      )}
      {mine.waiting > 0 && <Notice tone="info">{t.waiting({ count: mine.waiting })}</Notice>}
      {mine.reviews.length === 0 ? (
        <p className={styles.muted}>{t.none}</p>
      ) : (
        <ul className={styles.list}>
          {mine.reviews.map((review) => (
            <li key={review.id} className={styles.item}>
              <div className={styles.itemHead}>
                <strong>
                  {t.kinds[review.kind]} · {categoryLabel(review.category)}
                  {review.subcategory !== 'other' && review.subcategory !== ''
                    ? ` · ${subLabel(review.subcategory)}`
                    : ''}
                </strong>
                <StatusBadge tone={STATUS_TONES[review.status]} label={t.statuses[review.status]} />
              </div>
              <p className={styles.muted}>
                {t.number({ id: review.id, date: date(review.sentAt) })}
              </p>
              <p className={styles.excerpt} dir="auto">
                {review.excerpt}
              </p>
              {review.reply !== null && (
                <blockquote className={styles.reply}>
                  <p className={styles.label}>
                    {t.answer}
                    {review.replied !== null ? ` · ${date(review.replied)}` : ''}
                  </p>
                  <p dir="auto">{review.reply}</p>
                </blockquote>
              )}
              <div className={forms.actions}>
                {deleting === review.id ? (
                  <>
                    <span>{t.deleteConfirm}</span>
                    <Button
                      variant="primary"
                      onClick={() => {
                        remove(review);
                      }}
                    >
                      {t.delete}
                    </Button>
                    <Button
                      onClick={() => {
                        setDeleting(null);
                      }}
                    >
                      {t.cancel}
                    </Button>
                  </>
                ) : (
                  <Button
                    onClick={() => {
                      setDeleting(review.id);
                    }}
                  >
                    {t.delete}
                  </Button>
                )}
              </div>
            </li>
          ))}
        </ul>
      )}
      {error !== null && <Notice tone="danger">{error}</Notice>}
    </div>
  );
}
