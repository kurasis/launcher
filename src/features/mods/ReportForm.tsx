import { useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type { ModsReportReason } from '../../lib/contracts';
import { describeIpcError, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';

const REASONS: ModsReportReason[] = ['malware', 'stolen', 'inappropriate', 'broken', 'other'];

/** A player's report of another player's pack, for the catalog's owner (D-066 stage 6). */
export function ReportForm({ projectId, onClose }: { projectId: string; onClose: () => void }) {
  const t = strings.mods.project;
  const [reason, setReason] = useState<ModsReportReason>('broken');
  const [text, setText] = useState('');
  const [working, setWorking] = useState(false);
  const [sent, setSent] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (sent) return <Notice tone="info">{t.reportSent}</Notice>;
  return (
    <form
      className={forms.form}
      aria-label={t.report}
      onSubmit={(event) => {
        event.preventDefault();
        setWorking(true);
        setError(null);
        ipc
          .modsReport(projectId, reason, text)
          .then(() => {
            setSent(true);
          })
          .catch((e: unknown) => {
            setError(describeIpcError(e));
          })
          .finally(() => {
            setWorking(false);
          });
      }}
    >
      <Field label={t.reportReason}>
        {(props) => (
          <select
            {...props}
            className={forms.select}
            value={reason}
            disabled={working}
            onChange={(e) => {
              const picked = REASONS.find((r) => r === e.target.value);
              if (picked !== undefined) setReason(picked);
            }}
          >
            {REASONS.map((r) => (
              <option key={r} value={r}>
                {t.reportReasons[r]}
              </option>
            ))}
          </select>
        )}
      </Field>
      <Field label={t.reportText}>
        {(props) => (
          <textarea
            {...props}
            className={forms.input}
            value={text}
            maxLength={500}
            rows={3}
            disabled={working}
            onChange={(e) => {
              setText(e.target.value);
            }}
          />
        )}
      </Field>
      {error !== null && <Notice tone="danger">{error}</Notice>}
      <div className={forms.actions}>
        <Button type="submit" variant="primary" disabled={working}>
          {t.reportSend}
        </Button>
        <Button disabled={working} onClick={onClose}>
          {t.reportCancel}
        </Button>
      </div>
    </form>
  );
}
