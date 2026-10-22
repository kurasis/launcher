import { render, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { SkinHead } from './SkinHead';

const PNG = 'data:image/png;base64,AAAA';

/** An image that "loads" with the given size, or fails, a moment after its src is set. */
function fakeImage(width: number, height: number, fails = false) {
  return class {
    naturalWidth = width;
    naturalHeight = height;
    onload: (() => void) | null = null;
    onerror: (() => void) | null = null;
    set src(_value: string) {
      setTimeout(() => {
        if (fails) this.onerror?.();
        else this.onload?.();
      }, 0);
    }
  };
}

describe('SkinHead', () => {
  const drawImage = vi.fn();

  beforeEach(() => {
    drawImage.mockClear();
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue({
      imageSmoothingEnabled: true,
      clearRect: vi.fn(),
      drawImage,
    } as unknown as CanvasRenderingContext2D);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('draws the default face without a skin', () => {
    const { container } = render(<SkinHead texture={null} />);
    expect(container.querySelector('svg')).not.toBeNull();
    expect(container.querySelector('canvas')).toBeNull();
  });

  it('draws the face and the hat of an old 64×32 skin', async () => {
    vi.stubGlobal('Image', fakeImage(64, 32));
    const { container } = render(<SkinHead texture={PNG} />);
    expect(container.querySelector('canvas')).not.toBeNull();
    await waitFor(() => {
      expect(drawImage).toHaveBeenCalledTimes(2);
    });
    expect(drawImage.mock.calls[0]?.slice(1)).toEqual([8, 8, 8, 8, 0, 0, 8, 8]);
    expect(drawImage.mock.calls[1]?.slice(1)).toEqual([40, 8, 8, 8, 0, 0, 8, 8]);
  });

  it('falls back to the default face when the skin will not load or has no head', async () => {
    vi.stubGlobal('Image', fakeImage(64, 64, true));
    const { container, rerender } = render(<SkinHead texture={PNG} />);
    await waitFor(() => {
      expect(container.querySelector('svg')).not.toBeNull();
    });
    vi.stubGlobal('Image', fakeImage(64, 8));
    rerender(<SkinHead texture={`${PNG}BB`} />);
    await waitFor(() => {
      expect(container.querySelector('svg')).not.toBeNull();
    });
    expect(drawImage).not.toHaveBeenCalled();
  });
});
