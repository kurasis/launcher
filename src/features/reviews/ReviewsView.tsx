import {
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  type ClipboardEvent,
  type RefObject,
} from 'react';
import { Button } from '../../components/Button';
import { Choice } from '../../components/Choice';
import { Confirm } from '../../components/Confirm';
import { Field } from '../../components/Field';
import { Icon } from '../../components/Icon';
import { LiveStatus } from '../../components/LiveStatus';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { Screen } from '../../components/Screen';
import { TabPanel, Tabs } from '../../components/Tabs';
import { Tag, type TagTone } from '../../components/Tag';
import forms from '../../components/forms.module.css';
import { useFocusBack } from '../../components/useFocusBack';
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
import { isolate, strings } from '../../lib/strings';
import { shortWhen } from '../../lib/time';
import { gameLabel } from '../instances/loaderLabel';
import { galleryUrl } from '../gallery/galleryUrl';
import tree from '../../../services/reviews/categories.json';
import type { ReviewPrefill } from './openReview';
import styles from './Reviews.module.css';

const t = strings.reviews;
const KINDS = tree.kinds as ReviewKind[];
const MIN_MESSAGE = 20;
const MAX_MESSAGE = 5000;
const MAX_SUBJECT = 80;
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
/** How many of the player's reviews the panel beside the form lists. */
const RECENT = 5;

interface Attachment {
  key: string;
  file: ReviewFile;
  /** What its chip says. A picture's own name stays here: it is sent as screenshot.png. */
  label: string;
  /** A picture to show. */
  preview?: string;
}

