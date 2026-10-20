import { useId, useState } from 'react';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import forms from '../../components/forms.module.css';
import type { AppSnapshot, ModsKind, ModsProject, ModsVersion } from '../../lib/contracts';
import { strings } from '../../lib/strings';
import { gameLabel } from '../instances/loaderLabel';
import { AddDialog, type AddTarget } from './AddDialog';
import { AddToCatalog, RequestsSection } from './AddToCatalog';
import { ContentSection } from './ContentSection';
import { LinkConfirm } from './LinkConfirm';
import { PackDialog } from './PackDialog';
import { ProjectPage } from './ProjectPage';
import { SearchSection } from './SearchSection';
import styles from './Mods.module.css';

export type ModsSection = Exclude<ModsKind, 'world'> | 'installed' | 'requests';

const SECTIONS: ModsSection[] = [
  'mod',
  'resourcepack',
  'shader',
  'datapack',
  'modpack',
  'installed',
  'requests',
];

interface Props {
  snapshot: AppSnapshot;
  /** The installation to start with, e.g. from its details. */
  instanceId: string | null;
  section: ModsSection;
  onSection: (section: ModsSection) => void;
  onInstance: (instanceId: string | null) => void;
}

/**
 * The catalog of mods at mods.lapi.sh (D-066): search by kind, a project's
 * page, adding to an installation, "Add to the catalog" with its requests,
 * and what an installation holds.
 */
export function ModsView({ snapshot, instanceId, section, onSection, onInstance }: Props) {
  const t = strings.mods;
  const selectId = useId();
  const instance = snapshot.instances.find((i) => i.id === instanceId) ?? null;
  const [project, setProject] = useState<string | null>(null);
  const [adding, setAdding] = useState<AddTarget | null>(null);
  const [packing, setPacking] = useState<{ project: ModsProject; version: ModsVersion } | null>(
    null,
  );
  const [catalogText, setCatalogText] = useState<string | null>(null);
  const [link, setLink] = useState<string | null>(null);
  // Read the folder again after an install.
  const [revision, setRevision] = useState(0);

  const open = (projectId: string) => {
    setCatalogText(null);
    setProject(projectId);
  };

  return (
    <Panel title={t.title}>
      <div className={styles.controls}>
        <div className={styles.field}>
          <label htmlFor={selectId}>{t.installation}</label>
          <select
            id={selectId}
            className={forms.select}
            value={instance?.id ?? ''}
            onChange={(event) => {
              onInstance(event.target.value === '' ? null : event.target.value);
            }}
          >
            <option value="">{t.noInstallation}</option>
            {snapshot.instances.map((i) => (
              <option key={i.id} value={i.id}>
                {i.displayName} ({gameLabel(i)})
              </option>
            ))}
          </select>
        </div>
      </div>
      {instance === null && <p className={styles.muted}>{t.noInstallationHint}</p>}

      <div role="tablist" aria-label={t.sectionsLabel} className={styles.tabs}>
        {SECTIONS.map((item) => (
          <button
            key={item}
            type="button"
            role="tab"
            aria-selected={section === item && project === null}
            className={styles.tab}
            onClick={() => {
              setProject(null);
              onSection(item);
            }}
          >
            {t.sections[item]}
          </button>
        ))}
      </div>

      <div role="tabpanel" className={styles.panel}>
        {project !== null ? (
          <ProjectPage
            key={project}
            projectRef={project}
            instance={instance}
            onBack={() => {
              setProject(null);
            }}
            onLink={setLink}
            onAdd={(found, version) => {
              if (instance === null) return;
              setAdding({
                instance,
                versionId: version.id,
                title: found.title,
                kind: found.kind,
                replaces: null,
                world: null,
              });
            }}
            onInstallPack={(found, version) => {
              setPacking({ project: found, version });
            }}
          />
        ) : section === 'installed' ? (
          instance === null ? (
            <Notice tone="info">{t.content.choose}</Notice>
          ) : (
            <ContentSection
              key={instance.id}
              instance={instance}
              revision={revision}
              onOpen={open}
              onUpdate={(update, title, world) => {
                setAdding({
                  instance,
                  versionId: update.toVersionId,
                  title,
                  // Only a data pack's kind matters here: it goes into a world.
                  kind: world === null ? 'mod' : 'datapack',
                  replaces: update.path,
                  world,
                });
              }}
            />
          )
        ) : section === 'requests' ? (
          <RequestsSection onOpen={open} />
        ) : (
          <>
            {section === 'modpack' && <Notice tone="info">{t.modpacksHint}</Notice>}
            <SearchSection
              key={`${section}:${instance?.id ?? ''}`}
              kind={section}
              instance={instance}
              onOpen={open}
              onAddToCatalog={setCatalogText}
            />
          </>
        )}
      </div>

      {adding !== null && (
        <AddDialog
          target={adding}
          onClose={() => {
            setAdding(null);
          }}
          onDone={() => {
            setRevision((r) => r + 1);
          }}
        />
      )}
      {packing !== null && (
        <PackDialog
          title={packing.project.title || packing.project.slug}
          version={packing.version}
          onClose={() => {
            setPacking(null);
          }}
          onDone={(id) => {
            onInstance(id);
          }}
        />
      )}
      {catalogText !== null && (
        <AddToCatalog
          initial={catalogText}
          onClose={() => {
            setCatalogText(null);
          }}
          onOpen={open}
        />
      )}
      {link !== null && (
        <LinkConfirm
          url={link}
          onClose={() => {
            setLink(null);
          }}
        />
      )}
    </Panel>
  );
}
