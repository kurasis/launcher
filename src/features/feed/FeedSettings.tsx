import { useEffect, useId, useRef, useState, type RefObject } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { Switch } from '../../components/Switch';
import { useFocusBack } from '../../components/useFocusBack';
import forms from '../../components/forms.module.css';
import type {
  ChannelMark,
  FeedBlock,
  FeedMarks,
  FeedProblem,
  FeedSettings as Settings,
  MarkList,
  OwnChannel,
  OwnStreamer,
  PlayerMode,
  SettingsView,
  VideoLanguage,
} from '../../lib/contracts';
import { feedProblem, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import styles from '../settings/SettingsView.module.css';

const t = strings.feedSettings;
const LANGUAGES: readonly VideoLanguage[] = ['ui', 'en', 'ru', 'ar'];
const PLAYERS: readonly PlayerMode[] = ['window', 'browser'];
const BLOCKS: readonly FeedBlock[] = ['versions', 'news', 'videos', 'streams'];
/** The service's limit on a player's own channels. */
export const MAX_OWN_CHANNELS = 30;

interface OwnListProps<T> {
  disabled: boolean;
  legend: string;
  hint: string;
  inputLabel: string;
  load: () => Promise<T[]>;
  add: (input: string) => Promise<T[]>;
  remove: (item: T) => Promise<T[]>;
  keyOf: (item: T) => string;
  nameOf: (item: T) => string;
  /** What a problem says, where this list needs its own words. */
  problemText: (problem: FeedProblem) => string;
}

/** Channels the player added: a field to add one, and the list. */
function OwnList<T>({
  disabled,
  legend,
  hint,
  inputLabel,
  load,
  add,
  remove,
  keyOf,
  nameOf,
  problemText,
}: OwnListProps<T>) {
  const inputId = useId();
  const [items, setItems] = useState<T[]>([]);
  const [input, setInput] = useState('');
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<FeedProblem | null>(null);
  // A change keeps the field and the buttons as they are (a disabled control drops the
  // focus); when the channel removed took its button along, or the list is full, the
  // focus goes to the field, or to the count.
  const field = useRef<HTMLInputElement>(null);
  const count = useRef<HTMLParagraphElement>(null);
  useFocusBack(busy, field, count);

  useEffect(() => {
    load()
      .then(setItems)
      .catch(() => {
        setItems([]);
      });
  }, [load]);

  const full = items.length >= MAX_OWN_CHANNELS;
  const change = (request: Promise<T[]>, clear: boolean) => {
    setBusy(true);
    setProblem(null);
    request
      .then((list) => {
        setItems(list);
        if (clear) setInput('');
      })
      .catch((error: unknown) => {
        setProblem(feedProblem(error));
      })
      .finally(() => {
        setBusy(false);
      });
  };

  return (
    <Panel>
      <fieldset className={forms.choices} disabled={disabled}>
        <legend className={styles.legend}>{legend}</legend>
        <p className={styles.hint}>{hint}</p>
        <form
          className={styles.addRow}
          onSubmit={(event) => {
            event.preventDefault();
            if (busy || full) return;
            const value = input.trim();
            if (value === '') {
              setProblem('bad_input');
              return;
            }
            change(add(value), true);
          }}
        >
          <label htmlFor={inputId} className={styles.field}>
            <span className={styles.label}>{inputLabel}</span>
            <input
              ref={field}
              id={inputId}
              className={[forms.input, styles.long].join(' ')}
              value={input}
              maxLength={200}
              disabled={full}
              readOnly={busy}
              spellCheck={false}
              autoComplete="off"
              dir="auto"
              onChange={(event) => {
                setInput(event.target.value);
              }}
            />
          </label>
          <Button type="submit" disabled={full} aria-disabled={busy || undefined}>
            {busy ? t.adding : t.add}
          </Button>
        </form>
        {full && <p className={styles.hint}>{strings.feed.problems.full}</p>}
        {problem !== null && <Notice tone="danger">{problemText(problem)}</Notice>}
        {items.length === 0 ? (
          <p className={styles.hint}>{t.noChannels}</p>
        ) : (
          <>
            <p ref={count} className={[styles.hint, styles.status].join(' ')} tabIndex={-1}>
              {t.channelCount({ count: items.length, max: MAX_OWN_CHANNELS })}
            </p>
            <ul className={styles.items}>
              {items.map((item) => (
                <li key={keyOf(item)} className={styles.item}>
                  <span dir="auto">{nameOf(item)}</span>
                  <Button
                    size="small"
                    aria-disabled={busy || undefined}
                    aria-label={t.removeChannel({ title: nameOf(item) })}
                    onClick={() => {
                      if (!busy) change(remove(item), false);
                    }}
                  >
                    {t.remove}
                  </Button>
                </li>
              ))}
            </ul>
          </>
        )}
      </fieldset>
    </Panel>
  );
}

/** The YouTube channels the player added. */
function OwnChannels({ disabled }: { disabled: boolean }) {
  return (
    <OwnList<OwnChannel>
      disabled={disabled}
      legend={t.channels}
      hint={t.channelsHint}
      inputLabel={t.channelInput}
      load={ipc.feedChannels}
      add={ipc.feedAddChannel}
      remove={(channel) => ipc.feedRemoveChannel(channel.channelId)}
      keyOf={(channel) => channel.channelId}
      nameOf={(channel) => channel.title}
      problemText={(problem) => strings.feed.problems[problem]}
    />
  );
}

/** The Twitch channels the player added. */
function OwnStreamers({ disabled }: { disabled: boolean }) {
  return (
    <OwnList<OwnStreamer>
      disabled={disabled}
      legend={t.streamers}
      hint={t.streamersHint}
      inputLabel={t.streamerInput}
      load={ipc.feedStreamers}
      add={ipc.feedAddStreamer}
      remove={(streamer) => ipc.feedRemoveStreamer(streamer.login)}
      keyOf={(streamer) => streamer.login}
      nameOf={(streamer) => streamer.name}
      problemText={(problem) =>
        problem === 'bad_input'
          ? t.streamerBadInput
          : problem === 'not_found'
            ? t.streamerNotFound
            : strings.feed.problems[problem]
      }
    />
  );
}

/** One list of marked channels: their names and a button to take each off. */
function MarkedList({
  legend,
  hint,
  list,
  marks,
  disabled,
  busy,
  hintRef,
  onRemove,
}: {
  legend: string;
  hint: string;
  list: MarkList;
  marks: FeedMarks;
  disabled: boolean;
  busy: boolean;
  /** Where the focus goes when the channel's button goes with it. */
  hintRef: RefObject<HTMLParagraphElement | null>;
  onRemove: (channel: ChannelMark, list: MarkList) => void;
}) {
  const channels = list === 'hidden' ? marks.hidden : marks.favorite;
  return (
    <fieldset className={forms.choices}>
      <legend className={styles.legend}>{legend}</legend>
      <p ref={hintRef} className={[styles.hint, styles.status].join(' ')} tabIndex={-1}>
        {hint}
      </p>
      {channels.length === 0 ? (
        <p className={styles.hint}>{t.noMarks}</p>
      ) : (
        <ul className={styles.items}>
          {channels.map((channel) => (
            <li key={`${channel.kind}:${channel.id}`} className={styles.item}>
              <span dir="auto">
                {channel.name}{' '}
                <span className={styles.hint}>{strings.feed.places[channel.kind]}</span>
              </span>
              <Button
                size="small"
                disabled={disabled}
                aria-disabled={busy || undefined}
                aria-label={t.removeMark({ name: channel.name })}
                onClick={() => {
                  if (!busy) onRemove(channel, list);
                }}
              >
                {t.remove}
              </Button>
            </li>
          ))}
        </ul>
      )}
    </fieldset>
  );
}

/** The channels shown first and the ones not shown, set from a tile's ⋯ menu. */
function MarkedChannels({ disabled }: { disabled: boolean }) {
  const [marks, setMarks] = useState<FeedMarks>({ hidden: [], favorite: [] });
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<FeedProblem | null>(null);
  // The button pressed goes with its channel: the focus goes to the top of its list.
  const [from, setFrom] = useState<MarkList>('favorite');
  const favoriteHint = useRef<HTMLParagraphElement>(null);
  const hiddenHint = useRef<HTMLParagraphElement>(null);
  useFocusBack(busy, from === 'hidden' ? hiddenHint : favoriteHint);
  useEffect(() => {
    ipc
      .feedMarks()
      .then(setMarks)
      .catch(() => undefined);
  }, []);
  const remove = (channel: ChannelMark, list: MarkList) => {
    setFrom(list);
    setBusy(true);
    setProblem(null);
    ipc
      .feedMark(channel, null)
      .then(setMarks)
      .catch((error: unknown) => {
        setProblem(feedProblem(error));
      })
      .finally(() => {
        setBusy(false);
      });
  };
  return (
    <Panel>
      <MarkedList
        legend={t.favorites}
        hint={t.favoritesHint}
        list="favorite"
        marks={marks}
        disabled={disabled}
        busy={busy}
        hintRef={favoriteHint}
        onRemove={remove}
      />
      <MarkedList
        legend={t.hiddenChannels}
        hint={t.hiddenChannelsHint}
        list="hidden"
        marks={marks}
        disabled={disabled}
        busy={busy}
        hintRef={hiddenHint}
        onRemove={remove}
      />
      {problem !== null && <Notice tone="danger">{strings.feed.problems[problem]}</Notice>}
    </Panel>
  );
}

/** Settings → Feed (D-067). */
export function FeedSettings({ settings }: { settings: SettingsView }) {
  const feed = settings.feed;
  const update = useCommand((requestId: string, patch: Partial<Settings>) =>
    ipc.setAppSettings({ feed: patch }, requestId),
  );
  const disabled = !settings.writable;
  // A change being saved: the controls wait for it but keep the focus.
  const save = (patch: Partial<Settings>) => {
    if (!update.pending) void update.run(patch);
  };
  const languageId = useId();

  const toggle = (
    name: 'enabled' | 'snapshots' | 'shorts' | 'machineTranslation' | 'liveNotifications',
    hint?: string,
  ) => (
    <Switch
      label={t[name]}
      hint={hint}
      checked={feed[name]}
      disabled={disabled}
      pending={update.pending}
      onChange={(on) => {
        save({ [name]: on });
      }}
    />
  );

  return (
    <>
      <Panel title={t.showTitle} level={3}>
        {toggle('enabled', t.enabledHint)}
        {toggle('snapshots')}
        {toggle('shorts')}
        {toggle('machineTranslation', t.machineTranslationHint)}
        {toggle('liveNotifications', t.liveNotificationsHint)}
        <fieldset
          className={forms.choices}
          disabled={disabled}
          aria-busy={update.pending || undefined}
        >
          <legend>{t.hidden}</legend>
          {BLOCKS.map((block) => (
            <label key={block} className={forms.choice}>
              <input
                type="checkbox"
                checked={feed.hidden.includes(block)}
                onChange={(event) => {
                  const hidden = event.target.checked
                    ? [...feed.hidden, block]
                    : feed.hidden.filter((item) => item !== block);
                  save({ hidden: BLOCKS.filter((item) => hidden.includes(item)) });
                }}
              />
              {t.blocks[block]}
            </label>
          ))}
        </fieldset>
        {update.error !== null && <Notice tone="danger">{update.error}</Notice>}
      </Panel>
      <Panel>
        <div className={styles.field}>
          <label htmlFor={languageId} className={styles.label}>
            {t.videoLanguage}
          </label>
          <select
            id={languageId}
            className={[forms.select, styles.wide].join(' ')}
            value={feed.videoLanguage}
            disabled={disabled}
            aria-busy={update.pending || undefined}
            onChange={(event) => {
              const videoLanguage = LANGUAGES.find((item) => item === event.target.value);
              if (videoLanguage !== undefined) save({ videoLanguage });
            }}
          >
            {LANGUAGES.map((item) => (
              <option key={item} value={item}>
                {t.videoLanguages[item]}
              </option>
            ))}
          </select>
        </div>
        <fieldset
          className={forms.choices}
          disabled={disabled}
          aria-busy={update.pending || undefined}
        >
          <legend>{t.player}</legend>
          {PLAYERS.map((player) => (
            <label key={player} className={forms.choice}>
              <input
                type="radio"
                name="feedPlayer"
                value={player}
                checked={feed.player === player}
                onChange={() => {
                  save({ player });
                }}
              />
              {t.players[player]}
            </label>
          ))}
        </fieldset>
      </Panel>
      <OwnChannels disabled={disabled} />
      <OwnStreamers disabled={disabled} />
      <MarkedChannels disabled={disabled} />
    </>
  );
}
