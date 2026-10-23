import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type {
  AccountSummary,
  LauncherAccountView,
  PickedTexture,
  Wardrobe,
  WornTexture,
} from '../../lib/contracts';
import { snapshotFixture } from '../../test/fixtures';
import { type Draft, durationsToSend, newDraft, trialTexture } from './draft';
import { SkinPreview } from './SkinPreview';
import { SkinsView } from './SkinsView';
import {
  dataUrlBytes,
  frameAt,
  frameDurations,
  isWardrobe,
  parseFrameMs,
  skinParts,
} from './textures';

const HASH = 'a'.repeat(64);
const PNG = 'data:image/png;base64,iVBORw0KGgo=';

function view(overrides: Partial<LauncherAccountView> = {}): LauncherAccountView {
  return {
    id: 'acc-1',
    nick: 'Steve_2',
    emailVerified: true,
    signingIn: false,
    needsSignIn: false,
    problem: null,
    ...overrides,
  };
}

function worn(overrides: Partial<WornTexture> = {}): WornTexture {
  return { hash: HASH, slim: false, width: 64, height: 64, frames: 1, frameMs: [], ...overrides };
}

function picked(overrides: Partial<PickedTexture> = {}): PickedTexture {
  return {
    kind: 'skin',
    name: 'steve.png',
    width: 64,
    height: 256,
    frames: 4,
    frameMs: [],
    source: 'png',
    notes: [],
    bytes: 3000,
    preview: PNG,
    slim: null,
    ...overrides,
  };
}

type Answer = unknown;

/** The core's answers: a string answer to a skins command is thrown, as the core
 * rejects with a problem code. */
function backend(answers: Record<string, Answer>) {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === 'get_app_snapshot') return snapshotFixture();
    if (cmd in answers) {
      const answer = answers[cmd];
      const value = typeof answer === 'function' ? (answer as () => unknown)() : answer;
      // eslint-disable-next-line @typescript-eslint/only-throw-error
      if (typeof value === 'string' && cmd !== 'skins_texture') throw value;
      return value;
    }
    if (cmd === 'skins_mine') return { skin: null, cape: null, uploads: null } satisfies Wardrobe;
    if (cmd === 'skins_texture') return PNG;
    return null;
  });
  return calls;
}

/** Renders the Skins tab with these answers from the core. */
function setup(answers: Record<string, Answer> = {}, launcherAccount = view()) {
  const calls = backend(answers);
  const base = snapshotFixture();
  const account: AccountSummary = { ...base.account, launcherAccount };
  render(<SkinsView snapshot={{ ...base, account }} onOpenAccounts={vi.fn()} />);
  return { calls };
}

function region() {
  return screen.getByRole('region', { name: 'Skin and cape' });
}

/** Drops files on the card of a kind. */
function drop(card: 'Skin' | 'Cape', files: File[]) {
  const heading = screen.getByRole('heading', { name: card });
  const target = heading.closest('[data-over]');
  if (target === null) throw new Error('no drop target');
  const dataTransfer = { types: ['Files'], files, dropEffect: 'none' };
  fireEvent.dragOver(target, { dataTransfer });
  fireEvent.drop(target, { dataTransfer });
}

