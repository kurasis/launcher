import { useRef } from 'react';
import styles from './Segmented.module.css';

/** One choice of a segmented control. */
export interface SegmentedOption<T extends string | boolean> {
  value: T;
  label: string;
  /** Not open now (the control says why elsewhere). */
  disabled?: boolean;
}

interface SegmentedProps<T extends string | boolean> {
  /** The group's name for assistive technology. */
  label: string;
  options: readonly SegmentedOption<T>[];
  value: T;
  /** Nothing can be chosen (the setting cannot be written). */
  disabled?: boolean;
  /** A change is being saved: the choices wait, but keep the focus (a disabled
   * control loses it in WebView2 and WebKit). */
  pending?: boolean;
  /** Each choice only as wide as its words, for a toolbar. */
  compact?: boolean;
  onChange: (value: T) => void;
}

/**
 * A few choices side by side, one chosen (concept A's segmented control): radios in
 * one tab stop, the arrow keys move between the open ones, in the reading direction.
 */
export function Segmented<T extends string | boolean>({
  label,
  options,
  value,
  disabled = false,
  pending = false,
  compact = false,
  onChange,
}: SegmentedProps<T>) {
  const buttons = useRef(new Map<T, HTMLButtonElement>());
  const open = (option: SegmentedOption<T>) => !disabled && option.disabled !== true;
  const choose = (option: SegmentedOption<T> | undefined) => {
    if (option === undefined || pending || !open(option) || option.value === value) return;
    onChange(option.value);
    buttons.current.get(option.value)?.focus();
  };
  // The chosen one takes the tab stop; if none is (a value no option has), the first.
  const stop = options.some((option) => option.value === value) ? value : options[0]?.value;
  return (
    <div
      className={[styles.segmented, compact && styles.compact].filter(Boolean).join(' ')}
      role="radiogroup"
      aria-label={label}
      aria-busy={pending || undefined}
    >
      {options.map((option, index) => (
        <button
          key={String(option.value)}
          ref={(node) => {
            if (node === null) buttons.current.delete(option.value);
            else buttons.current.set(option.value, node);
          }}
          type="button"
          role="radio"
          aria-checked={option.value === value}
          aria-disabled={pending || undefined}
          tabIndex={option.value === stop ? 0 : -1}
          disabled={!open(option)}
          onClick={() => {
            choose(option);
          }}
          onKeyDown={(event) => {
            const keys: Record<string, number> = {
              ArrowLeft: -1,
              ArrowUp: -1,
              ArrowRight: 1,
              ArrowDown: 1,
            };
            let step = keys[event.key];
            if (step === undefined) return;
            event.preventDefault();
            // Left and right follow the text: in Arabic the next choice is on the left.
            const rtl = getComputedStyle(event.currentTarget).direction === 'rtl';
            if (rtl && (event.key === 'ArrowLeft' || event.key === 'ArrowRight')) step = -step;
            // The next open choice that way, round the ends.
            for (let i = 1; i < options.length; i++) {
              const next = options[(index + step * i + options.length) % options.length];
              if (next !== undefined && open(next)) {
                choose(next);
                return;
              }
            }
          }}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}
