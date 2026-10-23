import { type KeyboardEvent, useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import { Icon, type IconName } from '../../components/Icon';
import { Notice } from '../../components/Notice';
import type { AppSnapshot, ModsKind, ModsProject, ModsVersion } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { numberLocale, strings } from '../../lib/strings';
import { AddDialog, type AddTarget } from './AddDialog';
import { AddToCatalog, RequestsSection } from './AddToCatalog';
import { ContentSection } from './ContentSection';
import { InstallationSelect } from './InstallationSelect';
import { LinkConfirm } from './LinkConfirm';
import { PackDialog } from './PackDialog';
import { ProjectPage, type QuickTarget } from './ProjectPage';
import { SearchSection } from './SearchSection';
import { WorldDialog } from './WorldDialog';
import { installKind } from './helpers';
import { bestVersion } from './versions';
import catalog from './Catalog.module.css';

export type ModsSection = ModsKind | 'installed' | 'requests';

/** The tabs, with their icons; "My requests" opens from the top end instead. */
const TABS: { section: Exclude<ModsSection, 'requests'>; icon: IconName }[] = [
  { section: 'mod', icon: 'mods' },
  { section: 'resourcepack', icon: 'palette' },
  { section: 'shader', icon: 'sun' },
  { section: 'datapack', icon: 'db' },
  { section: 'modpack', icon: 'layers' },
  { section: 'world', icon: 'map' },
  { section: 'installed', icon: 'check' },
];

/** Requests still on their way into the catalog. */
const WAITING = new Set(['queued', 'choosing', 'importing']);

interface Props {
  snapshot: AppSnapshot;
  /** The installation to start with, e.g. from its details. */
  instanceId: string | null;
  section: ModsSection;
  onSection: (section: ModsSection) => void;
  onInstance: (instanceId: string | null) => void;
  /** A project whose page opens first, e.g. from the command palette (key the view by request). */
  initialProject?: string | null | undefined;
}

/**
 * The catalog of mods at mods.lapi.sh (D-066): search by kind, a project's
 * page, adding to an installation, "Add to the catalog" with its requests,
 * and what an installation holds.
 */
export function ModsView({
  snapshot,
  instanceId,
  section,
  onSection,
  onInstance,
  initialProject = null,
}: Props) {
  const t = strings.mods;
  const selectId = useId();
  const headingId = useId();
  const tabsId = useId();
  const instance = snapshot.instances.find((i) => i.id === instanceId) ?? null;
  const [project, setProject] = useState<string | null>(initialProject);
  const [adding, setAdding] = useState<AddTarget | null>(null);
  const [packing, setPacking] = useState<{ project: ModsProject; version: ModsVersion } | null>(
    null,
  );
  const [catalogText, setCatalogText] = useState<string | null>(null);
  const [link, setLink] = useState<string | null>(null);
  // Read the folder again after an install.
  const [revision, setRevision] = useState(0);
  // Requests still on their way, for the count beside "My requests".
  const [waiting, setWaiting] = useState(0);
  const draftText = useRef('');
  const tabs = useRef<HTMLDivElement>(null);
  // The latest quick add; an answer to an earlier one, or one from before the player
  // moved on, opens nothing.
  const quick = useRef(0);

  useEffect(() => {
    let live = true;
    ipc
      .modsImports()
      .then((list) => {
        if (live && Array.isArray(list)) {
          setWaiting(list.filter((request) => WAITING.has(request.state)).length);
        }
      })
      .catch(() => {
        // Signed out or offline: no count.
      });
    return () => {
      live = false;
    };
  }, [section]);

  const open = (projectId: string) => {
    setCatalogText(null);
    setProject(projectId);
  };

  // A project's page starts at the top. In a layout effect, after the catalog that
  // closes has kept how far down it was.
  useLayoutEffect(() => {
    if (project !== null) window.scrollTo(0, 0);
  }, [project]);

  // Before any answer can come in: another screen or dialog makes a pending quick add stale.
  const asking = catalogText !== null;
  useLayoutEffect(() => {
    quick.current += 1;
  }, [instance?.id, section, project, asking]);

  const choose = (next: ModsSection) => {
    setProject(null);
    onSection(next);
  };

  /**
   * Adds the newest version made for the installation's game version and loader, or
   * opens the project when none is. A dependency on a project's page is a mod.
   */
  const quickAdd = async (hit: QuickTarget, datapacks = section === 'datapack') => {
    if (instance === null) return;
    const asked = ++quick.current;
    let chosen: ModsVersion | undefined;
    try {
      const versions = await ipc.modsVersions(hit.id, null, null);
      chosen = bestVersion(
        hit.kind,
        Array.isArray(versions) ? versions : [],
        instance,
        datapacks,
        true,
      );
    } catch {
      chosen = undefined;
    }
    if (asked !== quick.current) return;
    if (chosen === undefined) {
      open(hit.id);
      return;
    }
    const target: AddTarget = {
      instance,
      versionId: chosen.id,
      title: hit.title || hit.slug,
      kind: installKind(hit.kind, chosen),
      replaces: null,
      world: null,
    };
    // An open dialog keeps its project.
    setAdding((now) => now ?? target);
  };

  // Arrow keys move between the tabs, Home and End to the first and last.
  const onTabKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const keys = ['ArrowLeft', 'ArrowRight', 'Home', 'End'];
    if (!keys.includes(event.key)) return;
    const buttons = Array.from(
      tabs.current?.querySelectorAll<HTMLButtonElement>('[role="tab"]') ?? [],
    );
    const at = buttons.findIndex((button) => button === document.activeElement);
    if (at < 0) return;
    event.preventDefault();
    const rtl = getComputedStyle(event.currentTarget).direction === 'rtl';
    const forward = (event.key === 'ArrowRight') !== rtl;
    const next =
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? buttons.length - 1
          : (at + (forward ? 1 : -1) + buttons.length) % buttons.length;
    const target = buttons[next];
    const item = TABS[next];
    if (target === undefined || item === undefined) return;
    target.focus();
    choose(item.section);
  };

  const searching = project === null && section !== 'installed' && section !== 'requests';
  const current = TABS.findIndex((item) => item.section === section && project === null);
  const tabId = (item: ModsSection) => `${tabsId}-${item}`;

  return (
    <section
      className={catalog.screen}
      aria-labelledby={project === null ? headingId : undefined}
      aria-label={project === null ? undefined : t.title}
    >
      <div className={catalog.frame}>
        {project !== null ? (
          <ProjectPage
            key={project}
            projectRef={project}
            instance={instance}
            instances={snapshot.instances}
            onInstance={onInstance}
            datapacks={section === 'datapack'}
            revision={revision}
            onBack={() => {
              setProject(null);
            }}
            onLink={setLink}
            onOpen={open}
            onAdd={(found, version, replaces) => {
              if (instance === null) return;
              setAdding({
                instance,
                versionId: version.id,
                title: found.title || found.slug,
                kind: installKind(found.kind, version),
                replaces,
                world: null,
              });
            }}
            onInstallPack={(found, version) => {
              setPacking({ project: found, version });
            }}
            onAddToCatalog={setCatalogText}
            onQuickAdd={(target) => {
              void quickAdd(target, false);
            }}
          />
        ) : (
          <>
            <header className={catalog.pageHead}>
              <h2 id={headingId} className={catalog.pageTitle}>
                {t.title}
              </h2>
              <InstallationSelect
                id={selectId}
                instances={snapshot.instances}
                value={instance?.id ?? null}
                onChange={onInstance}
              />
              <div className={catalog.requests} role="group" aria-label={t.catalog.requestsGroup}>
                <button
                  type="button"
                  className={catalog.smallButton}
                  aria-pressed={section === 'requests'}
                  onClick={() => {
                    choose('requests');
                  }}
                >
                  <Icon name="inbox" size={18} />
                  {t.sections.requests}
                  {waiting > 0 && ' '}
                  {waiting > 0 && (
                    <span className={[catalog.chip, catalog.chipSuccess].join(' ')}>
                      <span aria-hidden="true">
                        {waiting > 99 ? '99+' : waiting.toLocaleString(numberLocale())}
                      </span>
                      <span className={catalog.visuallyHidden}>
                        {t.catalog.requestsWaiting({ count: waiting })}
                      </span>
                    </span>
                  )}
                </button>
                <button
                  type="button"
                  className={[catalog.smallButton, catalog.soft].join(' ')}
                  onClick={() => {
                    setCatalogText(draftText.current.trim());
                  }}
                >
                  <Icon name="plus" size={18} />
                  {t.addToCatalog}
                </button>
              </div>
            </header>
            {instance === null && <p className={catalog.muted}>{t.noInstallationHint}</p>}

            <div
              className={[catalog.body, searching && catalog.withSide].filter(Boolean).join(' ')}
            >
              <div
                ref={tabs}
                role="tablist"
                aria-label={t.sectionsLabel}
                className={catalog.tabs}
                onKeyDown={onTabKey}
              >
                {TABS.map((item, index) => (
                  <button
                    key={item.section}
                    id={tabId(item.section)}
                    type="button"
                    role="tab"
                    aria-selected={index === current}
                    tabIndex={index === (current < 0 ? 0 : current) ? 0 : -1}
                    className={catalog.tab}
                    onClick={() => {
                      choose(item.section);
                    }}
                  >
                    <Icon name={item.icon} size={22} />
                    {t.sections[item.section]}
                  </button>
                ))}
              </div>

              {section === 'installed' ? (
                <div role="tabpanel" aria-labelledby={tabId('installed')} className={catalog.panel}>
                  {instance === null ? (
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
                  )}
                </div>
              ) : section === 'requests' ? (
                <div role="region" aria-label={t.sections.requests} className={catalog.panel}>
                  <RequestsSection onOpen={open} />
                </div>
              ) : (
                <SearchSection
                  key={`${section}:${instance?.id ?? ''}`}
                  kind={section}
                  instance={instance}
                  onOpen={open}
                  onAddToCatalog={setCatalogText}
                  onQuickAdd={quickAdd}
                  draftTextRef={draftText}
                  tabId={tabId(section)}
                />
              )}
            </div>
          </>
        )}
      </div>

      {adding?.kind === 'world' && (
        <WorldDialog
          key={`${adding.instance.id}:${adding.versionId}`}
          target={adding}
          onClose={() => {
            setAdding(null);
          }}
          onDone={() => {
            setRevision((r) => r + 1);
          }}
        />
      )}
      {adding !== null && adding.kind !== 'world' && (
        <AddDialog
          key={`${adding.instance.id}:${adding.versionId}:${adding.replaces ?? ''}`}
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
    </section>
  );
}
