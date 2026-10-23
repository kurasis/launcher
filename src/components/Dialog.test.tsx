import { act, fireEvent, render, screen } from '@testing-library/react';
import { useRef, useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { Dialog } from './Dialog';
import { coveredByDialog } from './dialogStack';

afterEach(() => {
  document.getElementById('root')?.remove();
});

/** A page like the app's: its root, a main that takes the focus, an opener. */
function Page({
  initial,
  closable = true,
  onClose,
}: {
  initial?: 'cancel' | 'dialog';
  closable?: boolean;
  onClose?: () => void;
}) {
  const [open, setOpen] = useState(false);
  const cancel = useRef<HTMLButtonElement>(null);
  return (
    <main tabIndex={-1}>
      <button
        type="button"
        onClick={() => {
          setOpen(true);
        }}
      >
        Open
      </button>
      {open && (
        <Dialog
          title="Question"
          closable={closable}
          initialFocus={initial === 'cancel' ? cancel : initial}
          onClose={() => {
            onClose?.();
            setOpen(false);
          }}
        >
          <button type="button">First</button>
          <input aria-label="Middle" />
          <button
            ref={cancel}
            type="button"
            onClick={() => {
              setOpen(false);
            }}
          >
            Cancel
          </button>
        </Dialog>
      )}
    </main>
  );
}

function mount(props: Parameters<typeof Page>[0] = {}) {
  const root = document.createElement('div');
  root.id = 'root';
  document.body.append(root);
  render(<Page {...props} />, { container: root });
  const opener = screen.getByRole('button', { name: 'Open' });
  opener.focus();
  fireEvent.click(opener);
  return { root, opener, dialog: screen.getByRole('dialog', { name: 'Question' }) };
}

describe('Dialog', () => {
  it('is a modal on the page body, the page under it out of reach', () => {
    const { root, dialog } = mount();
    expect(dialog).toHaveAttribute('aria-modal', 'true');
    expect(root.contains(dialog)).toBe(false);
    expect(root).toHaveAttribute('inert');
    fireEvent.keyDown(dialog, { key: 'Escape' });
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(root).not.toHaveAttribute('inert');
  });

  it('starts on the first control, or on the one it is given, or on itself', () => {
    const first = mount();
    expect(screen.getByRole('button', { name: 'First' })).toHaveFocus();
    fireEvent.keyDown(first.dialog, { key: 'Escape' });
    first.root.remove();

    const cancel = mount({ initial: 'cancel' });
    expect(screen.getByRole('button', { name: 'Cancel' })).toHaveFocus();
    fireEvent.keyDown(cancel.dialog, { key: 'Escape' });
    cancel.root.remove();

    const itself = mount({ initial: 'dialog' });
    expect(itself.dialog).toHaveFocus();
  });

  it('keeps Tab and Shift+Tab inside', () => {
    const { dialog } = mount();
    const first = screen.getByRole('button', { name: 'First' });
    const last = screen.getByRole('button', { name: 'Cancel' });
    last.focus();
    fireEvent.keyDown(last, { key: 'Tab' });
    expect(first).toHaveFocus();
    fireEvent.keyDown(first, { key: 'Tab', shiftKey: true });
    expect(last).toHaveFocus();
    // From the dialog itself, Shift+Tab goes to the last control too.
    dialog.focus();
    fireEvent.keyDown(dialog, { key: 'Tab', shiftKey: true });
    expect(last).toHaveFocus();
    // In between, the browser moves on as usual.
    const middle = screen.getByRole('textbox', { name: 'Middle' });
    middle.focus();
    const tab = fireEvent.keyDown(middle, { key: 'Tab' });
    expect(tab).toBe(true);
  });

  it('leaves Escape to an inner control, an IME, or a question that must be answered', () => {
    const onClose = vi.fn();
    const { dialog } = mount({ onClose });
    const middle = screen.getByRole('textbox', { name: 'Middle' });
    middle.addEventListener('keydown', (event) => {
      if (event.key === 'Escape') event.preventDefault();
    });
    fireEvent.keyDown(middle, { key: 'Escape' });
    fireEvent.keyDown(dialog, { key: 'Escape', isComposing: true });
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.keyDown(dialog, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('does not close by Escape while it may not', () => {
    const onClose = vi.fn();
    const { dialog } = mount({ closable: false, onClose });
    fireEvent.keyDown(dialog, { key: 'Escape' });
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByRole('dialog')).toBeVisible();
  });

  it('gives the focus back to what opened it', () => {
    const { opener } = mount();
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(opener).toHaveFocus();
  });

  it('leaves the focus where the one who closed it put it', () => {
    function Nested() {
      const [inner, setInner] = useState(false);
      const other = useRef<HTMLButtonElement>(null);
      return (
        <Dialog title="Outer" onClose={vi.fn()}>
          <button
            type="button"
            onClick={() => {
              setInner(true);
            }}
          >
            More
          </button>
          <button ref={other} type="button">
            Other
          </button>
          {inner && (
            <Dialog
              title="Inner"
              onClose={() => {
                setInner(false);
                other.current?.focus();
              }}
            >
              <button type="button">Answer</button>
            </Dialog>
          )}
        </Dialog>
      );
    }
    const root = document.createElement('div');
    root.id = 'root';
    document.body.append(root);
    render(<Nested />, { container: root });
    const more = screen.getByRole('button', { name: 'More' });
    more.focus();
    fireEvent.click(more);
    expect(screen.getByRole('button', { name: 'Answer' })).toHaveFocus();
    // Escape closes only the inner one; the page stays out of reach under the outer.
    fireEvent.keyDown(screen.getByRole('dialog', { name: 'Inner' }), { key: 'Escape' });
    expect(screen.queryByRole('dialog', { name: 'Inner' })).toBeNull();
    expect(screen.getByRole('dialog', { name: 'Outer' })).toBeVisible();
    expect(root).toHaveAttribute('inert');
    expect(screen.getByRole('button', { name: 'Other' })).toHaveFocus();
  });

  it('keeps the focus a control inside took', () => {
    function Counter() {
      const [count, setCount] = useState(0);
      return (
        <Dialog title="Count" onClose={vi.fn()}>
          <button type="button">Close</button>
          <button
            type="button"
            onClick={() => {
              setCount((n) => n + 1);
            }}
          >
            More {count}
          </button>
        </Dialog>
      );
    }
    render(<Counter />);
    const more = screen.getByRole('button', { name: 'More 0' });
    more.focus();
    fireEvent.click(more);
    expect(screen.getByRole('button', { name: 'More 1' })).toHaveFocus();
  });

  it('puts the focus back on what it was given, when the focus fell out', () => {
    function Retry() {
      const [failed, setFailed] = useState(true);
      const cancel = useRef<HTMLButtonElement>(null);
      return (
        <Dialog title="Delete" initialFocus={cancel} onClose={vi.fn()}>
          <button type="button">Delete</button>
          {failed && (
            <button
              type="button"
              onClick={() => {
                setFailed(false);
              }}
            >
              Try again
            </button>
          )}
          <button ref={cancel} type="button">
            Cancel
          </button>
        </Dialog>
      );
    }
    render(<Retry />);
    const retry = screen.getByRole('button', { name: 'Try again' });
    retry.focus();
    // The button with the focus goes away: Cancel takes it, not Delete.
    fireEvent.click(retry);
    expect(screen.getByRole('button', { name: 'Cancel' })).toHaveFocus();
  });

  it('brings back the focus that lands outside the dialogs', () => {
    const { dialog } = mount();
    // A window of the page body over it (a viewer), not a dialog of the stack.
    const viewer = document.createElement('button');
    document.body.append(viewer);
    viewer.focus();
    expect(screen.getByRole('button', { name: 'First' })).toHaveFocus();
    expect(coveredByDialog(viewer)).toBe(true);
    expect(coveredByDialog(screen.getByRole('button', { name: 'First' }))).toBe(false);
    expect(coveredByDialog(dialog.querySelector('input'))).toBe(false);
    viewer.remove();
  });

  it('keeps the focus on a press on the scrim', () => {
    const { dialog } = mount();
    const backdrop = dialog.parentElement;
    expect(backdrop).not.toBeNull();
    if (backdrop === null) return;
    expect(fireEvent.mouseDown(backdrop)).toBe(false);
    expect(fireEvent.mouseDown(dialog)).toBe(true);
  });

  it('gives the focus to the dialog under it when what opened it is gone', () => {
    function Stacked() {
      const [inner, setInner] = useState(false);
      return (
        <Dialog title="Outer" onClose={vi.fn()}>
          <button type="button">Stay</button>
          {!inner && (
            <button
              type="button"
              onClick={() => {
                setInner(true);
              }}
            >
              Ask
            </button>
          )}
          {inner && (
            <Dialog
              title="Inner"
              onClose={() => {
                setInner(false);
              }}
            >
              <button type="button">Answer</button>
            </Dialog>
          )}
        </Dialog>
      );
    }
    render(<Stacked />);
    const ask = screen.getByRole('button', { name: 'Ask' });
    ask.focus();
    fireEvent.click(ask);
    act(() => {
      fireEvent.keyDown(screen.getByRole('dialog', { name: 'Inner' }), { key: 'Escape' });
    });
    expect(screen.getByRole('button', { name: 'Stay' })).toHaveFocus();
  });

  it('gives the focus to the page when what opened it is gone', () => {
    function Gone() {
      const [open, setOpen] = useState(false);
      return (
        <main tabIndex={-1}>
          {!open && (
            <button
              type="button"
              onClick={() => {
                setOpen(true);
              }}
            >
              Delete
            </button>
          )}
          {open && (
            <Dialog
              title="Question"
              onClose={() => {
                setOpen(false);
              }}
            >
              <button type="button">Answer</button>
            </Dialog>
          )}
        </main>
      );
    }
    render(<Gone />);
    const opener = screen.getByRole('button', { name: 'Delete' });
    opener.focus();
    fireEvent.click(opener);
    // It is drawn again by the time the dialog closes, but it is a new button.
    act(() => {
      fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    });
    expect(screen.getByRole('main')).toHaveFocus();
  });

  it('goes round past a disabled last control', () => {
    render(
      <Dialog title="Sending" onClose={vi.fn()}>
        <button type="button">First</button>
        <button type="button">Second</button>
        <button type="button" disabled>
          Send
        </button>
      </Dialog>,
    );
    const second = screen.getByRole('button', { name: 'Second' });
    second.focus();
    fireEvent.keyDown(second, { key: 'Tab' });
    expect(screen.getByRole('button', { name: 'First' })).toHaveFocus();
  });

  it('skips controls a disabled fieldset holds', () => {
    render(
      <Dialog title="Sending" onClose={vi.fn()}>
        <fieldset disabled>
          <input type="checkbox" aria-label="File" />
        </fieldset>
        <button type="button">Close</button>
      </Dialog>,
    );
    expect(screen.getByRole('button', { name: 'Close' })).toHaveFocus();
  });
});
