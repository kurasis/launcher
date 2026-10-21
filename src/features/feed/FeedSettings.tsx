import { useEffect, useId, useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
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
    <fieldset className={forms.choices} disabled={disabled}>
      <legend>{legend}</legend>
      <p className={styles.hint}>{hint}</p>
      <form
        className={forms.actions}
        onSubmit={(event) => {
          event.preventDefault();
          const value = input.trim();
          if (value === '') {
            setProblem('bad_input');
            return;
          }
          change(add(value), true);
        }}
      >
        <label htmlFor={inputId} className={styles.field}>
          {inputLabel}
          <input
            id={inputId}
            className={forms.input}
            value={input}
            maxLength={200}
            disabled={busy || full}
            spellCheck={false}
            autoComplete="off"
            dir="auto"
            onChange={(event) => {
              setInput(event.target.value);
            }}
          />
        </label>
        <Button type="submit" disabled={busy || full}>
          {busy ? t.adding : t.add}
        </Button>
      </form>
      {full && <p className={styles.hint}>{strings.feed.problems.full}</p>}
      {problem !== null && <Notice tone="danger">{problemText(problem)}</Notice>}
      {items.length === 0 ? (
        <p className={styles.hint}>{t.noChannels}</p>
      ) : (
        <>
          <p className={styles.hint}>
            {t.channelCount({ count: items.length, max: MAX_OWN_CHANNELS })}
          </p>
          <ul className={forms.choices}>
            {items.map((item) => (
              <li key={keyOf(item)} className={forms.actions}>
                <span dir="auto">{nameOf(item)}</span>
                <Button
                  disabled={busy}
                  aria-label={t.removeChannel({ title: nameOf(item) })}
                  onClick={() => {
                    change(remove(item), false);
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
  busy,
  onRemove,
}: {
  legend: string;
  hint: string;
  list: MarkList;
  marks: FeedMarks;
  busy: boolean;
  onRemove: (channel: ChannelMark) => void;
}) {
  const channels = list === 'hidden' ? marks.hidden : marks.favorite;
  return (
    <fieldset className={forms.choices}>
      <legend>{legend}</legend>
      <p className={styles.hint}>{hint}</p>
      {channels.length === 0 ? (
        <p className={styles.hint}>{t.noMarks}</p>
      ) : (
        <ul className={forms.choices}>
          {channels.map((channel) => (
            <li key={`${channel.kind}:${channel.id}`} className={forms.actions}>
              <span dir="auto">
                {channel.name}{' '}
                <span className={styles.hint}>{strings.feed.places[channel.kind]}</span>
              </span>
              <Button
                disabled={busy}
                aria-label={t.removeMark({ name: channel.name })}
                onClick={() => {
                  onRemove(channel);
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
  useEffect(() => {
    ipc
      .feedMarks()
      .then(setMarks)
      .catch(() => undefined);
  }, []);
  const remove = (channel: ChannelMark) => {
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
    <>
      <MarkedList
        legend={t.favorites}
        hint={t.favoritesHint}
        list="favorite"
        marks={marks}
        busy={busy || disabled}
        onRemove={remove}
      />
      <MarkedList
        legend={t.hiddenChannels}
        hint={t.hiddenChannelsHint}
        list="hidden"
        marks={marks}
        busy={busy || disabled}
        onRemove={remove}
      />
      {problem !== null && <Notice tone="danger">{strings.feed.problems[problem]}</Notice>}
    </>
  );
}

/** Settings → Feed (D-067). */
export function FeedSettings({ settings }: { settings: SettingsView }) {
  const feed = settings.feed;
  const update = useCommand((requestId: string, patch: Partial<Settings>) =>
    ipc.setAppSettings({ feed: patch }, requestId),
  );
  const disabled = update.pending || !settings.writable;
  const languageId = useId();

  const toggle = (
    name: 'enabled' | 'snapshots' | 'shorts' | 'machineTranslation' | 'liveNotifications',
    hint?: string,
  ) => (
    <label className={forms.choice}>
      <input
        type="checkbox"
        checked={feed[name]}
        onChange={(event) => {
          void update.run({ [name]: event.target.checked });
        }}
      />
      <span>
        {t[name]}
        {hint !== undefined && <span className={styles.hint}> {hint}</span>}
      </span>
    </label>
  );

  return (
    <div className={styles.section}>
      <fieldset className={forms.choices} disabled={disabled}>
        <legend>{t.showTitle}</legend>
        {toggle('enabled', t.enabledHint)}
        {toggle('snapshots')}
        {toggle('shorts')}
        {toggle('machineTranslation', t.machineTranslationHint)}
        {toggle('liveNotifications', t.liveNotificationsHint)}
      </fieldset>
      <fieldset className={forms.choices} disabled={disabled}>
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
                void update.run({ hidden: BLOCKS.filter((item) => hidden.includes(item)) });
              }}
            />
            {t.blocks[block]}
          </label>
        ))}
      </fieldset>
      <div className={styles.field}>
        <label htmlFor={languageId}>{t.videoLanguage}</label>
        <select
          id={languageId}
          className={forms.select}
          value={feed.videoLanguage}
          disabled={disabled}
          onChange={(event) => {
            const videoLanguage = LANGUAGES.find((item) => item === event.target.value);
            if (videoLanguage !== undefined) void update.run({ videoLanguage });
          }}
        >
          {LANGUAGES.map((item) => (
            <option key={item} value={item}>
              {t.videoLanguages[item]}
            </option>
          ))}
        </select>
      </div>
      <fieldset className={forms.choices} disabled={disabled}>
        <legend>{t.player}</legend>
        {PLAYERS.map((player) => (
          <label key={player} className={forms.choice}>
            <input
              type="radio"
              name="feedPlayer"
              value={player}
              checked={feed.player === player}
              onChange={() => {
                void update.run({ player });
              }}
            />
            {t.players[player]}
          </label>
        ))}
      </fieldset>
      {update.error !== null && <Notice tone="danger">{update.error}</Notice>}
      <OwnChannels disabled={!settings.writable} />
      <OwnStreamers disabled={!settings.writable} />
      <MarkedChannels disabled={!settings.writable} />
    </div>
  );
}