interface SavedDraft {
  kind: ReviewKind;
  category: string;
  subcategory: string;
  subject?: string;
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

/**
 * What a review is about: the subject the player wrote, or as briefly as its
 * place says it ("Crashes").
 */
function subjectOf(review: SentReview): string {
  if (review.subject !== null && review.subject !== '') return review.subject;
  return review.subcategory === '' || review.subcategory === 'other'
    ? categoryLabel(review.category)
    : subLabel(review.subcategory);
}

/** Kind, category and subcategory: "A problem · Starting the game · Crashes". */
function placeOf(review: SentReview): string {
  return [
    t.kinds[review.kind],
    categoryLabel(review.category),
    ...(review.subcategory !== 'other' && review.subcategory !== ''
      ? [subLabel(review.subcategory)]
      : []),
  ].join(' · ');
}

/** The day a review was sent or answered, inside a sentence. */
const dayOf = (at: string) => isolate(shortWhen(at));

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

/** The reviews whose answer is not read yet. */
function unread(mine: MyReviews | null): Set<number> {
  return new Set(
    (mine?.reviews ?? []).filter((r) => r.reply !== null && !r.replySeen).map((r) => r.id),
  );
}

const TAG_TONES: Record<ReviewStatus, TagTone> = {
  sent: 'neutral',
  read: 'neutral',
  answered: 'success',
  closed: 'outline',
  gone: 'warning',
};

type ReviewsTab = 'new' | 'mine';

/** What a read of the player's reviews found, and whether it is the one shown: a newer
 * read can come back first. */
interface MineAnswer {
  found: MyReviews | null;
  applied: boolean;
}

interface ReviewsViewProps {
  snapshot: AppSnapshot;
  /** What the screen that opened this one knew. */
  prefill: ReviewPrefill | null;
  /** How many answers are not read yet (the dot on More). */
  onUnseen: (count: number) => void;
}

/**
 * More → Leave a review (D-070): a new review with the player's latest reviews beside
 * it, and all their reviews with the answers under the second tab.
 */
export function ReviewsView({ snapshot, prefill, onUnseen }: ReviewsViewProps) {
  const id = useId();
  const [tab, setTab] = useState<ReviewsTab>('new');
  const [mine, setMine] = useState<MyReviews | null>(null);
  const [mineError, setMineError] = useState<string | null>(null);
  /** The review "Open" asked for, to be shown under My reviews. */
  const [opening, setOpening] = useState<number | null>(null);
  /** What My reviews says while it reads them, in a region there before its words. */
  const [listStatus, setListStatus] = useState<string | null>(null);
  /** A new form once the player leaves a sent one; the first one keeps what opened it. */
  const [formKey, setFormKey] = useState(0);
  const sent = useRef(false);
  const newTab = useRef<HTMLButtonElement>(null);
  const mineTab = useRef<HTMLButtonElement>(null);
  const asked = useRef(0);
  const refreshMine = useCallback(
    (refresh: boolean) => {
      // Only the latest answer is shown: an older one can come after it.
      asked.current += 1;
      const ask = asked.current;
      return ipc
        .reviewsMine(refresh)
        .then((found): MineAnswer => {
          const applied = ask === asked.current;
          if (applied) {
            setMine(found);
            setMineError(null);
            onUnseen(found.unseen);
          }
          return { found, applied };
        })
        .catch((reason: unknown): MineAnswer => {
          const applied = ask === asked.current;
          if (applied) setMineError(describeIpcError(reason));
          return { found: null, applied };
        });
    },
    [onUnseen],
  );
  useEffect(() => {
    void refreshMine(false);
  }, [refreshMine]);
  const markSeen = useCallback(() => {
    setMine((old) =>
      old === null
        ? old
        : {
            ...old,
            unseen: 0,
            reviews: old.reviews.map((r) => (r.reply === null ? r : { ...r, replySeen: true })),
          },
    );
    onUnseen(0);
  }, [onUnseen]);
  const opened = useCallback(() => {
    setOpening(null);
  }, []);
  // A button that took the player to the other tab is gone with its tab.
  useFocusBack(tab, tab === 'mine' ? mineTab : newTab);

  const unseen = mine?.unseen ?? 0;
  const choose = (next: ReviewsTab) => {
    if (next !== 'new' && sent.current) {
      sent.current = false;
      setFormKey((key) => key + 1);
    }
    setTab(next);
  };
  const showMine = (review: number | null) => {
    setOpening(review);
    choose('mine');
  };
  const recent = tab === 'new' && mine !== null && (mine.reviews.length > 0 || mine.waiting > 0);

  return (
    <Screen title={t.title}>
      <Tabs
        label={t.title}
        id={id}
        value={tab}
        onChange={choose}
        items={[
          { value: 'new', label: t.newTab, ref: newTab },
          {
            value: 'mine',
            label: unseen > 0 ? t.mineTabNew({ count: unseen }) : t.mineTab,
            ref: mineTab,
          },
        ]}
      />
      <TabPanel id={id} value="new" shown={tab === 'new'}>
        <div className={styles.columns}>
          <div className={styles.formColumn}>
            <Panel>
              <ReviewForm
                key={formKey}
                snapshot={snapshot}
                prefill={formKey === 0 ? prefill : null}
                onSent={() => {
                  sent.current = true;
                  void refreshMine(false);
                }}
                onShowMine={() => {
                  showMine(null);
                }}
              />
            </Panel>
          </div>
          {recent && <RecentReviews mine={mine} onOpen={showMine} />}
        </div>
      </TabPanel>
      <TabPanel id={id} value="mine" shown={tab === 'mine'}>
        <LiveStatus>{listStatus}</LiveStatus>
        {tab === 'mine' && (
          <MyReviewsList
            mine={mine}
            error={mineError}
            refresh={refreshMine}
            onStatus={setListStatus}
            onSeen={markSeen}
            opening={opening}
            onOpened={opened}
            fallback={mineTab}
          />
        )}
      </TabPanel>
    </Screen>
  );
}

interface ReviewFormProps {
  snapshot: AppSnapshot;
  prefill: ReviewPrefill | null;
  onSent: () => void;
  onShowMine: () => void;
}

function ReviewForm({ snapshot, prefill, onSent, onShowMine }: ReviewFormProps) {
  const filesId = useId();
  const previewId = useId();
  const doneId = useId();
  const saved = prefill === null ? loadDraft() : null;
  const [kind, setKind] = useState<ReviewKind>(prefill?.kind ?? saved?.kind ?? 'bug');
  const [category, setCategory] = useState(prefill?.category ?? saved?.category ?? 'launch');
  const [subcategory, setSubcategory] = useState(
    prefill?.subcategory ?? saved?.subcategory ?? subsOf(prefill?.category ?? 'launch')[0] ?? '',
  );
  const [subject, setSubject] = useState(saved?.subject ?? '');
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
  /** The chip whose file is shown under the chips. */
  const [shownKey, setShownKey] = useState<string | null>(null);
  const [fileCounts, setFileCounts] = useState<ScrubCounts>(NO_COUNTS);
  const [gallery, setGallery] = useState<Gallery | null>(null);
  const [listing, setListing] = useState(false);
  const signedIn = snapshot.account.launcherAccount.id !== null;
  const [linkAccount, setLinkAccount] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [delivery, setDelivery] = useState<Delivery | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const chooseButton = useRef<HTMLButtonElement>(null);
  const galleryButton = useRef<HTMLButtonElement>(null);
  const showMineButton = useRef<HTMLButtonElement>(null);
  const [dragOver, setDragOver] = useState(false);

  useEffect(() => {
    saveDraft(
      message.trim() === '' && subject.trim() === ''
        ? null
        : { kind, category, subcategory, subject, message },
    );
  }, [kind, category, subcategory, subject, message]);

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

  // The screenshots closed under the one picked (first: a pick adds a chip too); a
  // removed chip took the focus with it.
  useFocusBack(gallery === null, galleryButton, chooseButton);
  useFocusBack(attachments.length, chooseButton);
  // Send is gone with the form once the review is sent.
  useFocusBack(delivery, showMineButton);

  const images = attachments.filter((a) => a.file.kind === 'image');
  const texts = attachments.filter((a) => a.file.kind === 'text');
  const shown = attachments.find((a) => a.key === shownKey);

  const addPictures = async (files: readonly Blob[]) => {
    setError(null);
    // Counted as they come: one drop can hold more than the rest allows.
    let count = images.length;
    for (const file of files) {
      if (count >= MAX_IMAGES) {
        setError(t.tooManyPictures({ count: MAX_IMAGES }));
        return;
      }
      if (file.size > MAX_PICTURE_BYTES) {
        setError(t.pictureTooLarge);
        continue;
      }
      try {
        const { data, preview } = await redraw(file);
        count += 1;
        const label = file instanceof File && file.name !== '' ? file.name : t.picture;
        setAttachments((old) =>
          old.filter((a) => a.file.kind === 'image').length >= MAX_IMAGES
            ? old
            : [
                ...old,
                {
                  key: nextKey(),
                  file: { name: 'screenshot.png', kind: 'image', data },
                  label,
                  preview,
                },
              ],
        );
      } catch {
        setError(t.pictureUnreadable);
      }
    }
  };

  const addTexts = async (files: readonly File[]) => {
    setError(null);
    let count = texts.length;
    for (const file of files) {
      if (count >= MAX_TEXTS) {
        setError(t.tooManyFiles({ count: MAX_TEXTS }));
        return;
      }
      try {
        const part = file.size > MAX_TEXT_READ ? file.slice(file.size - MAX_TEXT_READ) : file;
        const found = await ipc.reviewsTextFile(file.name, await part.text());
        const scrubbed = found.files[0];
        if (scrubbed === undefined) continue;
        count += 1;
        setFileCounts((old) => sum(old, found.removed));
        // The chip says the name the core keeps, which is the one sent.
        setAttachments((old) =>
          old.filter((a) => a.file.kind === 'text').length >= MAX_TEXTS
            ? old
            : [...old, { key: nextKey(), file: scrubbed, label: scrubbed.name }],
        );
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
        setAttachments((old) =>
          old.filter((a) => a.file.kind === 'image').length >= MAX_IMAGES
            ? old
            : [
                ...old,
                {
                  key: nextKey(),
                  file: { name: 'screenshot.png', kind: 'image', data },
                  label: file,
                  preview,
                },
              ],
        );
        setGallery(null);
      })
      .catch((reason: unknown) => {
        setError(describeIpcError(reason));
      });
  };

  const toggleGallery = () => {
    if (listing) return;
    if (gallery !== null) {
      setGallery(null);
      return;
    }
    setError(null);
    setListing(true);
    ipc
      .listScreenshots()
      .then(setGallery)
      .catch((reason: unknown) => {
        setError(describeIpcError(reason));
      })
      .finally(() => {
        setListing(false);
      });
  };

  /** Pictures and text files from the picker or dropped on the field. */
  const addFiles = (files: readonly File[]) => {
    const pictures = files.filter((f) => f.type.startsWith('image/'));
    const others = files.filter((f) => !f.type.startsWith('image/'));
    void (async () => {
      if (pictures.length > 0) await addPictures(pictures);
      if (others.length > 0) await addTexts(others);
    })();
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
    if (sending) return;
    setSending(true);
    setError(null);
    const logFiles = includeLogs ? (logs ?? []).filter((f) => keptLogs.has(f.name)) : [];
    ipc
      .reviewsSend({
        kind,
        category,
        subcategory: subcategory === '' ? null : subcategory,
        subject: subject.trim() === '' ? null : subject.trim(),
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
        {/* The button the focus goes to says it too: the notice comes with it. */}
        <div id={doneId}>
          <Notice tone="info">{delivery === 'sent' ? t.sent : t.queued}</Notice>
        </div>
        <div className={forms.actions}>
          <Button
            ref={showMineButton}
            variant="primary"
            aria-describedby={doneId}
            onClick={onShowMine}
          >
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
      <p className={styles.hint}>{t.intro}</p>
      <fieldset className={forms.choices} disabled={sending}>
        <legend>{t.kindLegend}</legend>
        <div className={styles.choiceRow}>
          {KINDS.map((choice) => (
            <Choice
              key={choice}
              type="radio"
              name="review-kind"
              value={choice}
              checked={kind === choice}
              title={t.kinds[choice]}
              onChange={() => {
                setKind(choice);
                setIncludeLogs(choice === 'bug');
              }}
            />
          ))}
        </div>
      </fieldset>

      <div className={styles.selects}>
        <Field label={t.category}>
          {(field) => (
            <select
              {...field}
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
          )}
        </Field>
        {subsOf(category).length > 0 && (
          <Field label={t.subcategory}>
            {(field) => (
              <select
                {...field}
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
            )}
          </Field>
        )}
      </div>
      {kind === 'bug' && instances.length > 0 && runId === null && (
        // A row of its own: a long name is never cut, in Arabic from its start.
        <div className={styles.wide}>
          <Field label={t.installation}>
            {(field) => (
              <select
                {...field}
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
                    {instance.displayName} ({gameLabel(instance)})
                  </option>
                ))}
              </select>
            )}
          </Field>
        </div>
      )}

      {kind === 'impression' && (
        <fieldset className={forms.choices} disabled={sending}>
          <legend>{t.rating}</legend>
          <div className={styles.choiceRow}>
            {[1, 2, 3, 4, 5].map((n) => (
              <Choice
                key={n}
                type="radio"
                name="review-rating"
                checked={rating === n}
                title={t.stars({ count: n })}
                onChange={() => {
                  setRating(n);
                }}
              />
            ))}
          </div>
        </fieldset>
      )}

      <Field label={t.subject} hint={t.subjectHint}>
        {(field) => (
          <input
            {...field}
            type="text"
            className={forms.input}
            value={subject}
            maxLength={MAX_SUBJECT}
            disabled={sending}
            placeholder={kind === 'bug' ? t.subjectBugExample : t.subjectExample}
            onChange={(event) => {
              setSubject(event.target.value);
            }}
          />
        )}
      </Field>

      <Field
        label={t.message}
        hint={
          <>
            {chars < MIN_MESSAGE
              ? t.messageShort({ count: MIN_MESSAGE - chars })
              : t.messageCount({ count: chars, max: MAX_MESSAGE })}{' '}
            {t.noSecrets}
          </>
        }
      >
        {(field) => (
          <textarea
            {...field}
            className={[forms.input, styles.message].join(' ')}
            value={message}
            rows={5}
            maxLength={MAX_MESSAGE}
            disabled={sending}
            placeholder={kind === 'bug' ? t.messageBugHint : t.messageHint}
            onChange={(event) => {
              setMessage(event.target.value);
            }}
          />
        )}
      </Field>

      <div className={styles.files} role="group" aria-labelledby={filesId}>
        <span id={filesId} className={styles.label}>
          {t.files}
        </span>
        <div
          className={styles.drop}
          data-over={dragOver}
          data-disabled={sending}
          onDragOver={(event) => {
            if (sending || !event.dataTransfer.types.includes('Files')) return;
            event.preventDefault();
            event.dataTransfer.dropEffect = 'copy';
            setDragOver(true);
          }}
          onDragLeave={(event) => {
            if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
              setDragOver(false);
            }
          }}
          onDrop={(event) => {
            event.preventDefault();
            setDragOver(false);
            if (!sending) addFiles(Array.from(event.dataTransfer.files));
          }}
        >
          <Icon name="upload" size={22} className={styles.dropIcon} />
          <p className={styles.dropTitle}>{t.dropHere}</p>
          <p className={styles.dropActions}>
            <button
              ref={chooseButton}
              type="button"
              className={styles.link}
              disabled={sending}
              onClick={() => {
                fileInput.current?.click();
              }}
            >
              {t.chooseFiles}
            </button>
            <span aria-hidden="true">·</span>
            <button
              ref={galleryButton}
              type="button"
              className={styles.link}
              disabled={sending || images.length >= MAX_IMAGES}
              aria-expanded={gallery !== null}
              aria-disabled={listing || undefined}
              onClick={toggleGallery}
            >
              {t.fromGallery}
            </button>
          </p>
          <p className={styles.hint}>{t.filesHint}</p>
        </div>
        <input
          ref={fileInput}
          type="file"
          accept={`image/*,${TEXT_ACCEPT}`}
          multiple
          hidden
          aria-label={t.chooseFiles}
          onChange={(event) => {
            const files = Array.from(event.target.files ?? []);
            event.target.value = '';
            addFiles(files);
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
        {attachments.length > 0 && (
          <ul className={styles.chips}>
            {attachments.map((a) => (
              <li key={a.key} className={styles.chip}>
                <Icon name={a.file.kind === 'image' ? 'image' : 'console'} size={14} />
                <button
                  type="button"
                  className={styles.chipName}
                  aria-expanded={shownKey === a.key}
                  aria-controls={shownKey === a.key ? previewId : undefined}
                  onClick={() => {
                    setShownKey((old) => (old === a.key ? null : a.key));
                  }}
                >
                  <bdi dir="ltr">{a.label}</bdi>
                </button>
                <button
                  type="button"
                  className={styles.chipRemove}
                  aria-label={t.removeFile({ name: isolate(a.label) })}
                  title={t.removeFile({ name: isolate(a.label) })}
                  disabled={sending}
                  onClick={() => {
                    setAttachments((old) => old.filter((b) => b.key !== a.key));
                  }}
                >
                  <Icon name="x" size={14} />
                </button>
              </li>
            ))}
          </ul>
        )}
        {shown !== undefined && (
          <div id={previewId} className={styles.preview}>
            {shown.preview !== undefined ? (
              <img src={shown.preview} alt={t.picture} className={styles.thumb} />
            ) : (
              <pre className={styles.text} dir="ltr">
                {(shown.file.text ?? '').slice(0, SHOWN_CHARS)}
              </pre>
            )}
          </div>
        )}
      </div>

      <div className={styles.logs}>
        <Choice
          type="checkbox"
          checked={includeLogs}
          disabled={sending}
          title={t.includeLogs}
          onChange={(event) => {
            setIncludeLogs(event.target.checked);
          }}
        />
        <LiveStatus className={styles.indent}>
          {includeLogs && logs === null && error === null ? t.findingLogs : null}
        </LiveStatus>
        {includeLogs && logs !== null && (
          <fieldset className={[styles.logList, styles.indent].join(' ')} disabled={sending}>
            <legend className={styles.hint}>{t.logsFound}</legend>
            {logs.map((file) => (
              <div key={file.name} className={styles.log}>
                <Choice
                  type="checkbox"
                  checked={keptLogs.has(file.name)}
                  title={
                    <span dir="ltr" className={styles.mono}>
                      {file.name}
                    </span>
                  }
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
                <details className={[styles.show, styles.indent].join(' ')}>
                  <summary>{t.show}</summary>
                  <pre className={styles.text} dir="ltr">
                    {(file.text ?? '').slice(0, SHOWN_CHARS)}
                  </pre>
                </details>
              </div>
            ))}
          </fieldset>
        )}
        {anyRemoved && (
          <p className={[styles.hint, styles.indent].join(' ')}>
            {strings.reports.removed(removed)}
          </p>
        )}
      </div>

      {signedIn && (
        <Choice
          type="checkbox"
          checked={linkAccount}
          disabled={sending}
          title={t.linkAccount}
          hint={t.linkAccountHint}
          onChange={(event) => {
            setLinkAccount(event.target.checked);
          }}
        />
      )}

      <p className={styles.hint}>{t.privacy}</p>
      {error !== null && <Notice tone="danger">{error}</Notice>}
      <div className={forms.actions}>
        <Button
          variant="primary"
          disabled={!valid}
          aria-disabled={sending || undefined}
          onClick={send}
        >
          <Icon name="send" size={18} mirror />
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
        <p className={styles.hint}>{t.noScreenshots}</p>
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
                  <img
                    src={thumb}
                    alt={t.screenshotOf({ name: isolate(nameOf(group.instanceId)) })}
                  />
                </button>
              </li>
            );
          })}
        </ul>
      )}
      <div className={forms.actions}>
        <Button size="small" onClick={onClose}>
          {t.closeGallery}
        </Button>
      </div>
    </div>
  );
}

