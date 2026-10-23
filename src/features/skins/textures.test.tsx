import { act, render } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { PickedTexture } from '../../lib/contracts';
import { type Draft, durationsToSend, newDraft, trialTexture } from './draft';
import { SkinPreview } from './SkinPreview';
import {
  dataUrlBytes,
  frameAt,
  frameDurations,
  isWardrobe,
  parseFrameMs,
  skinParts,
} from './textures';

const PNG = 'data:image/png;base64,iVBORw0KGgo=';

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
