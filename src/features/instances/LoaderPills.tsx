import { useId } from 'react';
import type { LoaderKind } from '../../lib/contracts';
import { strings } from '../../lib/strings';
import styles from './LoaderPills.module.css';

const tl = strings.loaders;

/** The pills in the order the new-installation page offers them: the plain game first. */
const OPTIONS: readonly (LoaderKind | 'none')[] = [
  'none',
  'fabric',
  'quilt',
  'forge',
  'neoforge',
  'optifine',
];

interface LoaderPillsProps {
  value: LoaderKind | 'none';
  onChange: (kind: LoaderKind | 'none') => void;
}

/**
 * The mod loader as a row of pills (D-072): native radio buttons in a named group,
 * so the keyboard moves between them with the arrow keys and the group is one tab
 * stop. The chosen loader's hint describes the group.
 */
export function LoaderPills({ value, onChange }: LoaderPillsProps) {
  const name = useId();
  const hintId = `${name}-hint`;
  return (
    <fieldset className={styles.group} aria-describedby={hintId}>
      <legend className={styles.legend}>{tl.loader}</legend>
      <div className={styles.pills}>
        {OPTIONS.map((kind) => (
          <label key={kind} className={styles.pill}>
            <input
              type="radio"
              className={styles.input}
              name={name}
              value={kind}
              checked={value === kind}
              onChange={() => {
                onChange(kind);
              }}
            />
            <span className={styles.face}>{kind === 'none' ? tl.noneShort : tl.names[kind]}</span>
          </label>
        ))}
      </div>
      <p id={hintId} className={styles.hint}>
        {tl.hints[value]}
      </p>
    </fieldset>
  );
}