/** The player's latest reviews beside the form, each opened under My reviews. */
function RecentReviews({
  mine,
  onOpen,
}: {
  mine: MyReviews;
  onOpen: (review: number | null) => void;
}) {
  return (
    <Panel title={t.mineTab} level={3} count={mine.reviews.length + mine.waiting}>
      {mine.waiting > 0 && <p className={styles.hint}>{t.waiting({ count: mine.waiting })}</p>}
      {mine.reviews.length > 0 && (
        <ul className={styles.recent}>
          {mine.reviews.slice(0, RECENT).map((review) => (
            <RecentReview
              key={review.id}
              review={review}
              onOpen={() => {
                onOpen(review.id);
              }}
            />
          ))}
        </ul>
      )}
      {mine.reviews.length > RECENT && (
        <div>
          <Button
            variant="ghost"
            size="small"
            onClick={() => {
              onOpen(null);
            }}
          >
            {t.allMine}
          </Button>
        </div>
      )}
    </Panel>
  );
}

function RecentReview({ review, onOpen }: { review: SentReview; onOpen: () => void }) {
  const titleId = useId();
  const numberId = useId();
  // Shown here, an answer is not read yet: it is read under My reviews.
  const fresh = review.reply !== null && !review.replySeen;
  return (
    <li className={styles.recentItem}>
      <span className={styles.recentName}>
        <span id={titleId} dir="auto">
          {subjectOf(review)}
        </span>
        <Tag tone={TAG_TONES[review.status]}>{t.statuses[review.status]}</Tag>
        {fresh && <Tag tone="warning">{t.fresh}</Tag>}
      </span>
      <span className={styles.recentMeta}>
        <span id={numberId}>{t.number({ id: review.id, date: dayOf(review.sentAt) })}</span>
        {review.reply !== null && (
          <span className={styles.recentAnswer}>
            {t.answerQuote({ reply: isolate(review.reply) })}
          </span>
        )}
      </span>
      <Button
        className={styles.recentOpen}
        variant="ghost"
        size="small"
        aria-describedby={`${titleId} ${numberId}`}
        onClick={onOpen}
      >
        {t.open}
      </Button>
    </li>
  );
}

