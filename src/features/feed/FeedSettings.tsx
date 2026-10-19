import { useEffect, useId, useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type {
  FeedBlock,
  FeedProblem,
  FeedSettings as Settings,
  OwnChannel,
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

/** The YouTube channels the player added: a field to add one, and the list. */
function OwnChannels({ disabled }: { disabled: boolean }) {
  const inputId = useId();
  const [channels, setChannels] = useState<OwnChannel[]>([]);
  const [input, setInput] = useState('');
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<FeedProblem | null>(null);

  useEffect(() => {
    ipc
      .feedChannels()
      .then(setChannels)
      .catch(() => {
        setChannels([]);
      });
  }, []);

  const full = channels.length >= MAX_OWN_CHANNELS;
  const change = (request: Promise<OwnChannel[]>, clear: boolean) => {
    setBusy(true);
    setProblem(null);
    request
      .then((list) => {
        setChannels(list);
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
      <legend>{t.channels}</legend>
      <p className={styles.hint}>{t.channelsHint}</p>
      <form
        className={forms.actions}
        onSubmit={(event) => {
          event.preventDefault();
          const value = input.trim();
          if (value === '') {
            setProblem('bad_input');
            return;
          }
          change(ipc.feedAddChannel(value), true);
        }}
      >
        <label htmlFor={inputId} className={styles.field}>
          {t.channelInput}
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
      {problem !== null && <Notice tone="danger">{strings.feed.problems[problem]}</Notice>}
      {channels.length === 0 ? (
        <p className={styles.hint}>{t.noChannels}</p>
      ) : (
        <>
          <p className={styles.hint}>
            {t.channelCount({ count: channels.length, max: MAX_OWN_CHANNELS })}
          </p>
          <ul className={forms.choices}>
            {channels.map((channel) => (
              <li key={channel.channelId} className={forms.actions}>
                <span dir="auto">{channel.title}</span>
                <Button
                  disabled={busy}
                  aria-label={t.removeChannel({ title: channel.title })}
                  onClick={() => {
                    change(ipc.feedRemoveChannel(channel.channelId), false);
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

/** Settings → Feed (D-067). */
export function FeedSettings({ settings }: { settings: SettingsView }) {
  const feed = settings.feed;
  const update = useCommand((requestId: string, patch: Partial<Settings>) =>
    ipc.setAppSettings({ feed: patch }, requestId),
  );
  const disabled = update.pending || !settings.writable;
  const languageId = useId();

  const toggle = (
    name: 'enabled' | 'snapshots' | 'shorts' | 'machineTranslation',
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
    </div>
  );
}
