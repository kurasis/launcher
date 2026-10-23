import dialog from '../../components/Dialog.module.css';
import { Notice } from '../../components/Notice';
import type { ModsSetProblem } from '../../lib/contracts';
import { strings } from '../../lib/strings';
import { describeProblem } from './helpers';

/** Problems listed at most; the rest are counted. */
const SHOWN = 8;

/**
 * What the mods' jars say may stop the game: a warning, not a block. Plain when it is
 * the whole question of a dialog; in a warning notice beside other things.
 */
export function Problems({
  problems,
  plain = false,
}: {
  problems: ModsSetProblem[];
  plain?: boolean;
}) {
  if (problems.length === 0) return null;
  const rest = problems.length - SHOWN;
  const list = (
    <>
      <p className={plain ? dialog.hint : undefined}>{strings.mods.problems.intro}</p>
      <ul className={plain ? dialog.list : undefined}>
        {problems.slice(0, SHOWN).map((problem, index) => (
          <li key={index}>{describeProblem(problem)}</li>
        ))}
      </ul>
      {rest > 0 && (
        <p className={plain ? dialog.hint : undefined}>
          {strings.mods.problems.more({ count: rest })}
        </p>
      )}
    </>
  );
  return plain ? list : <Notice tone="warning">{list}</Notice>;
}
