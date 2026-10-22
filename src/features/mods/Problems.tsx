import { Notice } from '../../components/Notice';
import type { ModsSetProblem } from '../../lib/contracts';
import { strings } from '../../lib/strings';
import { describeProblem } from './helpers';

/** Problems listed at most; the rest are counted. */
const SHOWN = 8;

/** What the mods' jars say may stop the game: a warning, not a block. */
export function Problems({ problems }: { problems: ModsSetProblem[] }) {
  if (problems.length === 0) return null;
  const rest = problems.length - SHOWN;
  return (
    <Notice tone="warning">
      <p>{strings.mods.problems.intro}</p>
      <ul>
        {problems.slice(0, SHOWN).map((problem, index) => (
          <li key={index}>{describeProblem(problem)}</li>
        ))}
      </ul>
      {rest > 0 && <p>{strings.mods.problems.more({ count: rest })}</p>}
    </Notice>
  );
}
