import { render } from '@testing-library/react';
import indexHtml from '../../index.html?raw';
import { StartBar, StartScreen } from './StartPlaceholder';

/** An element's tags and classes, down through its children. */
function shape(element: Element): string {
  const children = Array.from(element.children).map(shape).join('');
  return `<${element.tagName.toLowerCase()} ${element.className}>${children}</>`;
}

function fromIndex(selector: string): Element {
  const page = new DOMParser().parseFromString(indexHtml, 'text/html');
  const element = page.querySelector(selector);
  if (element === null) throw new Error(`index.html has no ${selector}`);
  return element;
}

describe('start placeholder', () => {
  it('draws the screen and the bar as index.html does', () => {
    const screen = render(<StartScreen />).container.firstElementChild;
    expect(screen && shape(screen)).toBe(shape(fromIndex('.boot .boot-screen')));
    const bar = render(<StartBar />).container.firstElementChild;
    expect(bar && shape(bar)).toBe(shape(fromIndex('.boot .boot-bar')));
  });

  it('is hidden from assistive technology', () => {
    expect(fromIndex('.boot').getAttribute('aria-hidden')).toBe('true');
    const { container } = render(
      <>
        <StartScreen />
        <StartBar />
      </>,
    );
    for (const element of Array.from(container.children)) {
      expect(element).toHaveAttribute('aria-hidden', 'true');
    }
  });
});
