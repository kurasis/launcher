import { useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type {
  InstanceSummary,
  JavaCheck,
  JavaChoice,
  JavaIssue,
  JavaProvider,
} from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { useShownError } from '../../lib/useShownError';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { javaLabel } from './javaLabel';
import styles from './SettingsView.module.css';

const t = strings.settings.instanceJava;
const providers: JavaProvider[] = ['mojang', 'temurin'];
type Mode = JavaProvider | 'custom';

function issueText(issue: JavaIssue, required: number, arch: string): string {
  return issue === 'older'
    ? t.issues.older({ required })
    : issue === 'newer'
      ? t.issues.newer({ required })
      : t.issues.architecture({ arch });
}

/**
 * Which Java one installation runs (D-044): the private runtime from a chosen
 * source, or the user's own Java after a compatibility check whose warnings the
 * user may accept. Also the extra JVM options. Keyed by instance id by the parent.
 */
export function InstanceJavaForm({ instance }: { instance: InstanceSummary }) {
  const [mode, setMode] = useState<Mode>(
    instance.customJava !== null
      ? 'custom'
      : (instance.javaProvider ?? instance.java?.provider ?? 'mojang'),
  );
  const [path, setPath] = useState(instance.customJava?.path ?? '');
  const [args, setArgs] = useState(instance.jvmArguments);
  // A check whose warnings wait for the user's answer.
  const [pendingCheck, setPendingCheck] = useState<JavaCheck | null>(null);
  const [lastCheck, setLastCheck] = useState<JavaCheck | null>(null);
  const [saved, setSaved] = useState(false);
  const picking = useShownError();
  const check = useCommand((_requestId: string, javaPath: string) =>
    ipc.checkJava(instance.id, javaPath),
  );
  // Every value sent is an argument: useCommand keeps a request ID only for
  // a retry with the same ones.
  const save = useCommand(
    (requestId: string, java: JavaChoice, jvmArguments: string, accept: boolean) =>
      ipc.setInstanceJava(
        { instanceId: instance.id, java, jvmArguments, acceptIssues: accept },
        requestId,
      ),
  );
  const repair = useCommand((requestId: string) => ipc.repairInstance(instance.id, requestId));
  const installed = instance.java !== null;
  const busy = instance.busy || check.pending || save.pending;

  const edited = () => {
    setSaved(false);
    setPendingCheck(null);
  };

  const store = async (java: JavaChoice, accept: boolean) => {
    setPendingCheck(null);
    const result = await save.run(java, args, accept);
    if (result) setSaved(true);
  };

  const submit = async () => {
    setSaved(false);
    if (mode !== 'custom') {
      await store({ kind: 'managed', provider: mode }, false);
      return;
    }
    const checked = await check.run(path.trim());
    if (!checked) return;
    setLastCheck(checked);
    if (checked.issues.length > 0) {
      setPendingCheck(checked);
      return;
    }
    await store({ kind: 'custom', path: checked.path }, false);
  };

  const shown = pendingCheck ?? lastCheck;
  // The saved Java's accepted warnings, while that Java is still the one shown.
  const accepted =
    mode === 'custom' && instance.customJava?.path === path.trim()
      ? instance.customJava.accepted
      : [];
  const required = shown?.requiredMajor ?? instance.java?.major ?? 0;

  return (
    <form
      className={forms.form}
      aria-label={t.title}
      onSubmit={(event) => {
        event.preventDefault();
        void submit();
      }}
    >
      <h3 className={styles.subheading}>{t.title}</h3>
      <p className={styles.hint}>{t.now({ value: javaLabel(instance) })}</p>
      <fieldset className={forms.choices} disabled={busy} aria-label={t.title}>
        {providers.map((provider) => (
          <label key={provider} className={styles.option}>
            <input
              type="radio"
              name={`java-${instance.id}`}
              value={provider}
              checked={mode === provider}
              onChange={() => {
                setMode(provider);
                edited();
              }}
            />
            <span>
              <span className={styles.optionTitle}>
                {t.managed({ provider: strings.settings.javaProviders[provider] })}
              </span>
              <span className={styles.hint}>{t.managedHint}</span>
            </span>
          </label>
        ))}
        <label className={styles.option}>
          <input
            type="radio"
            name={`java-${instance.id}`}
            value="custom"
            checked={mode === 'custom'}
            disabled={!installed}
            onChange={() => {
              setMode('custom');
              edited();
            }}
          />
          <span>
            <span className={styles.optionTitle}>{t.custom}</span>
            <span className={styles.hint}>{installed ? t.customHint : t.installFirst}</span>
          </span>
        </label>
      </fieldset>
      {mode === 'custom' && (
        <>
          <Field label={t.path} hint={t.pathHint}>
            {(props) => (
              <input
                {...props}
                className={forms.input}
                dir="ltr"
                spellCheck={false}
                value={path}
                disabled={busy}
                onChange={(event) => {
                  setPath(event.target.value);
                  edited();
                }}
              />
            )}
          </Field>
          <div className={forms.actions}>
            <Button
              disabled={busy}
              onClick={() => {
                picking.clear();
                void ipc
                  .pickJavaFile()
                  .then((picked) => {
                    if (picked !== null) {
                      setPath(picked);
                      edited();
                    }
                  })
                  .catch(picking.catcher);
              }}
            >
              {t.browse}
            </Button>
          </div>
        </>
      )}
      {mode === 'custom' && shown !== null && shown.path === path.trim() && (
        <p className={styles.hint} role="status">
          {t.checked({
            version: shown.javaVersion,
            vendor: shown.vendor ?? t.unknownVendor,
            arch: shown.arch,
            required: shown.requiredMajor,
          })}
        </p>
      )}
      {check.pending && <p role="status">{t.checking}</p>}
      {check.error !== null && <Notice tone="danger">{check.error}</Notice>}
      {picking.error !== null && <Notice tone="danger">{picking.error}</Notice>}
      {pendingCheck !== null && (
        <Notice tone="warning">
          {pendingCheck.issues.map((issue) => (
            <p key={issue}>{issueText(issue, pendingCheck.requiredMajor, pendingCheck.arch)}</p>
          ))}
          <p>{t.confirm}</p>
          <div className={forms.actions}>
            <Button
              variant="primary"
              onClick={() => void store({ kind: 'custom', path: pendingCheck.path }, true)}
            >
              {t.useAnyway}
            </Button>
            <Button
              onClick={() => {
                setPendingCheck(null);
              }}
            >
              {strings.createForm.cancel}
            </Button>
          </div>
        </Notice>
      )}
      {pendingCheck === null && accepted.length > 0 && (
        <Notice tone="warning">
          <p>{t.accepted}</p>
          {accepted.map((issue) => (
            <p key={issue}>{issueText(issue, required, shown?.arch ?? '')}</p>
          ))}
        </Notice>
      )}
      <Field label={t.jvmArguments} hint={t.jvmHint}>
        {(props) => (
          <input
            {...props}
            className={forms.input}
            dir="ltr"
            spellCheck={false}
            value={args}
            disabled={busy}
            onChange={(event) => {
              setArgs(event.target.value);
              edited();
            }}
          />
        )}
      </Field>
      {save.error !== null && <Notice tone="danger">{save.error}</Notice>}
      {instance.javaProvider !== null && installed && (
        <Notice tone="info">
          <p>
            {t.pendingSwitch({ provider: strings.settings.javaProviders[instance.javaProvider] })}
          </p>
          <div className={forms.actions}>
            <Button disabled={instance.busy || repair.pending} onClick={() => void repair.run()}>
              {t.repairNow}
            </Button>
          </div>
          {repair.error !== null && <p>{repair.error}</p>}
        </Notice>
      )}
      <div className={forms.actions}>
        <Button variant="primary" type="submit" disabled={busy || pendingCheck !== null}>
          {t.save}
        </Button>
        {saved && <span role="status">{strings.details.saved}</span>}
      </div>
    </form>
  );
}
