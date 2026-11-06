import { canCacheSource } from './storageLayout';
import { useTranslation } from 'react-i18next';
import { useState } from 'react';
import { useDroppable } from '@dnd-kit/core';
import {
  Album,
  ChevronDown,
  ChevronRight,
  Circle,
  Database,
  Flag,
  Folder,
  History,
  MoreHorizontal,
  Plus,
  Search,
  Sparkles,
  X,
} from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'react-toastify';
import { Collection, Session, useCatalogStore } from '../../store/useCatalogStore';
import { useLibraryStore } from '../../store/useLibraryStore';
import { useUIStore } from '../../store/useUIStore';
import { useSettingsStore } from '../../store/useSettingsStore';
import { useContextMenu } from '../../context/ContextMenuContext';
import { Asset, initialQuery } from '../../store/useCatalogStore';
import { convertFileSrc } from '@tauri-apps/api/core';
import { useProcessStore } from '../../store/useProcessStore';
import { LibraryDisplayMode } from '../ui/AppProperties';
import Text from '../ui/Text';
import { TextVariants, TextWeights } from '../../types/typography';

function CollectionRow({ collection, depth, toggle }: { collection: Collection; depth: number; toggle: () => void }) {
  const { showContextMenu } = useContextMenu();
  const allPaths = async (availableOnly = true) => {
    const paths: string[] = [];
    for (let offset = 0; ; offset += 1000) {
      const page = await invoke<{ assets: Asset[]; total: number }>('library_query', {
        query: { ...initialQuery, collectionId: collection.id, offset, limit: 1000 },
      });
      paths.push(...page.assets.filter((a) => !availableOnly || a.available).map((a) => a.path));
      useProcessStore.getState().setProcess((s) => ({
        thumbnails: {
          ...s.thumbnails,
          ...Object.fromEntries(
            page.assets.filter((a) => a.smallPreview).map((a) => [a.path, convertFileSrc(a.smallPreview!)]),
          ),
        },
      }));
      if (offset + 1000 >= page.total) break;
    }
    return paths;
  };
  const menu = (event: React.MouseEvent) => {
    event.preventDefault();
    event.stopPropagation();
    const action = (run: () => Promise<unknown>) => () => void run().catch((e) => toast.error(String(e)));
    showContextMenu(event.clientX, event.clientY, [
      { label: 'Edit collection…', onClick: () => useCatalogStore.getState().set({ edit: collection }) },
      ...(collection.kind !== 'set'
        ? [
            {
              label: 'Review collection',
              onClick: action(async () => {
                pick();
                useCatalogStore.getState().set({ pendingSelection: 'first' });
                const settings = useSettingsStore.getState().appSettings;
                if (settings)
                  await useSettingsStore
                    .getState()
                    .handleSettingsChange({ ...settings, libraryDisplayMode: LibraryDisplayMode.Cull });
              }),
            },
            {
              label: 'Auto Cull collection',
              onClick: action(async () => {
                const paths = await allPaths();
                useUIStore.getState().setUI({
                  cullingModalState: {
                    isOpen: true,
                    progress: null,
                    suggestions: null,
                    error: null,
                    pathsToCull: paths,
                  },
                });
              }),
            },
            ...(useCatalogStore
              .getState()
              .catalog?.storage?.sources.some((source) =>
                canCacheSource(
                  useCatalogStore.getState().catalog?.storage,
                  source.path + '/photo',
                  useCatalogStore.getState().catalog?.cache.directory,
                ),
              )
              ? [
                  {
                    label: 'Cache collection on faster drive',
                    onClick: action(async () => {
                      await invoke('library_cache', { paths: await allPaths(), action: 'cache' });
                      useCatalogStore.getState().set({ utilities: true });
                    }),
                  },
                  {
                    label: 'Pin collection on faster drive',
                    onClick: action(async () => {
                      await invoke('library_cache', { paths: await allPaths(), action: 'pin' });
                      useCatalogStore.getState().set({ utilities: true });
                    }),
                  },
                  {
                    label: 'Unpin collection',
                    onClick: action(async () => {
                      await invoke('library_cache', { paths: await allPaths(false), action: 'unpin' });
                    }),
                  },
                ]
              : [{ label: 'Configure storage…', onClick: () => useCatalogStore.getState().set({ utilities: true }) }]),
          ]
        : []),
    ]);
  };
  const active = useCatalogStore((s) => s.active && s.query.collectionId === collection.id);
  const target = useCatalogStore((s) => s.catalog?.target === collection.id);
  const { setNodeRef, isOver } = useDroppable({
    id: `catalog:${collection.id}`,
    disabled: collection.kind !== 'collection',
    data: { type: 'catalog-collection', id: collection.id },
  });
  const Icon = collection.kind === 'set' ? Folder : collection.kind === 'smart' ? Sparkles : Album;
  const pick = () => {
    useCatalogStore.getState().navigate('all', collection.id);
    useLibraryStore.getState().setLibrary({
      currentFolderPath: collection.name,
      activeAlbumId: null,
      multiSelectedPaths: [],
      libraryActivePath: null,
    });
    useUIStore.getState().setUI({ activeView: 'library' });
  };
  return (
    <div
      ref={setNodeRef}
      className={`group flex items-center gap-2 rounded-md py-2 pr-2 ${isOver ? 'bg-accent/20' : active ? 'bg-surface' : 'hover:bg-surface/50'}`}
      style={{ paddingLeft: 10 + depth * 14 }}
      onContextMenu={menu}
    >
      <button
        className="flex flex-1 min-w-0 items-center gap-2 text-left"
        onClick={collection.kind === 'set' ? toggle : pick}
      >
        <Icon size={16} className="shrink-0 text-text-secondary" />
        <span className="truncate">{collection.name}</span>
        {target && <span className="text-accent text-xs">{'●'}</span>}
        <span className="ml-auto text-xs text-text-secondary">{collection.kind === 'set' ? '' : collection.count}</span>
      </button>
      <button
        aria-label={`Edit ${collection.name}`}
        className="opacity-0 group-hover:opacity-100 focus:opacity-100 text-text-secondary hover:text-text-primary"
        onClick={menu}
      >
        <MoreHorizontal size={15} />
      </button>
    </div>
  );
}
export default function CatalogSources() {
  const { t } = useTranslation();
  const { active, query, catalog } = useCatalogStore();
  const [expanded, setExpanded] = useState(true);
  const [collectionsOpen, setCollectionsOpen] = useState(true);
  const [sessionsOpen, setSessionsOpen] = useState(false);
  const [openSets, setOpenSets] = useState(new Set<string>());
  const sessions = useCatalogStore((s) => s.sessions);
  const pick = (view: string, name: string) => {
    useCatalogStore.getState().navigate(view);
    useLibraryStore
      .getState()
      .setLibrary({ currentFolderPath: name, activeAlbumId: null, multiSelectedPaths: [], libraryActivePath: null });
    useUIStore.getState().setUI({ activeView: 'library' });
  };
  const rows = (parent: string | null, depth = 0): React.ReactNode =>
    catalog?.collections
      .filter((c) => c.parent === parent)
      .map((c) => (
        <div key={c.id}>
          <CollectionRow
            collection={c}
            depth={depth}
            toggle={() =>
              setOpenSets((current) => {
                const next = new Set(current);
                if (next.has(c.id)) next.delete(c.id);
                else next.add(c.id);
                return next;
              })
            }
          />
          {openSets.has(c.id) && rows(c.id, depth + 1)}
        </div>
      ));
  const heading = 'flex w-full items-center gap-2 py-3 text-text-secondary';
  return (
    <div className="text-sm text-text-primary pb-2">
      <button className={heading} onClick={() => setExpanded(!expanded)}>
        {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        <Text variant={TextVariants.small} weight={TextWeights.semibold} className="uppercase tracking-wide">
          {t('catalog.library')}
        </Text>
      </button>
      {expanded && (
        <div className="space-y-0.5">
          {(
            [
              ['all', t('catalog.allPhotos'), Database],
              ['picks', t('catalog.picks'), Flag],
              ['rejected', t('catalog.rejected'), X],
              ['unreviewed', t('catalog.unreviewed'), Circle],
              ['recent', t('catalog.recentlyAdded'), History],
              ['missing', t('catalog.unavailable'), Search],
            ] as const
          ).map(([view, label, Icon]) => (
            <button
              key={view}
              className={`w-full flex items-center gap-2 px-3 py-2 rounded-md text-left ${active && !query.collectionId && query.view === view ? 'bg-surface' : 'hover:bg-surface/50'}`}
              onClick={() => pick(view, label)}
            >
              <Icon size={16} className="text-text-secondary" />
              <span>{label}</span>
              <span className="ml-auto text-xs text-text-secondary">{catalog?.counts[view]?.toLocaleString()}</span>
            </button>
          ))}
        </div>
      )}
      <div className="flex items-center">
        <button className={heading} onClick={() => setCollectionsOpen(!collectionsOpen)}>
          {collectionsOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
          <Text variant={TextVariants.small} weight={TextWeights.semibold} className="uppercase tracking-wide">
            {t('catalog.collections')}
          </Text>
        </button>
        <button
          aria-label={t('catalog.createCollection')}
          className="p-2 text-text-secondary hover:text-text-primary"
          onClick={() =>
            useCatalogStore
              .getState()
              .set({ edit: { name: '', kind: 'collection', parent: null, rules: [], mode: 'all' } })
          }
        >
          <Plus size={14} />
        </button>
      </div>
      {collectionsOpen && rows(null)}
      <button
        className={heading}
        onClick={() => {
          setSessionsOpen(!sessionsOpen);
          invoke<Session[]>('library_session', { action: 'list', session: {} })
            .then((sessions) => useCatalogStore.getState().set({ sessions }))
            .catch((e) => toast.error(String(e)));
        }}
      >
        {sessionsOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        <Text variant={TextVariants.small} weight={TextWeights.semibold} className="uppercase tracking-wide">
          {t('catalog.reviewSessions')}
        </Text>
      </button>
      {sessionsOpen &&
        sessions.map((s) => (
          <button
            key={s.id}
            className="w-full flex gap-2 items-center text-left rounded-md px-3 py-2 hover:bg-surface"
            onClick={() =>
              void invoke<Session>('library_resume_session', { session: s })
                .then((resume) => {
                  if (resume.kind === 'culling') {
                    useUIStore.getState().setUI({
                      cullingModalState: {
                        isOpen: true,
                        progress: null,
                        suggestions: null,
                        error: null,
                        pathsToCull: resume.paths || [],
                        savedSettings: resume.settings,
                      },
                    });
                    return;
                  }
                  if (resume.criteria) useLibraryStore.getState().setLibrary(resume.criteria);
                  useLibraryStore.getState().setLibrary({ libraryActivePath: null, multiSelectedPaths: [] });
                  useCatalogStore
                    .getState()
                    .set({ active: true, query: resume.query, resume, pendingSelection: undefined });
                })
                .catch((e) => toast.error(String(e)))
            }
          >
            <History size={15} className="text-text-secondary" />
            <span className="truncate">{s.name}</span>
          </button>
        ))}
    </div>
  );
}
