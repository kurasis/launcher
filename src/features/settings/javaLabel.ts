import type { InstanceSummary } from '../../lib/contracts';
import { strings } from '../../lib/strings';

/** The Java an installation runs: the user's own, the private runtime, or none yet. */
export function javaLabel(instance: InstanceSummary): string {
  const t = strings.settings;
  if (instance.customJava !== null) {
    return t.instanceJava.customValue({ version: instance.customJava.javaVersion });
  }
  if (instance.java === null) return strings.details.javaNone;
  return t.javaValue({
    provider: t.javaShort[instance.java.provider],
    major: instance.java.major,
    release: instance.java.release,
  });
}
