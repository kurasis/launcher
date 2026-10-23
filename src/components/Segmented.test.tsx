import { fireEvent, render, screen } from '@testing-library/react';
import { useState } from 'react';
import { describe, expect, it, vi } from 'vitest';
import { Segmented, type SegmentedOption } from './Segmented';

const OPTIONS: SegmentedOption<string>[] = [
  { value: 'a', label: 'Alpha' },
  { value: 'b', label: 'Beta', disabled: true },
  { value: 'c', label: 'Gamma' },
];

function Choice({
  first = 'a',
  options = OPTIONS,
  pending = false,
  disabled = false,
  onChange = vi.fn(),
}: {
  first?: string;
  options?: SegmentedOption<string>[];
  pending?: boolean;
  disabled?: boolean;
  onChange?: (value: string) => void;
}) {
  const [value, setValue] = useState(first);
  return (
    <Segmented
      label="Letter"
      options={options}
      value={value}
      pending={pending}
      disabled={disabled}
      onChange={(next) => {
        onChange(next);
        setValue(next);
      }}
    />
  );
}

const radio = (name: string) => screen.getByRole('radio', { name });

describe('Segmented', () => {
  it('is one tab stop on the chosen option', () => {
    render(<Choice first="c" />);
    expect(screen.getByRole('radiogroup', { name: 'Letter' })).toBeVisible();
    expect(radio('Gamma')).toHaveAttribute('aria-checked', 'true');
    expect(radio('Gamma')).toHaveAttribute('tabindex', '0');
    expect(radio('Alpha')).toHaveAttribute('tabindex', '-1');
  });

  it('gives the tab stop to the first option when none is chosen', () => {
    render(<Choice first="z" />);
    expect(radio('Alpha')).toHaveAttribute('tabindex', '0');
  });

  it('moves with the arrows past a closed option, round the ends, and takes the focus', () => {
    const onChange = vi.fn();
    render(<Choice onChange={onChange} />);
    radio('Alpha').focus();
    fireEvent.keyDown(radio('Alpha'), { key: 'ArrowRight' });
    expect(onChange).toHaveBeenLastCalledWith('c');
    expect(radio('Gamma')).toHaveFocus();
    fireEvent.keyDown(radio('Gamma'), { key: 'ArrowDown' });
    expect(onChange).toHaveBeenLastCalledWith('a');
    fireEvent.keyDown(radio('Alpha'), { key: 'ArrowUp' });
    expect(onChange).toHaveBeenLastCalledWith('c');
    expect(onChange).not.toHaveBeenCalledWith('b');
  });

  it('follows the reading direction in Arabic', () => {
    const onChange = vi.fn();
    render(
      <div dir="rtl" style={{ direction: 'rtl' }}>
        <Choice
          options={OPTIONS.map((option) => ({ value: option.value, label: option.label }))}
          onChange={onChange}
        />
      </div>,
    );
    // Left is forward when the text runs right to left.
    fireEvent.keyDown(radio('Alpha'), { key: 'ArrowLeft' });
    expect(onChange).toHaveBeenLastCalledWith('b');
    fireEvent.keyDown(radio('Beta'), { key: 'ArrowRight' });
    expect(onChange).toHaveBeenLastCalledWith('a');
    // Up and down keep their sense.
    fireEvent.keyDown(radio('Alpha'), { key: 'ArrowDown' });
    expect(onChange).toHaveBeenLastCalledWith('b');
  });

  it('does not choose a closed option by a click, or the chosen one again', () => {
    const onChange = vi.fn();
    render(<Choice onChange={onChange} />);
    expect(radio('Beta')).toBeDisabled();
    fireEvent.click(radio('Beta'));
    fireEvent.click(radio('Alpha'));
    expect(onChange).not.toHaveBeenCalled();
  });

  it('waits while a change is saved, keeping the focus where it is', () => {
    const onChange = vi.fn();
    render(<Choice pending onChange={onChange} />);
    expect(screen.getByRole('radiogroup')).toHaveAttribute('aria-busy', 'true');
    expect(radio('Gamma')).toHaveAttribute('aria-disabled', 'true');
    // Not disabled: a disabled button would drop the focus.
    expect(radio('Gamma')).toBeEnabled();
    radio('Alpha').focus();
    fireEvent.click(radio('Gamma'));
    fireEvent.keyDown(radio('Alpha'), { key: 'ArrowRight' });
    expect(onChange).not.toHaveBeenCalled();
    expect(radio('Alpha')).toHaveFocus();
  });

  it('chooses nothing when the whole control is closed', () => {
    const onChange = vi.fn();
    render(<Choice disabled onChange={onChange} />);
    for (const name of ['Alpha', 'Beta', 'Gamma']) expect(radio(name)).toBeDisabled();
    fireEvent.keyDown(radio('Alpha'), { key: 'ArrowRight' });
    expect(onChange).not.toHaveBeenCalled();
  });

  it('works with true and false', () => {
    const onChange = vi.fn();
    render(
      <Segmented
        label="Mode"
        options={[
          { value: false, label: 'Off' },
          { value: true, label: 'On' },
        ]}
        value={false}
        onChange={onChange}
      />,
    );
    fireEvent.click(radio('On'));
    expect(onChange).toHaveBeenCalledWith(true);
  });
});