describe('skin and cape', () => {
  it('says nothing is worn yet and shows a flat view without WebGL', async () => {
    setup();
    expect(region()).toBeVisible();
    expect(await screen.findAllByText("No skin yet: the game's own is shown.")).not.toHaveLength(0);
    expect(screen.getByText('No cape.')).toBeVisible();
    expect(
      screen.getByText('3D is not available on this computer, so a flat view is shown.'),
    ).toBeVisible();
    expect(screen.getByText('Worn now')).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Remove skin' })).not.toBeInTheDocument();
  });

  it('shows what is worn through the core, never from the network', async () => {
    const calls = setup({
      skins_mine: {
        skin: worn({ slim: true, width: 128, height: 128 }),
        cape: worn({ height: 32 }),
      },
    }).calls;
    expect(await screen.findByRole('img', { name: 'Skin preview' })).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Cape preview' })).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Skin from behind' })).toBeInTheDocument();
    expect(calls.filter((c) => c.cmd === 'skins_texture').map((c) => c.args)).toContainEqual({
      hash: HASH,
    });
    // The facts of the HD skin, under the character and on its card.
    expect(screen.getAllByText(/128×128 · HD ×2 · 1 frame · 1 KB/)).not.toHaveLength(0);
    expect(document.querySelector('img[src^="http"]')).toBeNull();
  });

  it('picks, tries on and wears an animated slim skin with a duration per frame', async () => {
    const { calls } = setup({
      skins_pick: picked({ frameMs: [100, 200, 300, 400] }),
      skins_upload: { skin: worn({ slim: true, height: 256, frames: 4 }), cape: null },
    });
    fireEvent.click(await screen.findByRole('button', { name: 'Upload skin…' }));
    const form = await screen.findByRole('form', { name: 'New skin' });
    expect(calls.find((c) => c.cmd === 'skins_pick')?.args).toEqual({
      kind: 'skin',
      title: 'Choose a skin (PNG, GIF or APNG)',
    });
    expect(within(form).getByText('steve.png')).toBeVisible();
    expect(within(form).getByText('4 frames in a strip from top to bottom')).toBeVisible();
    expect(screen.getByText('Trying on')).toBeVisible();
    // The file's own durations, and the cycle they make.
    expect(screen.getByRole('spinbutton', { name: 'Frame 2 duration, ms' })).toHaveValue(200);
    expect(within(form).getByText('4 frames, cycle 1 s')).toBeVisible();
    fireEvent.click(screen.getByRole('radio', { name: 'Slim, 3 pixels wide' }));
    fireEvent.change(screen.getByRole('spinbutton', { name: 'Frame 4 duration, ms' }), {
      target: { value: '250' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Wear' }));
    expect(await screen.findByText(/The skin is on\. Players see it when they join/)).toBeVisible();
    expect(calls.find((c) => c.cmd === 'skins_upload')?.args).toEqual({
      kind: 'skin',
      slim: true,
      frameMs: [100, 200, 300, 250],
    });
    expect(screen.queryByRole('form', { name: 'New skin' })).not.toBeInTheDocument();
    expect(screen.getByText('Worn now')).toBeVisible();
  });

  it('sends one duration when all frames share it', async () => {
    const { calls } = setup({
      skins_pick: picked({ frameMs: [120, 120, 120, 120] }),
      skins_upload: { skin: worn({ height: 256, frames: 4 }), cape: null },
    });
    fireEvent.click(await screen.findByRole('button', { name: 'Upload skin…' }));
    const same = await screen.findByRole('checkbox', { name: 'One duration for all' });
    expect(same).toBeChecked();
    fireEvent.change(screen.getByRole('spinbutton', { name: 'Each frame, ms' }), {
      target: { value: '300' },
    });
    // Every frame follows it.
    expect(screen.getByRole('spinbutton', { name: 'Frame 3 duration, ms' })).toHaveValue(300);
    fireEvent.click(screen.getByRole('button', { name: 'Wear' }));
    await screen.findByText(/The skin is on\./);
    expect(calls.find((c) => c.cmd === 'skins_upload')?.args).toEqual({
      kind: 'skin',
      slim: false,
      frameMs: [300],
    });
  });

  it('refuses frame durations outside the rule before anything is sent', async () => {
    const { calls } = setup({ skins_pick: picked({ frameMs: [100, 150, 100, 100] }) });
    fireEvent.click(await screen.findByRole('button', { name: 'Upload skin…' }));
    const input = await screen.findByRole('spinbutton', { name: 'Frame 1 duration, ms' });
    const wear = screen.getByRole('button', { name: 'Wear' });
    for (const junk of ['49', '10001', '', '-100', '1e3', '0x64']) {
      fireEvent.change(input, { target: { value: junk } });
      expect(wear).toBeDisabled();
    }
    fireEvent.change(input, { target: { value: '50' } });
    expect(wear).toBeEnabled();
    expect(calls.some((c) => c.cmd === 'skins_upload')).toBe(false);
  });

  it('wears a cape with no model and no duration for one frame', async () => {
    const { calls } = setup({
      skins_pick: picked({ kind: 'cape', name: 'cape.png', height: 32, frames: 1 }),
      skins_upload: { skin: null, cape: worn({ height: 32 }) },
    });
    fireEvent.click(await screen.findByRole('button', { name: 'Upload cape…' }));
    const form = await screen.findByRole('form', { name: 'New cape' });
    expect(within(form).getByText('One frame, no animation')).toBeVisible();
    expect(screen.queryByRole('radio', { name: /pixels wide/ })).not.toBeInTheDocument();
    expect(screen.queryByRole('spinbutton')).not.toBeInTheDocument();
    // A cape on the back can be seen as elytra too.
    fireEvent.click(screen.getByRole('button', { name: 'Elytra' }));
    expect(screen.getByRole('button', { name: 'Elytra' })).toHaveAttribute('aria-pressed', 'true');
    fireEvent.click(screen.getByRole('button', { name: 'Wear' }));
    expect(await screen.findByText(/The cape is on\./)).toBeVisible();
    expect(calls.find((c) => c.cmd === 'skins_upload')?.args).toEqual({
      kind: 'cape',
      slim: false,
      frameMs: null,
    });
  });

  it('says what importing a GIF changed', async () => {
    setup({
      skins_pick: picked({
        source: 'gif',
        name: 'neon.gif',
        width: 256,
        height: 1024,
        frames: 4,
        frameMs: [100, 50, 50, 10_000],
        bytes: 400 * 1024,
        notes: [
          { kind: 'everyNth', from: 8, to: 4, step: 2 },
          { kind: 'clamped', frame: 1, was: 20, now: 50 },
          { kind: 'clamped', frame: 2, was: 30, now: 50 },
          { kind: 'clamped', frame: 3, was: 12_000, now: 10_000 },
          { kind: 'clamped', frame: 0, was: 49, now: 50 },
          { kind: 'clamped', frame: 9, was: 1, now: 50 },
        ],
      }),
    });
    fireEvent.click(await screen.findByRole('button', { name: 'Upload skin…' }));
    const form = await screen.findByRole('form', { name: 'New skin' });
    for (const text of [
      'Frame 256×256, HD ×4',
      '400 KB of 2,048 KB',
      'The GIF became a strip of frames with its own durations.',
      'Frame 2 lasted 20 ms; the game shows it for 50 ms.',
      'Frame 4 lasted 12000 ms; the game shows it for 10000 ms.',
      '2 more frames were brought within 50–10000 ms.',
    ]) {
      expect(within(form).getByText(text)).toBeVisible();
    }
    expect(within(form).getByText(/every 2nd frame was kept \(4\)/)).toBeVisible();
  });

  it('takes a dropped file through the core, by its contents', async () => {
    const { calls } = setup({
      skins_take: picked({ kind: 'cape', name: 'drop.gif', height: 64, frames: 2, source: 'gif' }),
    });
    await screen.findByRole('button', { name: 'Upload cape…' });
    drop('Cape', [new File([new Uint8Array([71, 73, 70, 56, 57, 97])], 'drop.gif')]);
    expect(await screen.findByRole('form', { name: 'New cape' })).toBeVisible();
    expect(calls.find((c) => c.cmd === 'skins_take')?.args).toEqual({
      kind: 'cape',
      name: 'drop.gif',
      data: 'R0lGODlh',
    });
  });

  it('refuses a dropped file of another kind or too heavy before reading it', async () => {
    const { calls } = setup();
    await screen.findByRole('button', { name: 'Upload skin…' });
    drop('Skin', [new File(['<svg onload=alert(1)>'], 'skin.svg')]);
    expect(
      await screen.findByText('This is not a PNG, GIF or APNG image, or the file is damaged.'),
    ).toBeVisible();
    const heavy = new File(['x'], 'big.png');
    Object.defineProperty(heavy, 'size', { value: 9 * 1024 * 1024 });
    drop('Skin', [heavy]);
    expect(await screen.findByText(/The file is too large/)).toBeVisible();
    expect(calls.some((c) => c.cmd === 'skins_take')).toBe(false);
  });

  it('says why the core or the service refused', async () => {
    let refusal = 'too_many_frames';
    setup({
      skins_pick: () => (refusal === 'ok' ? picked({ frames: 1, height: 64 }) : refusal),
      skins_upload: () => refusal,
    });
    fireEvent.click(await screen.findByRole('button', { name: 'Upload skin…' }));
    expect(await screen.findByText(/The animation is too long/)).toBeVisible();
    refusal = 'ok';
    fireEvent.click(screen.getByRole('button', { name: 'Upload skin…' }));
    await screen.findByRole('form', { name: 'New skin' });
    refusal = 'rate_limited';
    fireEvent.click(screen.getByRole('button', { name: 'Wear' }));
    expect(
      await screen.findByText('Too many uploads: up to 10 an hour. Try again later.'),
    ).toBeVisible();
    // The pick stays for another try.
    expect(screen.getByRole('form', { name: 'New skin' })).toBeVisible();
    refusal = '<img src=x onerror=alert(1)>';
    fireEvent.click(screen.getByRole('button', { name: 'Wear' }));
    expect(
      await screen.findByText('Something went wrong on the skins server. Try again later.'),
    ).toBeVisible();
    expect(document.querySelector('img[src="x"]')).toBeNull();
  });

  it('says when the account must sign in again', async () => {
    setup({ skins_mine: 'signed_out' });
    expect(await screen.findByText('The sign-in has ended. Sign in again.')).toBeVisible();
  });

  it('removes the skin', async () => {
    const { calls } = setup({
      skins_mine: { skin: worn(), cape: null },
      skins_remove: { skin: null, cape: null },
    });
    fireEvent.click(await screen.findByRole('button', { name: 'Remove skin' }));
    expect(await screen.findByText('The skin is removed.')).toBeVisible();
    expect(calls.find((c) => c.cmd === 'skins_remove')?.args).toEqual({ kind: 'skin' });
    expect(screen.queryByRole('img', { name: 'Skin preview' })).not.toBeInTheDocument();
  });

  it('a picked file name is shown as text, whatever it holds', async () => {
    setup({ skins_pick: picked({ name: '<img src=x onerror=alert(1)>.png' }) });
    fireEvent.click(await screen.findByRole('button', { name: 'Upload skin…' }));
    expect(await screen.findByText('<img src=x onerror=alert(1)>.png')).toBeVisible();
    expect(document.querySelector('img[src="x"]')).toBeNull();
  });

  it('a texture that is not a PNG data URL is not shown', async () => {
    setup({
      skins_mine: { skin: worn(), cape: null },
      skins_texture: 'javascript:alert(1)',
    });
    expect(await screen.findByText('Worn, but its picture could not be loaded.')).toBeVisible();
    expect(screen.queryByRole('img', { name: 'Skin preview' })).not.toBeInTheDocument();
  });

  it('shows how players without the mod see the account', async () => {
    setup({ skins_mine: { skin: worn(), cape: worn({ height: 32 }) } });
    await screen.findByRole('img', { name: 'Skin preview' });
    fireEvent.click(screen.getByRole('switch', { name: 'Without the mod' }));
    expect(screen.getByText('Seen by players without the mod')).toBeVisible();
    expect(screen.getByText(/see the game's own skin and no cape/)).toBeVisible();
    expect(screen.queryByRole('img', { name: 'Cape from behind' })).not.toBeInTheDocument();
  });

  it('is not offered until the account is signed in', () => {
    setup({}, view({ id: null, nick: null }));
    expect(screen.queryByRole('region', { name: 'Skin and cape' })).not.toBeInTheDocument();
  });

  it('is not offered when the sign-in ended', () => {
    setup({}, view({ needsSignIn: true, problem: 'sign_in_again' }));
    expect(screen.queryByRole('region', { name: 'Skin and cape' })).not.toBeInTheDocument();
  });
});

describe('skins tab', () => {
  function tab(launcherAccount: LauncherAccountView) {
    backend({});
    const base = snapshotFixture();
    const onOpenAccounts = vi.fn();
    render(
      <SkinsView
        snapshot={{ ...base, account: { ...base.account, launcherAccount } }}
        onOpenAccounts={onOpenAccounts}
      />,
    );
    return onOpenAccounts;
  }

  it('is the same screen for a signed-in account', async () => {
    tab(view());
    expect(await screen.findByRole('button', { name: 'Upload skin…' })).toBeVisible();
    expect(region()).toBeVisible();
  });

  it('sends a player without an account to Accounts', () => {
    const open = tab(view({ id: null, nick: null }));
    expect(screen.getByText(/Skins and capes belong to a launcher account/)).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Open Accounts' }));
    expect(open).toHaveBeenCalled();
  });
});

describe('skin preview', () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  function drawing() {
    const drawn: number[][] = [];
    const context = {
      clearRect: vi.fn(),
      save: vi.fn(),
      restore: vi.fn(),
      translate: vi.fn(),
      scale: vi.fn(),
      imageSmoothingEnabled: true,
      drawImage: (_: unknown, ...rect: number[]) => drawn.push(rect),
    };
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(
      context as unknown as CanvasRenderingContext2D,
    );
    const images: { onload: (() => void) | null }[] = [];
    vi.stubGlobal(
      'Image',
      class {
        onload: (() => void) | null = null;
        src = '';
        constructor() {
          images.push(this);
        }
      },
    );
    return { drawn, context, images };
  }

  it('draws the front of each frame in turn, at HD scale', () => {
    vi.useFakeTimers();
    const { drawn, context, images } = drawing();
    render(
      <SkinPreview
        src={PNG}
        kind="skin"
        slim={false}
        width={128}
        height={256}
        frames={2}
        frameMs={[300]}
        label="Skin preview"
      />,
    );
    act(() => images[0]?.onload?.());
    // The head's front of the first frame, twice the standard size.
    expect(drawn[0]).toEqual([16, 16, 16, 16, 32, 0, 64, 64]);
    expect(context.imageSmoothingEnabled).toBe(false);
    drawn.length = 0;
    act(() => {
      vi.advanceTimersByTime(300);
    });
    // The second frame starts 128 pixels lower.
    expect(drawn[0]).toEqual([16, 144, 16, 16, 32, 0, 64, 64]);
  });

  it('holds one frame and draws the back', () => {
    vi.useFakeTimers();
    const { drawn, images } = drawing();
    render(
      <SkinPreview
        src={PNG}
        kind="skin"
        slim={false}
        width={64}
        height={192}
        frames={3}
        frameMs={[100]}
        label="Skin from behind"
        side="back"
        frame={2}
      />,
    );
    act(() => images[0]?.onload?.());
    // The back of the head, in the third frame.
    expect(drawn[0]).toEqual([24, 136, 8, 8, 32, 0, 64, 64]);
    drawn.length = 0;
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(drawn).toHaveLength(0);
  });
});

describe('skin textures', () => {
  it('reads frame durations strictly', () => {
    expect(parseFrameMs('100')).toBe(100);
    expect(parseFrameMs(' 50 ')).toBe(50);
    expect(parseFrameMs('10000')).toBe(10_000);
    for (const junk of [
      '49',
      '10001',
      '',
      '1.5',
      '-1',
      '1e3',
      '0x10',
      '१००',
      '100ms',
      '99999999',
    ]) {
      expect(parseFrameMs(junk)).toBeNull();
    }
  });

  it('gives every frame a duration within the rule', () => {
    expect(frameDurations(3, [])).toEqual([100, 100, 100]);
    expect(frameDurations(3, [250])).toEqual([250, 250, 250]);
    expect(frameDurations(2, [10, 99999])).toEqual([50, 10_000]);
    expect(frameDurations(2, [Number.NaN])).toEqual([100, 100]);
    expect(frameDurations(1000, [])).toHaveLength(32);
    expect(frameDurations(-1, [])).toEqual([100]);
  });

  it('follows the wall clock, as the mod does', () => {
    const durations = [100, 200, 300];
    expect(frameAt(durations, 0)).toBe(0);
    expect(frameAt(durations, 99)).toBe(0);
    expect(frameAt(durations, 100)).toBe(1);
    expect(frameAt(durations, 299)).toBe(1);
    expect(frameAt(durations, 300)).toBe(2);
    expect(frameAt(durations, 600)).toBe(0);
    expect(frameAt(durations, 1_700_000_000_350)).toBe(2);
    expect(frameAt([100], 5000)).toBe(0);
    expect(frameAt([], 5000)).toBe(0);
  });

  it('mirrors the right limbs for the old layout and narrows slim arms', () => {
    const legacy = skinParts(false, true);
    expect(legacy.filter((p) => p.mirror === true)).toHaveLength(2);
    expect(legacy.every((p) => p.sy < 32)).toBe(true);
    const slim = skinParts(true, false);
    expect(slim.filter((p) => p.w === 3)).toHaveLength(4);
    // From behind, the right arm is on the right and its back face follows its width.
    const back = skinParts(true, false, 'back');
    expect(back).toContainEqual({ sx: 51, sy: 20, w: 3, h: 12, dx: 12, dy: 8 });
    expect(back).toContainEqual({ sx: 43, sy: 52, w: 3, h: 12, dx: 1, dy: 8 });
    expect(skinParts(false, true, 'back').filter((p) => p.mirror === true)).toHaveLength(2);
  });

  it('knows a wardrobe from junk', () => {
    expect(isWardrobe({ skin: null, cape: null })).toBe(true);
    for (const junk of [null, undefined, 'x', 1, {}, { skin: null }]) {
      expect(isWardrobe(junk)).toBe(false);
    }
  });

  it('knows the weight of a data URL', () => {
    expect(dataUrlBytes('data:image/png;base64,AAAA')).toBe(3);
    expect(dataUrlBytes('data:image/png;base64,AAA=')).toBe(2);
    expect(dataUrlBytes('data:image/png;base64,AA==')).toBe(1);
    expect(dataUrlBytes('junk')).toBe(0);
  });

  it('sends nothing, one duration or one per frame', () => {
    const draft: Draft = newDraft(picked({ frameMs: [100, 200, 100, 100] }), false);
    expect(draft.same).toBe(false);
    expect(durationsToSend(draft)).toEqual([100, 200, 100, 100]);
    expect(durationsToSend({ ...draft, same: true, sameText: '70' })).toEqual([70]);
    expect(durationsToSend({ ...draft, texts: ['100', '100', '100', '100'] })).toEqual([100]);
    expect(durationsToSend({ ...draft, texts: ['100', 'x', '100', '100'] })).toBeUndefined();
    expect(durationsToSend(newDraft(picked({ frames: 1, height: 64 }), false))).toBeNull();
    // A file without durations starts at the default, and tries on with it.
    const plain = newDraft(picked(), true);
    expect(plain.texts).toEqual(['100', '100', '100', '100']);
    expect(trialTexture(plain)).toMatchObject({ slim: true, frameMs: [100, 100, 100, 100] });
    expect(trialTexture(newDraft(picked({ kind: 'cape', height: 128 }), true)).slim).toBe(false);
  });
});

describe('the core', () => {
  it('is asked for nothing it does not need when the screen opens', async () => {
    const { calls } = setup();
    await waitFor(() => {
      expect(calls.some((c) => c.cmd === 'skins_mine')).toBe(true);
    });
    expect(calls.some((c) => c.cmd === 'skins_take' || c.cmd === 'skins_pick')).toBe(false);
  });
});
