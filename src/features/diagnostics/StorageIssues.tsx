import { Notice } from '../../components/Notice';
import type { StorageIssue } from '../../lib/contracts';
import { strings } from '../../lib/strings';

export function StorageIssues({ issues }: { issues: StorageIssue[] }) {
  if (issues.length === 0) return null;
  return (
    <Notice tone="warning">
      <strong>{strings.storage.issuesTitle}</strong>
      <ul>
        {issues.map((issue) => (
          <li key={`${issue.document}:${issue.message}`}>
            {issue.message} <code>{issue.document}</code>
          </li>
        ))}
      </ul>
    </Notice>
  );
}
