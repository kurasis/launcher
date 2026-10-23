import { fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { Avatar, Cell, LetterAvatar, Mosaic } from './Card';
import { ASPECT, avatarTone, initial, pictureAspect, rowSpan } from './mosaic';

describe('mosaic', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('spans as many small rows as a card is tall, its gap included', () => {
    expect(rowSpan(100, 16, 4)).toBe(29);
    expect(rowSpan(101, 16, 4)).toBe(30);
    // Nothing to measure (a test, a hidden placeholder): one row.
    expect(rowSpan(0, 16, 4)).toBe(1);
    expect(rowSpan(Number.NaN, 16, 4)).toBe(1);
    expect(rowSpan(100, 16, 0)).toBe(1);
  });

  it('keeps pictures between 4:5 and 16:9', () => {
    expect(pictureAspect(1600, 900)).toBeCloseTo(16 / 9);
    expect(pictureAspect(1200, 1200)).toBe(1);
    expect(pictureAspect(1080, 1350)).toBeCloseTo(4 / 5);
    // Taller than 4:5 or wider than 16:9 is cropped to them.
    expect(pictureAspect(600, 1600)).toBe(ASPECT.tall);
    expect(pictureAspect(1920, 600)).toBe(ASPECT.wide);
    expect(pictureAspect(null, null)).toBe(ASPECT.wide);
    expect(pictureAspect(0, 720)).toBe(ASPECT.wide);
  });

  it('gives a channel the same letter and color wherever it shows', () => {
    expect(initial('builder')).toBe('B');
    expect(initial('  ёжик')).toBe('Ё');
    expect(initial('')).toBe('?');
    expect(initial('👩‍🚀 Space')).toBe('👩‍🚀');
    expect(avatarTone('Builder')).toBe(avatarTone('Builder'));
    const tones = new Set(['Builder', 'Speedy', 'Mine', 'Ph1LzA', 'Redstone Lab'].map(avatarTone));
    expect(tones.size).toBeGreaterThan(1);
    for (const tone of tones) expect(tone).toBeLessThan(5);
  });

  it('draws a letter when a channel has no picture or it does not load', () => {
    const { container, rerender } = render(<LetterAvatar name="speedy" />);
    expect(container.textContent).toBe('S');
    expect(container.firstElementChild).toHaveAttribute('aria-hidden', 'true');
    expect(container.firstElementChild?.className).toMatch(/letter/);
    expect(container.firstElementChild?.className).toMatch(/tone\d/);
    rerender(<Avatar src="feed://localhost/ya/x.jpg" name="Mine" />);
    const picture = container.querySelector('img');
    expect(picture).toHaveAttribute('src', 'feed://localhost/ya/x.jpg');
    expect(picture).toHaveAttribute('alt', '');
    if (picture === null) throw new Error('no picture');
    fireEvent.error(picture);
    expect(container.querySelector('img')).toBeNull();
    expect(container.textContent).toBe('M');
  });

  it('keeps the feed’s order in the DOM and gives each card its rows once measured', () => {
    const heights: Record<string, number> = { a: 300, b: 120, c: 200 };
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
      this: HTMLElement,
    ) {
      return { height: heights[this.dataset.card ?? ''] ?? 0 } as DOMRect;
    });
    const real = window.getComputedStyle.bind(window);
    // The grid's small rows and its gap, as the stylesheet sets them.
    const grid: Record<string, string> = { gridAutoRows: '4px', columnGap: '16px' };
    vi.spyOn(window, 'getComputedStyle').mockImplementation((element) => {
      const style = real(element);
      if (element.getAttribute('role') !== 'list') return style;
      return new Proxy(style, {
        get: (target, key) => {
          if (typeof key === 'string' && key in grid) return grid[key];
          const value: unknown = Reflect.get(target, key, target);
          return typeof value === 'function' ? (value as () => unknown).bind(target) : value;
        },
      });
    });
    render(
      <Mosaic label="All">
        {['a', 'b', 'c'].map((name) => (
          <Cell key={name}>
            <article data-card={name}>{name}</article>
          </Cell>
        ))}
      </Mosaic>,
    );
    const list = screen.getByRole('list', { name: 'All' });
    const cells = screen.getAllByRole('listitem');
    expect(cells.map((cell) => cell.textContent)).toEqual(['a', 'b', 'c']);
    expect(list).toHaveAttribute('data-measured', 'true');
    expect(cells.map((cell) => cell.style.gridRowEnd)).toEqual(['span 79', 'span 34', 'span 54']);
  });

  it('stays a plain grid with nothing measured', () => {
    render(
      <Mosaic label="All">
        <Cell>
          <article>a</article>
        </Cell>
      </Mosaic>,
    );
    expect(screen.getByRole('list', { name: 'All' })).not.toHaveAttribute('data-measured');
    expect(screen.getByRole('listitem').style.gridRowEnd).toBe('');
  });
});
