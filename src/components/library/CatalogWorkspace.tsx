import { useTranslation } from 'react-i18next';
import { useCallback, useEffect, useRef } from 'react';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Loader2, Check, Undo2 } from 'lucide-react';
import { toast } from 'react-toastify';
import { Asset, Catalog, Query, Rule, Session, useCatalogStore } from '../../store/useCatalogStore';
import { useLibraryStore } from '../../store/useLibraryStore';
import { useProcessStore } from '../../store/useProcessStore';
import { useSettingsStore } from '../../store/useSettingsStore';
import { useUIStore } from '../../store/useUIStore';
import { Invokes, LibraryDisplayMode, LibraryViewMode } from '../ui/AppProperties';
import CatalogDialogs from './CatalogDialogs';
import NativeDetailDialog from './NativeDetailDialog';
import { CatalogFeed, FeedState } from './CatalogFeed';

// Supply the original grid and editor; there is no separate catalog browser.
export default function CatalogWorkspace() {
  const { t } = useTranslation();
  const {
    active,
    query,
    total,
    loaded,
    loading,
    loadError,
    previewPaths,
    refresh: revision,
    resume,
  } = useCatalogStore();
  const cullingOpen = useUIStore((s) => s.cullingModalState.isOpen);
  const settings = useSettingsStore((s) => s.appSettings);
  const filter = useLibraryStore((s) => s.filterCriteria);
  const search = useLibraryStore((s) => s.searchCriteria);
  const sort = useLibraryStore((s) => s.sortCriteria);
  const selected = useLibraryStore((s) => s.multiSelectedPaths);
  const activePath = useLibraryStore((s) => s.libraryActivePath);
  const imageList = useLibraryStore((s) => s.imageList);
  const currentAssetId = (imageList as Asset[]).find((a) => a.path === activePath)?.assetId;
  const sessionRef = useRef<Session | undefined>(undefined);
  const saveQueue = useRef<Promise<void>>(Promise.resolve());
  const scope = useCatalogStore((s) => s.scopeRevision);
  const lastScope = useRef(scope);
  if (lastScope.current !== scope) {
    sessionRef.current = undefined;
    lastScope.current = scope;
  }
  const isReview = settings?.libraryDisplayMode === LibraryDisplayMode.Cull;
  const mounted = useRef(true),
    activeRef = useRef(active);
  activeRef.current = active;
  const latest = useRef<Query>(query);
  const constraints: Rule[] = [...query.rules];
  if (filter.rating)
    constraints.push({
      field: 'rating',
      operator: filter.rating === -1 ? 'eq' : filter.ratingOperator || 'gte',
      value: filter.rating === -1 ? '0' : String(filter.rating),
    });
  if (filter.flagStatus && filter.flagStatus !== 'all')
    constraints.push({
      field: 'flag',
      operator: filter.flagStatus === 'excludeRejected' ? 'ne' : 'eq',
      value: filter.flagStatus === 'picked' ? 'pick' : 'reject',
    });
  if (filter.editedStatus && filter.editedStatus !== 'all')
    constraints.push({ field: 'edited', operator: 'eq', value: filter.editedStatus === 'editedOnly' ? '1' : '0' });
  if (filter.rawStatus && filter.rawStatus !== 'all')
    constraints.push({ field: 'raw', operator: 'eq', value: filter.rawStatus === 'rawOnly' ? '1' : '0' });
  latest.current = {
    ...query,
    offset: resume || useCatalogStore.getState().pendingAssetId ? query.offset : 0,
    recursive: query.folder ? settings?.libraryViewMode === LibraryViewMode.Recursive : query.recursive,
    search: search.text,
    sort: query.sort === 'manual' ? 'manual' : sort.key === 'date_taken' ? 'capture' : sort.key,
    rules: constraints,
    descending: sort.order === 'desc',
    colors: filter.colors,
    tags: search.tags,
    tagMode: search.mode === 'AND' ? 'all' : 'any',
  };
  const filters = JSON.stringify({ filter, search, sort, folderMode: settings?.libraryViewMode });
  const feed = useRef<CatalogFeed | null>(null);
  if (!feed.current)
    feed.current = new CatalogFeed(
      (query) => invoke('library_query', { query }),
      (state: FeedState) => {
        if (!mounted.current || !activeRef.current) return;
        useCatalogStore
          .getState()
          .set({
            total: state.total,
            loaded: state.assets.length,
            loading: state.loading,
            loadError: state.error,
            fileTotal: state.fileTotal,
          });
        if (state.loading || state.error) return;
        const assets = state.assets;
        useLibraryStore.getState().setLibrary({
          imageList: assets,
          imageRatings: Object.fromEntries(assets.map((a) => [a.path, a.rating])),
          exifByPath: Object.fromEntries(assets.map((a) => [a.path, a.exif || {}])),
          isViewLoading: false,
        });
        const { pendingSelection: pending, pendingAssetId, pendingAssetIndex } = useCatalogStore.getState();
        const chosen =
          assets.find((a) => a.assetId === pendingAssetId) ||
          (pendingAssetIndex !== undefined
            ? assets[pendingAssetIndex]
            : pending === 'last'
              ? assets.at(-1)
              : pending === 'first'
                ? assets[0]
                : undefined);
        if (chosen) {
          useCatalogStore
            .getState()
            .set({ pendingSelection: undefined, pendingAssetId: undefined, pendingAssetIndex: undefined });
          useLibraryStore.getState().setLibrary({
            libraryActivePath: chosen.path,
            multiSelectedPaths: [chosen.path],
            selectionAnchorPath: chosen.path,
          });
          useUIStore.getState().imageSelectHandler?.(chosen.path, false);
        }
        useProcessStore.getState().setProcess((s) => ({
          thumbnails: {
            ...s.thumbnails,
            ...Object.fromEntries(
              assets.filter((a) => a.smallPreview).map((a) => [a.path, convertFileSrc(a.smallPreview!)]),
            ),
          },
          mediumThumbnails: {
            ...s.mediumThumbnails,
            ...Object.fromEntries(
              assets.filter((a) => a.mediumPreview).map((a) => [a.path, convertFileSrc(a.mediumPreview!)]),
            ),
          },
        }));
      },
    );
  const refresh = useCallback(async () => {
    void invoke<Catalog>('library_state')
      .then((catalog) => {
        if (mounted.current) useCatalogStore.getState().set({ catalog });
      })
      .catch((e) => console.error('Failed to refresh library state:', e));
    if (!activeRef.current) return;
    feed.current!.setQuery(latest.current);
    await feed.current!.refresh((useCatalogStore.getState().pendingAssetIndex ?? -1) + 1);
  }, []);
  useEffect(() => {
    mounted.current = true;
    let timer: ReturnType<typeof setTimeout>;
    const off = listen('library-changed', () => {
      clearTimeout(timer);
      timer = setTimeout(() => void refresh(), 600);
    });
    useCatalogStore.getState().set({
      loadMore: (retry = false) => {
        if (!activeRef.current) return;
        if (retry && !feed.current!.state.assets.length) void refresh();
        else void feed.current!.more(retry);
      },
    });
    return () => {
      mounted.current = false;
      feed.current?.cancel();
      clearTimeout(timer);
      void off.then((fn) => fn());
      void invoke('library_previews', { paths: [] });
    };
  }, [refresh]);
  useEffect(() => {
    if (active) void refresh();
    else feed.current?.cancel();
  }, [active, query, filters, revision, refresh]);
  const previewCandidates = new Set(previewPaths || []);
  if (activePath) previewCandidates.add(activePath);
  const visibleAssets = (imageList as Asset[]).filter((asset) => previewCandidates.has(asset.path));
  const missing =
    active && !cullingOpen
      ? (visibleAssets.length ? visibleAssets : (imageList.slice(0, 60) as Asset[]))
          .filter((a) => a.available && (!a.smallPreview || !a.mediumPreview))
          .map((a) => a.path)
          .join('\n')
      : '';
  useEffect(() => {
    let cancelled = false;
    if (missing)
      void invoke('library_previews', { paths: missing.split('\n') })
        .then(() => {
          if (!cancelled) void refresh();
        })
        .catch((e) => {
          if (!cancelled) toast.error(String(e));
        });
    return () => {
      cancelled = true;
      if (missing) void invoke('library_previews', { paths: [] });
    };
  }, [missing, refresh]);
  useEffect(() => {
    if (!settings?.rootFolders?.length || useLibraryStore.getState().rootPaths.length) return;
    const roots = settings.rootFolders;
    useLibraryStore.getState().setLibrary({ rootPaths: roots });
    Promise.all(
      roots.map((path) =>
        invoke(Invokes.GetFolderTree, { path, expandedFolders: [path], showImageCounts: false }).catch(() => null),
      ),
    ).then((trees) => useLibraryStore.getState().setLibrary({ folderTrees: trees.filter(Boolean) }));
  }, [settings?.rootFolders]);
  useEffect(() => {
    if (!resume || !settings) return;
    sessionRef.current = resume;
    void useSettingsStore.getState().handleSettingsChange({ ...settings, libraryDisplayMode: LibraryDisplayMode.Cull });
    useCatalogStore.getState().set({ resume: undefined, pendingAssetId: resume.assetId, refresh: Date.now() });
  }, [resume, settings]);
  useEffect(() => {
    if (!active || !isReview || !activePath || resume) return;
    const asset = (imageList as Asset[]).find((a) => a.path === activePath);
    if (!asset || asset.assetId !== currentAssetId) return;
    const snapshot: Session = {
      name:
        useCatalogStore.getState().catalog?.collections.find((c) => c.id === query.collectionId)?.name ||
        'Photo review',
      query,
      resolvedQuery: latest.current,
      path: activePath,
      assetId: asset.assetId,
      completed: 0,
      criteria: { filterCriteria: filter, searchCriteria: search, sortCriteria: sort },
    };
    const scopeAtSave = scope;
    const timer = setTimeout(() => {
      saveQueue.current = saveQueue.current
        .then(async () => {
          if (lastScope.current !== scopeAtSave) return;
          const saved = await invoke<Session>('library_session', {
            action: 'save',
            session: { ...snapshot, id: sessionRef.current?.id, completed: sessionRef.current?.completed || 0 },
          });
          if (lastScope.current === scopeAtSave) sessionRef.current = saved;
        })
        .catch((e) => {
          toast.error(String(e));
        });
    }, 150);
    return () => clearTimeout(timer);
  }, [activePath, active, isReview, query, filter, search, sort, scope, resume, currentAssetId]);
  useEffect(() => {
    if (!active) return;
    const handler = (e: KeyboardEvent) => {
      if (
        useUIStore.getState().activeView !== 'library' ||
        (e.target as HTMLElement).closest('input,textarea,select,[contenteditable="true"]') ||
        e.isComposing ||
        useCatalogStore.getState().utilities ||
        !!useCatalogStore.getState().edit ||
        !!useCatalogStore.getState().detailPaths?.length
      )
        return;
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z') {
        e.preventDefault();
        e.stopImmediatePropagation();
        void invoke('library_undo')
          .then(refresh)
          .catch((error) => toast.error(String(error)));
      } else if (!e.ctrlKey && !e.metaKey && !e.altKey && e.key.toLowerCase() === 'b') {
        e.preventDefault();
        e.stopImmediatePropagation();
        const state = useCatalogStore.getState();
        void invoke('library_collection', {
          action: 'toggle',
          collection: { id: state.catalog?.target || 'quick' },
          paths: useLibraryStore.getState().multiSelectedPaths,
        })
          .then(refresh)
          .catch((error) => toast.error(String(error)));
      }
    };
    window.addEventListener('keydown', handler, true);
    return () => window.removeEventListener('keydown', handler, true);
  }, [active, refresh]);
  const current = (imageList as Asset[]).find((a) => a.path === activePath);
  const decide = (status: string) =>
    void invoke('library_edit', { paths: selected, field: 'review', value: status })
      .then(refresh)
      .catch((e) => toast.error(String(e)));
  return (
    <>
      <CatalogDialogs />
      <NativeDetailDialog />
      {active && (
        <div className="shrink-0 flex items-center justify-between gap-3 px-3 py-1.5 text-xs text-text-secondary bg-bg-secondary rounded-lg">
          <div className="flex items-center gap-3">
            <span>
              {total.toLocaleString()} {t('catalog.photos')}
              {(useCatalogStore.getState().fileTotal || 0) > total && (
                <span title={t('catalog.groupCopiesHint')}>
                  {' '}
                  · {t('catalog.copiesGrouped', { count: (useCatalogStore.getState().fileTotal || total) - total })}
                </span>
              )}
              {search.text || constraints.length || filter.colors.length || search.tags.length
                ? ' · Filters active'
                : ''}
            </span>
            {isReview && (
              <>
                <select
                  aria-label={t('catalog.reviewStatus')}
                  className="bg-transparent outline-none py-1"
                  value={current?.review || ''}
                  disabled={!selected.length}
                  onChange={(e) => decide(e.target.value)}
                >
                  <option value="" disabled>
                    {t('catalog.reviewStatus')}
                  </option>
                  <option value="unreviewed">{t('catalog.unreviewed')}</option>
                  <option value="reviewed">{t('catalog.reviewed')}</option>
                  <option value="deferred">{t('catalog.deferred')}</option>
                </select>
                <button
                  data-tooltip={t('catalog.markReviewed')}
                  disabled={!selected.length}
                  onClick={() => decide('reviewed')}
                  className="hover:text-text-primary disabled:opacity-30"
                >
                  <Check size={16} />
                </button>
              </>
            )}
          </div>
          <div className="flex items-center gap-3">
            {query.copyKey ? (
              <button
                className="hover:text-text-primary"
                onClick={() =>
                  useCatalogStore
                    .getState()
                    .set({ query: { ...query, copyKey: undefined, groupCopies: undefined, offset: 0 } })
                }
              >
                {t('catalog.backToGroupedView')}
              </button>
            ) : (
              <label className="flex items-center gap-2" title={t('catalog.groupCopiesHint')}>
                <input
                  type="checkbox"
                  checked={useCatalogStore.getState().catalog?.groupCopies || false}
                  onChange={(event) => {
                    void invoke('library_copy_view', { grouped: event.target.checked })
                      .then(refresh)
                      .catch((error) => toast.error(String(error)));
                  }}
                />
                {t('catalog.groupLikelyCopies')}
              </label>
            )}
            <button
              data-tooltip={t('catalog.undoLibraryDecision')}
              onClick={() =>
                void invoke('library_undo')
                  .then(refresh)
                  .catch((e) => toast.error(String(e)))
              }
            >
              <Undo2 size={15} />
            </button>
            {loading && (
              <span role="status" className="flex items-center gap-2">
                <Loader2 size={14} className="animate-spin" />
                {t('catalog.loadingPhotos')}
              </span>
            )}
            {loadError ? (
              <span role="alert" className="flex items-center gap-2 text-amber-400">
                {t('catalog.loadingFailed')}
                <button className="underline px-2 py-1" onClick={() => useCatalogStore.getState().loadMore?.(true)}>
                  {t('catalog.retry')}
                </button>
              </span>
            ) : (
              !loading &&
              loaded < total && (
                <button
                  className="px-2 py-1 hover:text-text-primary"
                  onClick={() => useCatalogStore.getState().loadMore?.()}
                >
                  {t('catalog.loadMorePhotos')}
                </button>
              )
            )}
          </div>
        </div>
      )}
    </>
  );
}