interface MyReviewsListProps {
  mine: MyReviews | null;
  error: string | null;
  refresh: (refresh: boolean) => Promise<MineAnswer>;
  /** What to say while the reviews are read ("Checking for answers…"), or nothing. */
  onStatus: (text: string | null) => void;
  /** The answers were marked read on this computer. */
  onSeen: () => void;
  /** A review to show: the focus goes to it. */
  opening: number | null;
  onOpened: () => void;
  /** Where the focus goes when the list it was in is gone: the tab. */
  fallback: RefObject<HTMLElement | null>;
}

function MyReviewsList({
  mine,
  error,
  refresh,
  onStatus,
  onSeen,
  opening,
  onOpened,
  fallback,
}: MyReviewsListProps) {
  const [checking, setChecking] = useState(true);
  /** Try again was pressed: its button stays while it checks, so it keeps the focus. */
  const [retried, setRetried] = useState(false);
  /** The answers new when the list was opened: they keep their mark while it is open. */
  const [fresh, setFresh] = useState<ReadonlySet<number>>(() => unread(mine));
  const [asking, setAsking] = useState<number | null>(null);
  const [removing, setRemoving] = useState<number | null>(null);
  const [failed, setFailed] = useState<{ id: number; text: string } | null>(null);
  const list = useRef<HTMLUListElement>(null);
  const opener = useRef<HTMLElement | null>(null);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  // Opening the list asks the service; the answers shown count as read.
  const load = useCallback(
    (alive: () => boolean) => {
      const settle = (answer: MineAnswer): Promise<void> | undefined => {
        if (!alive()) return undefined;
        // A newer read is on screen: read again, so only answers shown count as read.
        if (!answer.applied) return refresh(false).then(settle);
        setChecking(false);
        const found = answer.found;
        if (found === null) return undefined;
        setFresh((old) => new Set([...old, ...unread(found)]));
        if (found.unseen > 0) {
          void ipc
            .reviewsSeen()
            .then(() => {
              onSeen();
            })
            .catch(() => undefined);
        }
        return undefined;
      };
      return refresh(true).then(settle);
    },
    [refresh, onSeen],
  );
  useEffect(() => {
    let current = true;
    void load(() => current);
    return () => {
      current = false;
    };
  }, [load]);

  const statusText = mine === null && error === null ? t.loading : checking ? t.checking : null;
  useEffect(() => {
    onStatus(statusText);
  }, [statusText, onStatus]);
  useEffect(
    () => () => {
      onStatus(null);
    },
    [onStatus],
  );

  // "Open" beside the form: that review, focused and in view.
  useEffect(() => {
    if (opening === null || mine === null) return;
    list.current?.querySelector<HTMLElement>(`[data-review="${String(opening)}"] h3`)?.focus();
    onOpened();
  }, [opening, mine, onOpened]);

  // A question that closes gives the focus back; a deleted review's goes to the list.
  useFocusBack(asking, opener, list, fallback);
  useFocusBack(mine === null, list, fallback);

  const remove = (review: SentReview) => {
    setRemoving(review.id);
    setFailed(null);
    ipc
      .reviewsDelete(review.id)
      .then(() => refresh(false))
      .catch((reason: unknown) => {
        setFailed({ id: review.id, text: describeIpcError(reason) });
      })
      .finally(() => {
        setRemoving(null);
        setAsking(null);
      });
  };

  if (mine === null) {
    // An error is said once an answer is in, each time anew; never the last one again
    // while the next check runs.
    return error === null || (checking && !retried) ? null : (
      <Panel>
        {!checking && <Notice tone="danger">{error}</Notice>}
        <div>
          <Button
            size="small"
            aria-disabled={checking || undefined}
            onClick={() => {
              if (checking) return;
              setChecking(true);
              setRetried(true);
              void load(() => mounted.current);
            }}
          >
            {t.retry}
          </Button>
        </div>
      </Panel>
    );
  }

  return (
    <Panel>
      {mine.waiting > 0 && <Notice tone="info">{t.waiting({ count: mine.waiting })}</Notice>}
      {mine.reviews.length === 0 ? (
        <p className={styles.hint}>{t.none}</p>
      ) : (
        <ul ref={list} className={styles.items} tabIndex={-1} aria-label={t.mineTab}>
          {mine.reviews.map((review) => (
            <MyReview
              key={review.id}
              review={review}
              fresh={fresh.has(review.id)}
              asking={asking === review.id}
              removing={removing === review.id}
              held={removing !== null}
              failed={failed?.id === review.id ? failed.text : null}
              onAsk={(button) => {
                opener.current = button;
                setFailed(null);
                setAsking(review.id);
              }}
              onYes={() => {
                remove(review);
              }}
              onNo={() => {
                setAsking(null);
              }}
            />
          ))}
        </ul>
      )}
    </Panel>
  );
}

