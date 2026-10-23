import { type DragEvent, type ReactNode, useEffect, useId, useRef, useState } from 'react';
import { type MenuItem, MoreMenu } from '../../app/MoreMenu';
import { Button } from '../../components/Button';
import { Icon } from '../../components/Icon';
import forms from '../../components/forms.module.css';
import { type SkinKind, type WornTexture, WARDROBE_NAME_CHARS } from '../../lib/contracts';
import { strings } from '../../lib/strings';
import { SkinPreview } from './SkinPreview';
import styles from './Skins.module.css';
import { firstChars, textureOf, wardrobeName } from './closet';

const w = strings.accounts.skins.wardrobe;

/** A texture drawn still on a card (its first frame), or why it is not. */
export function Picture({
  kind,
  texture,
  src,
}: {
  kind: SkinKind;
  texture: WornTexture | null;
  src: string | undefined;
}) {
  const drawable = textureOf(kind, texture, src);
  if (drawable !== null) return <SkinPreview {...drawable} frame={0} small label="" />;
  if (texture === null) return null;
  return (
    <span className={styles.pictureWait}>
      {src === '' ? w.noPicture : strings.accounts.skins.loading}
    </span>
  );
}

interface CardProps {
  /** The name shown (the one given, or the screen's). */
  name: string;
  /** The name as kept, to rename from. */
  kept: string;
  picture: ReactNode;
  chips: readonly string[];
  favourite: boolean;
  worn: boolean;
  trying: boolean;
  /** A change is on its way: the card's actions wait. */
  busy: boolean;
  /** Its own entries in the menu, before Rename and Remove. */
  entries: readonly MenuItem[];
  onTry: () => void;
  onFavourite: () => void;
  /** Whether the new name was kept. */
  onRename: (name: string) => Promise<boolean>;
  onRemove: () => void;
}

/**
 * A skin, cape or set in the wardrobe: pressed, it is tried on; the star marks it;
 * its menu wears, renames or removes it (asking first).
 */
export function WardrobeCard({
  name,
  kept,
  picture,
  chips,
  favourite,
  worn,
  trying,
  busy,
  entries,
  onTry,
  onFavourite,
  onRename,
  onRemove,
}: CardProps) {
  const [mode, setMode] = useState<'rename' | 'remove' | null>(null);
  const main = useRef<HTMLButtonElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  const backToCard = useRef(false);
  // The question takes the focus once, when it opens; closing the form or the question
  // gives it back to the card.
  useEffect(() => {
    if (mode === 'remove') {
      cancel.current?.focus();
    } else if (mode === null && backToCard.current) {
      backToCard.current = false;
      main.current?.focus();
    }
  }, [mode]);
  const close = () => {
    backToCard.current = true;
    setMode(null);
  };
  return (
    <li className={styles.card} data-trying={trying || undefined}>
      <button
        ref={main}
        type="button"
        className={styles.cardTry}
        aria-pressed={trying}
        aria-label={w.tryOn({ name })}
        onClick={onTry}
      >
        <span className={styles.cardPicture} aria-hidden="true">
          {picture}
        </span>
        <span className={styles.cardName}>
          <span className={styles.cardNameText} dir="auto">
            {name}
          </span>
          {trying && <span className={styles.tryTag}>{w.tryingTag}</span>}
        </span>
      </button>
      {worn && <span className={styles.wornTag}>{w.wornTag}</span>}
      <button
        type="button"
        className={styles.star}
        aria-pressed={favourite}
        aria-label={w.favourite}
        title={w.favourite}
        aria-disabled={busy || undefined}
        onClick={() => {
          if (!busy) onFavourite();
        }}
      >
        <Icon name="star" size={16} />
      </button>
      <div className={[styles.cardFoot, styles.withMenu].filter(Boolean).join(' ')}>
        <ul className={styles.cardChips}>
          {chips.map((chip) => (
            <li key={chip} dir="auto">
              {chip}
            </li>
          ))}
        </ul>
        <div className={styles.cardMenu}>
          <MoreMenu
            label={w.actions({ name })}
            ariaLabel={w.actions({ name })}
            icon={<Icon name="more" size={18} />}
            caret={false}
            current={false}
            buttonClassName={styles.menuButton}
            entries={[
              ...entries,
              {
                kind: 'item',
                key: 'rename',
                label: w.rename,
                onSelect: () => {
                  setMode('rename');
                },
              },
              {
                kind: 'item',
                key: 'remove',
                label: w.remove,
                onSelect: () => {
                  setMode('remove');
                },
              },
            ]}
          />
        </div>
      </div>
      {mode === 'rename' && (
        <RenameForm
          kept={kept}
          busy={busy}
          onSave={async (next) => {
            if (await onRename(next)) close();
          }}
          onClose={close}
        />
      )}
      {mode === 'remove' && (
        <div
          className={styles.cardConfirm}
          role="alertdialog"
          aria-label={w.removeQuestion({ name })}
        >
          <p dir="auto">{w.removeQuestion({ name })}</p>
          <div className={forms.actions}>
            <Button
              variant="primary"
              size="small"
              aria-disabled={busy || undefined}
              onClick={() => {
                if (busy) return;
                onRemove();
              }}
            >
              {w.removeYes}
            </Button>
            <Button ref={cancel} size="small" onClick={close}>
              {strings.accounts.skins.cancel}
            </Button>
          </div>
        </div>
      )}
    </li>
  );
}

