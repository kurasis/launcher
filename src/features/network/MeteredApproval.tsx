import { Button } from '../../components/Button';
import { Dialog } from '../../components/Dialog';
import dialog from '../../components/Dialog.module.css';
import { Notice } from '../../components/Notice';
import type { AppSnapshot, ApprovalRequest } from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { isolate } from '../mods/helpers';
import { putAside, useAside } from './approvalStore';

const t = strings.metered;
const MIB = 1024 * 1024;

/**
 * A download that waits on a metered connection (D-051): a dialog that opens by itself
 * with its size and file count, or how it would pass the monthly limit, and Download or
 * Don't download. Closed without an answer, it waits; Downloads and the installation
 * can show it again.
 */
export function MeteredApproval({ snapshot }: { snapshot: AppSnapshot }) {
  const progress = snapshot.operationProgress;
  const aside = useAside(progress?.operationId ?? null);
  if (progress === null || progress.approval === null || aside) return null;
  const instance = snapshot.instances.find((one) => one.id === progress.instanceId);
  return (
    <Question
      key={progress.operationId}
      operationId={progress.operationId}
      approval={progress.approval}
      name={instance?.displayName ?? null}
    />
  );
}

function Question({
  operationId,
  approval,
  name,
}: {
  operationId: string;
  approval: ApprovalRequest;
  name: string | null;
}) {
  const answer = useCommand((requestId: string, allow: boolean) =>
    ipc.answerDownload(operationId, allow, requestId),
  );
  const size = formatBytes(approval.bytes);
  const used = formatBytes(approval.monthBytes);
  const limit = formatBytes(approval.monthlyLimitMib * MIB);
  const reply = (allow: boolean) => {
    if (!answer.pending) void answer.run(allow);
  };
  return (
    // It opens by itself, maybe while the player types elsewhere: the dialog takes the
    // focus, not an answer, so a stray Enter neither downloads nor cancels.
    <Dialog
      title={t.approvalTitle({ size })}
      initialFocus="dialog"
      closable={!answer.pending}
      onClose={() => {
        putAside(operationId);
      }}
    >
      {approval.reason === 'over_limit' ? (
        <p>{t.approvalOverLimit({ size, used, limit })}</p>
      ) : (
        <p>
          {name === null
            ? t.approvalLarge({ files: approval.files, size })
            : t.approvalFor({ files: approval.files, name: isolate(name) })}{' '}
          {approval.monthlyLimitMib > 0
            ? t.monthUsedOfLimit({ used, limit })
            : t.monthUsed({ used })}
        </p>
      )}
      {answer.error !== null && <Notice tone="danger">{answer.error}</Notice>}
      <div className={dialog.actions}>
        <Button
          variant="primary"
          size="small"
          aria-disabled={answer.pending || undefined}
          onClick={() => {
            reply(true);
          }}
        >
          {t.allow}
        </Button>
        <Button
          size="small"
          aria-disabled={answer.pending || undefined}
          onClick={() => {
            reply(false);
          }}
        >
          {t.decline}
        </Button>
      </div>
    </Dialog>
  );
}
