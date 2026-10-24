import type { KeyboardEvent, ReactNode, Ref } from 'react';
import styles from './Tabs.module.css';

export interface TabItem<T extends string> {
  value: T;
  label: ReactNode;
  /** The tab itself, for the focus to come back to. */
  ref?: Ref<HTMLButtonElement>;
}

const tabId = (id: string, value: string) => `${id}-${value}`;
const panelId = (id: string, value: string) => `${id}-${value}-panel`;

interface TabsProps<T extends string> {
  /** The tabs' name for assistive technology. */
  label: string;
  /** What the ids of the tabs and their panels start with (a `useId`). */
  id: string;
  items: readonly TabItem<T>[];
  value: T;
  onChange: (value: T) => void;
}

/**
 * Tabs underlined in the accent (concept A): one tab stop, the arrow keys move in the
 * reading direction, Home and End go to the ends. Each tab controls its own `TabPanel`.
 */
export function Tabs<T extends string>({ label, id, items, value, onChange }: TabsProps<T>) {
  const move = (event: KeyboardEvent<HTMLDivElement>) => {
    const at = items.findIndex((item) => item.value === value);
    let next: TabItem<T> | undefined;
    if (event.key === 'Home') next = items[0];
    else if (event.key === 'End') next = items[items.length - 1];
    else if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
      let step = event.key === 'ArrowRight' ? 1 : -1;
      if (getComputedStyle(event.currentTarget).direction === 'rtl') step = -step;
      next = items[(at + step + items.length) % items.length];
    }
    if (next === undefined) return;
    event.preventDefault();
    if (next.value !== value) onChange(next.value);
    event.currentTarget.querySelector<HTMLElement>(`[data-tab="${next.value}"]`)?.focus();
  };
  return (
    <div role="tablist" aria-label={label} className={styles.tabs} onKeyDown={move}>
      {items.map((item) => (
        <button
          key={item.value}
          ref={item.ref}
          type="button"
          role="tab"
          id={tabId(id, item.value)}
          data-tab={item.value}
          aria-selected={item.value === value}
          aria-controls={panelId(id, item.value)}
          tabIndex={item.value === value ? 0 : -1}
          className={styles.tab}
          onClick={() => {
            if (item.value !== value) onChange(item.value);
          }}
        >
          {item.label}
        </button>
      ))}
    </div>
  );
}

/** The panel of one tab: always there for the tab to point at, hidden while another is chosen. */
export function TabPanel({
  id,
  value,
  shown,
  className,
  children,
}: {
  id: string;
  value: string;
  shown: boolean;
  className?: string | undefined;
  children: ReactNode;
}) {
  return (
    <div
      role="tabpanel"
      id={panelId(id, value)}
      aria-labelledby={tabId(id, value)}
      hidden={!shown}
      className={className}
    >
      {children}
    </div>
  );
}
