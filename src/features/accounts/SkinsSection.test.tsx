import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
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
import { AccountView } from './AccountView';
import { SkinPreview } from './SkinPreview';
import { frameDurations, isWardrobe, parseFrameMs, skinParts } from './skinTextures';

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
    preview: PNG,
    ...overrides,
  };
}

type Answer = unknown;

/** Renders the Accounts screen with these answers from the core; a string answer
 * to a skins command is thrown, as the core rejects with a problem code. */
function setup(answers: Record<string, Answer> = {}, launcherAccount = view()) {
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
    if (cmd === 'skins_mine') return { skin: null, cape: null } satisfies Wardrobe;
    if (cmd === 'skins_texture') return PNG;
    return null;
  });
  const base = snapshotFixture();
  const account: AccountSummary = { ...base.account, launcherAccount };
  render(<AccountView snapshot={{ ...base, account }} />);
  return { calls };
}

describe('skin and cape', () => {
  it('says nothing is worn yet', async () => {
    setup();
    expect(screen.getByRole('region', { name: 'Skin and cape' })).toBeVisible();
    expect(await screen.findByText("No skin yet: the game's own is shown.")).toBeVisible();
    expect(screen.getByText('No cape.')).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Remove skin' })).not.toBeInTheDocument();
  });

  it('shows what is worn through the core, never from the network', async () => {
    const { calls } = setup({
      skins_mine: { skin: worn({ slim: true }), cape: worn({ height: 32 }) },
    });
    expect(await screen.findByRole('img', { name: 'Skin preview' })).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Cape preview' })).toBeInTheDocument();
    expect(calls.filter((c) => c.cmd === 'skins_texture').map((c) => c.args)).toContainEqual({
      hash: HASH,
    });
    expect(document.querySelector('img[src^="http"]')).toBeNull();
  });

  it('picks, checks and uploads an animated slim skin', async () => {
    const { calls } = setup({
      skins_pick: picked(),
      skins_upload: { skin: worn({ slim: true, height: 256, frames: 4 }), cape: null },
    });
    fireEvent.click(await screen.findByRole('button', { name: 'Upload skin…' }));
    expect(await screen.findByText('steve.png: 64×256, 4 frames')).toBeVisible();
    expect(calls.find((c) => c.cmd === 'skins_pick')?.args).toEqual({
      kind: 'skin',
      title: 'Choose a skin (PNG)',
    });
    expect(screen.getByRole('img', { name: 'Preview of the picked file' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('radio', { name: 'Slim, 3 pixels wide' }));
    fireEvent.change(screen.getByRole('spinbutton', { name: 'Frame duration, ms' }), {
      target: { value: '250' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Upload' }));
    expect(await screen.findByText('The skin is uploaded.')).toBeVisible();
    expect(calls.find((c) => c.cmd === 'skins_upload')?.args).toEqual({
      kind: 'skin',
      slim: true,
      frameMs: [250],
    });
    expect(screen.queryByText('steve.png: 64×256, 4 frames')).not.toBeInTheDocument();
  });

  it('refuses frame durations outside the rule before anything is sent', async () => {
    const { calls } = setup({ skins_pick: picked() });
    fireEvent.click(await screen.findByRole('button', { name: 'Upload skin…' }));
    const input = await screen.findByRole('spinbutton', { name: 'Frame duration, ms' });
    const upload = screen.getByRole('button', { name: 'Upload' });
    for (const junk of ['49', '10001', '', '-100', '1e3', '0x64']) {
      fireEvent.change(input, { target: { value: junk } });
      expect(upload).toBeDisabled();
    }
    fireEvent.change(input, { target: { value: '50' } });
    expect(upload).toBeEnabled();
    expect(calls.some((c) => c.cmd === 'skins_upload')).toBe(false);
  });

  it('uploads a cape with no model and no duration for one frame', async () => {
    const { calls } = setup({
      skins_pick: picked({ kind: 'cape', name: 'cape.png', height: 32, frames: 1 }),
      skins_upload: { skin: null, cape: worn({ height: 32 }) },
    });
    fireEvent.click(await screen.findByRole('button', { name: 'Upload cape…' }));
    expect(await screen.findByText('cape.png: 64×32, 1 frame')).toBeVisible();
    expect(screen.queryByRole('radio', { name: /pixels wide/ })).not.toBeInTheDocument();
    expect(screen.queryByRole('spinbutton')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Upload' }));
    expect(await screen.findByText('The cape is uploaded.')).toBeVisible();
    expect(calls.find((c) => c.cmd === 'skins_upload')?.args).toEqual({
      kind: 'cape',
      slim: false,
      frameMs: null,
    });
  });

  it('says why the core or the service refused', async () => {
    let refusal = 'bad_size';
    setup({
      skins_pick: () => (refusal === 'ok' ? picked({ frames: 1, height: 64 }) : refusal),
      skins_upload: () => refusal,
    });
    fireEvent.click(await screen.findByRole('button', { name: 'Upload skin…' }));
    expect(await screen.findByText(/This size is not taken\./)).toBeVisible();
    refusal = 'ok';
    fireEvent.click(screen.getByRole('button', { name: 'Upload skin…' }));
    await screen.findByText('steve.png: 64×64, 1 frame');
    refusal = 'rate_limited';
    fireEvent.click(screen.getByRole('button', { name: 'Upload' }));
    expect(
      await screen.findByText('Too many uploads: up to 10 an hour. Try again later.'),
    ).toBeVisible();
    // The pick stays for another try.
    expect(screen.getByText('steve.png: 64×64, 1 frame')).toBeVisible();
    refusal = '<img src=x onerror=alert(1)>';
    fireEvent.click(screen.getByRole('button', { name: 'Upload' }));
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
    expect(screen.getByText("No skin yet: the game's own is shown.")).toBeVisible();
    expect(screen.queryByRole('img', { name: 'Skin preview' })).not.toBeInTheDocument();
  });

  it('a picked file name is shown as text, whatever it holds', async () => {
    setup({ skins_pick: picked({ name: '<img src=x onerror=alert(1)>.png' }) });
    fireEvent.click(await screen.findByRole('button', { name: 'Upload skin…' }));
    expect(await screen.findByText(/<img src=x onerror=alert\(1\)>\.png/)).toBeVisible();
    expect(document.querySelector('img[src="x"]')).toBeNull();
  });

  it('a texture that is not a PNG data URL is not shown', async () => {
    setup({
      skins_mine: { skin: worn(), cape: null },
      skins_texture: 'javascript:alert(1)',
    });
    await waitFor(() => {
      expect(screen.queryByText('Loading…')).not.toBeInTheDocument();
    });
    expect(screen.queryByRole('img', { name: 'Skin preview' })).not.toBeInTheDocument();
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

describe('skin preview', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('draws the front of each frame in turn, at HD scale', () => {
    vi.useFakeTimers();
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
    vi.unstubAllGlobals();
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

  it('mirrors the right limbs for the old layout and narrows slim arms', () => {
    const legacy = skinParts(false, true);
    expect(legacy.filter((p) => p.mirror === true)).toHaveLength(2);
    expect(legacy.every((p) => p.sy < 32)).toBe(true);
    const slim = skinParts(true, false);
    expect(slim.filter((p) => p.w === 3)).toHaveLength(4);
  });

  it('knows a wardrobe from junk', () => {
    expect(isWardrobe({ skin: null, cape: null })).toBe(true);
    for (const junk of [null, undefined, 'x', 1, {}, { skin: null }]) {
      expect(isWardrobe(junk)).toBe(false);
    }
  });
});
