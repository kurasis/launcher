import { act, fireEvent, render, screen } from '@testing-library/react';
import { useRef, useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { Dialog } from './Dialog';

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
    function Placed() {
      const [open, setOpen] = useState(true);
      const heading = useRef<HTMLHeadingElement>(null);
      return (
        <>
          <h3 ref={heading} tabIndex={-1}>
            Name
          </h3>
          {open && (
            <Dialog
              title="Question"
              onClose={() => {
                setOpen(false);
                heading.current?.focus();
              }}
            >
              <button type="button">Answer</button>
            </Dialog>
          )}
        </>
      );
    }
    render(<Placed />);
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    expect(screen.getByRole('heading', { name: 'Name' })).toHaveFocus();
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
