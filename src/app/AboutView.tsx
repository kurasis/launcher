import golosLicense from '@fontsource-variable/golos-text/LICENSE?raw';
import notoArabicLicense from '@fontsource-variable/noto-sans-arabic/LICENSE?raw';
import { useId } from 'react';
import { Button } from '../components/Button';
import { Icon, type IconName } from '../components/Icon';
import { Logo } from '../components/Logo';
import { Notice } from '../components/Notice';
import { Panel } from '../components/Panel';
import panel from '../components/Panel.module.css';
import { Screen } from '../components/Screen';
import { SystemInfoPanel } from '../features/diagnostics/SystemInfoPanel';
import { openReview } from '../features/reviews/openReview';
import { UpdatePanel } from '../features/updates/UpdatePanel';
import type { AppSnapshot, ExternalLink } from '../lib/contracts';
import { ipc } from '../lib/ipc';
import { strings } from '../lib/strings';
import { useShownError } from '../lib/useShownError';
import styles from './About.module.css';
import { useAppInfo, type AppInfoState } from './useAppInfo';

const t = strings.about;

/** A page the core opens in the browser, from its fixed list. */
function LinkButton({ link, label }: { link: ExternalLink; label: string }) {
  const opening = useShownError();
  return (
    <div className={styles.link}>
      <IconButton
        icon="link"
        label={label}
        onClick={() => {
          opening.clear();
          void ipc.openExternal(link).catch(opening.catcher);
        }}
      />
      {opening.error !== null && <Notice tone="danger">{opening.error}</Notice>}
    </div>
  );
}

function IconButton({
  icon,
  label,
  onClick,
}: {
  icon: IconName;
  label: string;
  onClick: () => void;
}) {
  return (
    <Button size="small" onClick={onClick}>
      <Icon name={icon} size={18} />
      {label}
    </Button>
  );
}

/** The launcher's name, its version once the core answers, and what it is. */
function Brand({ info, reviews }: { info: AppInfoState; reviews: boolean }) {
  const headingId = useId();
  return (
    <section className={[panel.panel, styles.about].join(' ')} aria-labelledby={headingId}>
      <div className={styles.brand}>
        <Logo tone="surface" className={styles.logo} />
        <div className={styles.name}>
          <h3 id={headingId} className={styles.title}>
            {strings.brand.name} {strings.brand.suffix}
          </h3>
          {info.status === 'ready' && (
            <p className={styles.hint}>
              {t.versionLine({
                version: info.info.appVersion,
                profile: info.info.buildProfile,
              })}
            </p>
          )}
        </div>
      </div>
      <p>{strings.foundation.body}</p>
      <p>{strings.independentNotice}</p>
      <p className={styles.hint}>{t.license}</p>
      {/* The bundled fonts travel with their copyright notice and license (OFL 1.1). */}
      <details className={styles.fonts}>
        <summary>{t.fonts}</summary>
        <pre className={styles.licenseText} dir="ltr" lang="en">
          {golosLicense}
        </pre>
        <pre className={styles.licenseText} dir="ltr" lang="en">
          {notoArabicLicense}
        </pre>
      </details>
      <div className={styles.actions}>
        <LinkButton link="source_code" label={t.sourceCode} />
        {reviews && (
          <IconButton
            icon="star"
            label={strings.nav.reviews}
            onClick={() => {
              openReview();
            }}
          />
        )}
      </div>
    </section>
  );
}

/**
 * About (concept A, D-072): the launcher and its links, Minecraft's official pages, then
 * the version waiting to be installed and the core's facts. It opens while the core is
 * still loading or failed too; the update and Leave a review wait for its snapshot.
 */
export function AboutView({ snapshot }: { snapshot: AppSnapshot | null }) {
  const { state, reload } = useAppInfo();
  return (
    <Screen title={t.title}>
      <div className={styles.columns}>
        <div className={styles.column}>
          <Brand info={state} reviews={snapshot !== null} />
          <Panel title={t.officialTitle} level={3}>
            <div className={styles.actions}>
              <LinkButton link="minecraft_eula" label={t.eula} />
              <LinkButton link="minecraft_help" label={t.help} />
            </div>
          </Panel>
        </div>
        <div className={styles.column}>
          {snapshot !== null && <UpdatePanel update={snapshot.update} />}
          <SystemInfoPanel state={state} onRetry={reload} level={3} />
        </div>
      </div>
    </Screen>
  );
}