/** A new name for a card: up to 32 characters, without hidden ones (the service's rule). */
function RenameForm({
  kept,
  busy,
  onSave,
  onClose,
}: {
  kept: string;
  busy: boolean;
  onSave: (name: string) => Promise<void>;
  onClose: () => void;
}) {
  const [text, setText] = useState(kept);
  const badId = useId();
  const name = wardrobeName(text);
  return (
    <form
      className={styles.cardRename}
      onSubmit={(event) => {
        event.preventDefault();
        if (busy || name === null) return;
        if (name === kept) onClose();
        else void onSave(name);
      }}
    >
      <label className={styles.cardRenameLabel}>
        {w.nameLabel}
        <input
          className={forms.input}
          value={text}
          dir="auto"
          autoComplete="off"
          spellCheck={false}
          aria-invalid={name === null}
          aria-describedby={name === null ? badId : undefined}
          // Opened on purpose from the menu: typing starts here.
          autoFocus
          onChange={(event) => {
            // Cut by characters, not by UTF-16 units, so a cut never splits one.
            setText(firstChars(event.target.value, WARDROBE_NAME_CHARS + 1));
          }}
          onKeyDown={(event) => {
            if (event.key === 'Escape') {
              event.preventDefault();
              event.stopPropagation();
              onClose();
            }
          }}
        />
      </label>
      {name === null && (
        <p id={badId} className={styles.bad}>
          {strings.accounts.skins.problems.bad_name}
        </p>
      )}
      <div className={forms.actions}>
        <Button type="submit" variant="primary" size="small" aria-disabled={busy || undefined}>
          {w.save}
        </Button>
        <Button size="small" onClick={onClose}>
          {strings.accounts.skins.cancel}
        </Button>
      </div>
    </form>
  );
}

/** The first card of a tab: a file from the dialog or dropped on it, or one taken by a
 * friend's nick. */
export function UploadCard({
  kind,
  busy,
  onPick,
  onDrop,
  onByNick,
}: {
  kind: SkinKind;
  busy: boolean;
  onPick: () => void;
  onDrop: (file: File) => void;
  onByNick: () => void;
}) {
  const [over, setOver] = useState(false);
  const accepts = (event: DragEvent) => !busy && event.dataTransfer.types.includes('Files');
  return (
    <li
      className={styles.uploadCard}
      data-over={over}
      onDragOver={(event) => {
        if (!accepts(event)) return;
        event.preventDefault();
        event.dataTransfer.dropEffect = 'copy';
        setOver(true);
      }}
      onDragLeave={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setOver(false);
      }}
      onDrop={(event) => {
        event.preventDefault();
        setOver(false);
        const file = event.dataTransfer.files[0];
        if (!busy && file !== undefined) onDrop(file);
      }}
    >
      <button
        type="button"
        className={styles.uploadButton}
        aria-disabled={busy || undefined}
        onClick={() => {
          if (!busy) onPick();
        }}
      >
        <Icon name="upload" size={20} />
        <span className={styles.uploadTitle}>{w.upload[kind]}</span>
        <span className={styles.hint}>{w.uploadHint}</span>
      </button>
      <button type="button" className={styles.link} onClick={onByNick}>
        {w.byFriend}
      </button>
    </li>
  );
}
