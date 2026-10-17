import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type { ApprovalRequest } from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';

const t = strings.metered;
const MIB = 1024 * 1024;

/**
 * A download waiting on a metered connection (D-051): its size, or how it
 * would pass the monthly limit, with Download and Don't download.
 */
export function DownloadApproval({
  operationId,
  approval,
}: {
  operationId: string;
  approval: ApprovalRequest;
}) {
  const answer = useCommand((requestId: string, allow: boolean) =>
    ipc.answerDownload(operationId, allow, requestId),
  );
  const size = formatBytes(approval.bytes);
  return (
    <Notice tone="warning">
      <p>
        <strong>{t.approvalTitle({ size })}</strong>
      </p>
      <p>
        {approval.reason === 'over_limit'
          ? t.approvalOverLimit({
              size,
              used: formatBytes(approval.monthBytes),
              limit: formatBytes(approval.monthlyLimitMib * MIB),
            })
          : t.approvalLarge({ files: approval.files, size })}
      </p>
      <div className={forms.actions}>
        <Button variant="primary" disabled={answer.pending} onClick={() => void answer.run(true)}>
          {t.allow}
        </Button>
        <Button disabled={answer.pending} onClick={() => void answer.run(false)}>
          {t.decline}
        </Button>
      </div>
      {answer.error !== null && <p>{answer.error}</p>}
    </Notice>
  );
}