interface MyReviewProps {
  review: SentReview;
  /** Its answer was not read when the list opened. */
  fresh: boolean;
  asking: boolean;
  removing: boolean;
  /** A delete is under way, this one's or another's. */
  held: boolean;
  failed: string | null;
  onAsk: (button: HTMLElement) => void;
  onYes: () => void;
  onNo: () => void;
}

function MyReview({
  review,
  fresh,
  asking,
  removing,
  held,
  failed,
  onAsk,
  onYes,
  onNo,
}: MyReviewProps) {
  const titleId = useId();
  const numberId = useId();
  const number = t.number({ id: review.id, date: dayOf(review.sentAt) });
  return (
    <li className={styles.item} data-review={review.id}>
      <div className={styles.itemHead}>
        <h3 id={titleId} className={styles.itemTitle} tabIndex={-1} dir="auto">
          {review.subject ?? placeOf(review)}
        </h3>
        <Tag tone={TAG_TONES[review.status]}>{t.statuses[review.status]}</Tag>
        {fresh && <Tag tone="warning">{t.fresh}</Tag>}
      </div>
      {/* Hidden, not gone, while its question is open: the focus comes back to it. */}
      <div className={styles.itemActions} hidden={asking}>
        <Button
          variant="ghost"
          size="small"
          aria-describedby={`${titleId} ${numberId}`}
          aria-disabled={held || undefined}
          onClick={(event) => {
            if (!held) onAsk(event.currentTarget);
          }}
        >
          {t.delete}
        </Button>
      </div>
      <p id={numberId} className={styles.itemMeta}>
        {review.subject !== null && `${placeOf(review)} · `}
        {number}
      </p>
      <p className={styles.excerpt} dir="auto">
        {review.excerpt}
      </p>
      {review.reply !== null && (
        <div className={styles.reply}>
          <p className={styles.replyHead}>
            {t.answer}
            {review.replied !== null ? ` · ${dayOf(review.replied)}` : ''}
          </p>
          <p dir="auto">{review.reply}</p>
        </div>
      )}
      {asking && (
        <div className={styles.itemWide}>
          <Confirm
            label={number}
            text={t.deleteConfirm}
            action={t.delete}
            cancel={t.cancel}
            pending={removing}
            onYes={onYes}
            onNo={onNo}
          />
        </div>
      )}
      {failed !== null && (
        <div className={styles.itemWide}>
          <Notice tone="danger">{failed}</Notice>
        </div>
      )}
    </li>
  );
}
