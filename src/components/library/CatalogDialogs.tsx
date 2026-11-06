import { useTranslation } from 'react-i18next';
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { Database, FolderOpen, HardDrive, Loader2, Pause, Play, Plus, X } from 'lucide-react';
import { toast } from 'react-toastify';
import { useCatalogStore, Collection, Rule } from '../../store/useCatalogStore';
import { useLibraryStore } from '../../store/useLibraryStore';
import { useSettingsStore } from '../../store/useSettingsStore';
import Button from '../ui/Button';
import Text from '../ui/Text';
import { TextVariants, TextColors } from '../../types/typography';

const field =
  'w-full bg-surface rounded-md px-3 py-2 text-text-primary border border-transparent focus:border-accent/40 focus:outline-none';
const secondary = 'bg-surface text-text-primary shadow-none';
const gib = (bytes = 0) => `${(bytes / 1073741824).toFixed(1)} GiB`;
async function attempt(action: () => Promise<unknown>) {
  try {
    await action();
  } catch (error) {
    toast.error(String(error));
  }
}

export default function CatalogDialogs() {
  const { t } = useTranslation();
  const { utilities, edit, catalog } = useCatalogStore();
  const [draft, setDraft] = useState<Partial<Collection>>({});
  const [working, setWorking] = useState(false);
  const [oldPath, setOldPath] = useState('');
  const [result, setResult] = useState('');
  const [deleteRequested, setDeleteRequested] = useState(false);
  const [cacheLimits, setCacheLimits] = useState({ rawGib: 150, previewGib: 50, reserveGib: 100 });
  useEffect(() => {
    if (catalog?.cache.limits) setCacheLimits(catalog.cache.limits);
  }, [catalog?.cache.limits]);
  const appSettings = useSettingsStore((s) => s.appSettings);
  const configured = useLibraryStore((s) => s.rootPaths);
  const roots = [...new Set([...configured, ...(appSettings?.rootFolders || []), ...(catalog?.roots || [])])];
  const selected = useLibraryStore((s) => s.multiSelectedPaths);
  const close = () => useCatalogStore.getState().set({ utilities: false, edit: null });
  useEffect(() => {
    setDeleteRequested(false);
    if (edit) setDraft({ ...edit, rules: edit.rules || [], mode: edit.mode || 'all' });
  }, [edit]);
  useEffect(() => {
    if (!utilities && !edit) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopImmediatePropagation();
        close();
      }
    };
    window.addEventListener('keydown', handler, true);
    return () => window.removeEventListener('keydown', handler, true);
  }, [utilities, edit]);
  const job = async (kind: string, paths: string[]) => {
    await invoke('library_start_job', { kind, roots: paths });
    useCatalogStore.getState().set({ refresh: Date.now() });
  };
  const execute = (action: () => Promise<unknown>) =>
    void attempt(async () => {
      setWorking(true);
      try {
        await action();
      } finally {
        setWorking(false);
      }
    });
  const rule = (i: number, value: Partial<Rule>) =>
    setDraft((d) => ({ ...d, rules: d.rules?.map((r, j) => (i === j ? { ...r, ...value } : r)) }));
  if (!utilities && !edit) return null;
  return (
    <div
      className="fixed inset-0 z-[100] bg-black/60 backdrop-blur-sm flex items-center justify-center p-6"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) close();
      }}
    >
      <section
        role="dialog"
        aria-modal="true"
        aria-label={edit ? 'Collection editor' : 'Library utilities'}
        className="w-full max-w-2xl max-h-[85vh] flex flex-col rounded-xl bg-bg-secondary shadow-2xl border border-border-color/30 text-text-primary"
      >
        <header className="flex items-center justify-between p-5 border-b border-surface">
          <div className="flex items-center gap-3">
            <Database size={20} className="text-text-secondary" />
            <Text variant={TextVariants.heading}>
              {edit ? (edit.id ? 'Edit collection' : 'New collection') : 'Library utilities'}
            </Text>
          </div>
          <button
            aria-label={t('catalog.close')}
            onClick={close}
            className="p-2 rounded-md text-text-secondary hover:bg-surface"
          >
            <X size={20} />
          </button>
        </header>
        <div className="p-6 overflow-y-auto custom-scrollbar space-y-6">
          {utilities && (
            <>
              <div className="flex gap-4">
                <div className="p-3 h-fit bg-surface rounded-lg">
                  <FolderOpen size={23} className="text-text-secondary" />
                </div>
                <div className="flex-1 space-y-3">
                  <Text variant={TextVariants.heading}>{t('catalog.prepareYourPhotoLibrary')}</Text>
                  <Text color={TextColors.secondary}>
                    {t('catalog.buildThumbnailsAndPreviewsOnTheSsdForSmoothBrowsing')}
                  </Text>
                  <Button disabled={working || !roots.length} onClick={() => execute(() => job('prepare', roots))}>
                    {t('catalog.buildLibraryPreviews')}
                  </Button>
                  <button
                    className="text-sm text-text-secondary hover:text-text-primary"
                    onClick={() =>
                      execute(async () => {
                        const folder = await open({ directory: true, multiple: false });
                        if (typeof folder === 'string') await job('prepare', [folder]);
                      })
                    }
                  >
                    {t('catalog.chooseAnotherFolder')}
                  </button>
                </div>
              </div>
              {catalog?.jobs.length ? (
                <div className="space-y-3">
                  {catalog.jobs.map((j) => (
                    <div key={j.id} className="rounded-lg bg-surface/60 p-4 space-y-2">
                      <div className="flex items-center gap-3">
                        <span className="flex-1 font-medium">
                          {j.kind === 'prepare'
                            ? 'Library previews'
                            : j.kind === 'index'
                              ? 'Library scan'
                              : j.kind.startsWith('cache')
                                ? 'SSD copies'
                                : 'Previews'}
                        </span>
                        <span className="text-xs text-text-secondary">
                          {j.status} · {j.completed.toLocaleString()} / {j.total.toLocaleString()}
                        </span>
                        {['running', 'queued', 'paused'].includes(j.status) && (
                          <>
                            <button
                              aria-label={j.status === 'paused' ? 'Resume job' : 'Pause job'}
                              className="p-1 text-text-secondary hover:text-text-primary"
                              onClick={() =>
                                execute(() =>
                                  invoke('library_job', {
                                    id: j.id,
                                    action: j.status === 'paused' ? 'resume' : 'pause',
                                  }),
                                )
                              }
                            >
                              {j.status === 'paused' ? <Play size={16} /> : <Pause size={16} />}
                            </button>
                            <button
                              aria-label={t('catalog.cancelJob')}
                              className="p-1 text-text-secondary hover:text-text-primary"
                              onClick={() => execute(() => invoke('library_job', { id: j.id, action: 'cancel' }))}
                            >
                              <X size={16} />
                            </button>
                          </>
                        )}
                      </div>
                      <progress className="w-full h-1.5 accent-accent" value={j.completed} max={j.total || 1} />
                      {j.error && <p className="text-xs text-amber-400">{j.error}</p>}
                      {j.failed > 0 && (
                        <p className="text-xs text-text-secondary">
                          {j.failed}
                          {t('catalog.photosCouldNotBeProcessed')}
                        </p>
                      )}
                    </div>
                  ))}
                </div>
              ) : null}
              <div className="border-t border-surface pt-5 flex gap-4">
                <HardDrive size={22} className="text-text-secondary mt-1" />
                <div className="flex-1 space-y-2">
                  <Text variant={TextVariants.heading}>{t('catalog.ssdCache')}</Text>
                  <Text color={TextColors.secondary}>
                    {gib(catalog?.cache.used)} / {gib(catalog?.cache.rawLimit || 150 * 1073741824)} ·{' '}
                    {gib(catalog?.cache.pinned)} {t('catalog.pinned')}
                  </Text>
                  <div className="h-1.5 bg-surface rounded-full overflow-hidden">
                    <div
                      className="h-full bg-accent/70"
                      style={{
                        width: `${Math.min(100, ((catalog?.cache.used || 0) / (catalog?.cache.rawLimit || 150 * 1073741824)) * 100)}%`,
                      }}
                    />
                  </div>
                  <Text variant={TextVariants.small} color={TextColors.secondary}>
                    {t('catalog.previewsAndAnalysis')} {gib(catalog?.cache.previewUsed)} /{' '}
                    {gib(catalog?.cache.previewLimit || 50 * 1073741824)}
                    {t('catalog.freeSpaceReserve')} {gib(catalog?.cache.reserve || 100 * 1073741824)}.
                  </Text>
                </div>
              </div>
              <details className="border-t border-surface pt-4">
                <summary className="cursor-pointer text-text-secondary text-sm">
                  {t('catalog.maintenanceAndStorage')}
                </summary>
                <div className="space-y-4 pt-4">
                  <div className="grid grid-cols-3 gap-3">
                    {(
                      [
                        ['rawGib', 'RAW cache'],
                        ['previewGib', 'Previews and analysis'],
                        ['reserveGib', 'Keep free'],
                      ] as const
                    ).map(([key, label]) => (
                      <label key={key} className="text-xs text-text-secondary space-y-1 block">
                        {label}
                        {t('catalog.gib')}
                        <input
                          type="number"
                          min={key === 'previewGib' ? 1 : 0}
                          max={1048576}
                          className={field}
                          value={cacheLimits[key]}
                          onChange={(e) => setCacheLimits({ ...cacheLimits, [key]: Number(e.target.value) })}
                        />
                      </label>
                    ))}
                  </div>
                  <Button
                    className={secondary}
                    disabled={working}
                    onClick={() => execute(() => invoke('library_cache_limits', { value: cacheLimits }))}
                  >
                    {t('catalog.saveCacheLimits')}
                  </Button>
                  <div className="text-xs text-text-secondary break-all space-y-1">
                    <p>
                      {t('catalog.catalog')}
                      {catalog?.directory}
                    </p>
                    <p>
                      {t('catalog.cache')}
                      {catalog?.cache.directory}
                    </p>
                    <p>
                      {t('catalog.photoFolders')}
                      {roots.join(' · ')}
                    </p>
                  </div>
                  <div className="flex flex-wrap gap-2">
                    <Button
                      className={secondary}
                      disabled={working}
                      onClick={() =>
                        execute(async () => {
                          const path = await invoke<string>('library_backup');
                          setResult(`Recovery snapshot saved: ${path}`);
                        })
                      }
                    >
                      {t('catalog.createRecoverySnapshot')}
                    </Button>
                    <Button className={secondary} disabled={working} onClick={() => execute(() => job('index', roots))}>
                      {t('catalog.rescanFolders')}
                    </Button>
                    <Button
                      className={secondary}
                      onClick={() =>
                        execute(async () => {
                          const path = await open({ filters: [{ name: 'RapidRAW albums', extensions: ['json'] }] });
                          if (typeof path === 'string') {
                            const n = await invoke<number>('library_import_albums', { path });
                            setResult(`Copied ${n} albums and sets. Recovery copies are in Library/Migrations.`);
                          }
                        })
                      }
                    >
                      {t('catalog.copyRapidrawAlbums')}
                    </Button>
                  </div>
                  <div className="flex items-center gap-2">
                    <input
                      aria-label={t('catalog.recordedFolderToRelink')}
                      placeholder={t('catalog.recordedFolderPathIncludingAnUnavailableDrive')}
                      className={field}
                      value={oldPath}
                      onChange={(e) => setOldPath(e.target.value)}
                    />
                    <Button
                      className={`${secondary} shrink-0`}
                      disabled={!oldPath.trim()}
                      onClick={() =>
                        execute(async () => {
                          const newRoot = await open({
                            directory: true,
                            title: 'Choose the folder at its new location',
                          });
                          if (typeof newRoot === 'string') {
                            await invoke('library_relink', { oldRoot: oldPath.trim(), newRoot });
                            setResult('Folder relinked after checking recorded file identities.');
                          }
                        })
                      }
                    >
                      {t('catalog.relink')}
                    </Button>
                  </div>
                  <Button
                    className={secondary}
                    disabled={selected.length < 2}
                    onClick={() =>
                      execute(async () => {
                        const groups = await invoke<string[][]>('library_exact_duplicates', { paths: selected });
                        setResult(
                          groups.length
                            ? `Verified identical files: ${JSON.stringify(groups)}`
                            : 'No exact duplicates in the selection.',
                        );
                      })
                    }
                  >
                    {t('catalog.verifyExactDuplicatesInSelection')}
                  </Button>
                  {result && <p className="text-xs text-text-secondary break-all">{result}</p>}
                </div>
              </details>
            </>
          )}
          {deleteRequested && (
            <Text color={TextColors.secondary}>
              {t('catalog.deleteThisCollectionAndItsNestedCollectionsOriginalFilesAnd')}
            </Text>
          )}
          {edit && (
            <>
              <label className="block space-y-2">
                <Text variant={TextVariants.small} color={TextColors.secondary}>
                  {t('catalog.name')}
                </Text>
                <input
                  autoFocus
                  aria-label={t('catalog.collectionName')}
                  className={field}
                  value={draft.name || ''}
                  onChange={(e) => setDraft({ ...draft, name: e.target.value })}
                />
              </label>
              <div className="grid grid-cols-2 gap-4">
                <label className="space-y-2">
                  <Text variant={TextVariants.small} color={TextColors.secondary}>
                    {t('catalog.type')}
                  </Text>
                  <select
                    className={field}
                    aria-label={t('catalog.collectionType')}
                    value={draft.kind}
                    onChange={(e) => setDraft({ ...draft, kind: e.target.value })}
                  >
                    <option value="collection">{t('catalog.collection')}</option>
                    <option value="smart">{t('catalog.smartCollection')}</option>
                    <option value="set">{t('catalog.collectionSet')}</option>
                  </select>
                </label>
                <label className="space-y-2">
                  <Text variant={TextVariants.small} color={TextColors.secondary}>
                    {t('catalog.inside')}
                  </Text>
                  <select
                    className={field}
                    aria-label={t('catalog.parentCollectionSet')}
                    value={draft.parent || ''}
                    onChange={(e) => setDraft({ ...draft, parent: e.target.value || null })}
                  >
                    <option value="">{t('catalog.topLevel')}</option>
                    {catalog?.collections
                      .filter((c) => c.kind === 'set' && c.id !== draft.id)
                      .map((c) => (
                        <option key={c.id} value={c.id}>
                          {c.name}
                        </option>
                      ))}
                  </select>
                </label>
              </div>
              {draft.kind === 'smart' && (
                <div className="space-y-3">
                  <div className="flex items-center gap-2">
                    <span>{t('catalog.match')}</span>
                    <select
                      aria-label={t('catalog.matchConditions')}
                      className={`${field} !w-auto`}
                      value={draft.mode}
                      onChange={(e) => setDraft({ ...draft, mode: e.target.value })}
                    >
                      <option value="all">{t('catalog.allConditions')}</option>
                      <option value="any">{t('catalog.anyCondition')}</option>
                    </select>
                  </div>
                  {draft.rules?.map((r, i) => (
                    <div key={i} className="flex gap-2">
                      <select
                        className={field}
                        aria-label={`Rule ${i + 1} field`}
                        value={r.field}
                        onChange={(e) => {
                          const name = e.target.value;
                          rule(i, {
                            field: name,
                            operator: 'eq',
                            value:
                              name === 'rating'
                                ? '3'
                                : ['edited', 'available'].includes(name)
                                  ? '1'
                                  : name === 'flag'
                                    ? 'pick'
                                    : name === 'review'
                                      ? 'unreviewed'
                                      : name === 'label'
                                        ? 'red'
                                        : '',
                          });
                        }}
                      >
                        {[
                          ['rating', t('catalog.rating')],
                          ['flag', t('catalog.flag')],
                          ['label', t('catalog.colorLabel')],
                          ['tag', t('catalog.keyword')],
                          ['date', t('catalog.captureDate')],
                          ['camera', t('catalog.camera')],
                          ['lens', t('catalog.lens')],
                          ['edited', t('catalog.edited')],
                          ['available', t('catalog.available')],
                          ['review', t('catalog.reviewStatus')],
                          ['path', t('catalog.folderPath')],
                        ].map(([value, label]) => (
                          <option key={value} value={value}>
                            {label}
                          </option>
                        ))}
                      </select>
                      <select
                        className={field}
                        aria-label={`Rule ${i + 1} condition`}
                        value={r.operator}
                        onChange={(e) => rule(i, { operator: e.target.value })}
                      >
                        {(r.field === 'rating'
                          ? [
                              ['eq', 'is'],
                              ['ne', 'is not'],
                              ['gte', 'at least'],
                              ['lte', 'at most'],
                            ]
                          : r.field === 'date'
                            ? [
                                ['eq', 'on'],
                                ['ne', 'not on'],
                                ['before', 'before'],
                                ['after', 'after'],
                              ]
                            : ['flag', 'review', 'edited', 'available', 'label'].includes(r.field)
                              ? [
                                  ['eq', 'is'],
                                  ['ne', 'is not'],
                                ]
                              : [
                                  ['eq', 'is'],
                                  ['ne', 'is not'],
                                  ['contains', 'contains'],
                                ]
                        ).map(([value, label]) => (
                          <option key={value} value={value}>
                            {label}
                          </option>
                        ))}
                      </select>
                      {['rating', 'flag', 'review', 'edited', 'available', 'label'].includes(r.field) ? (
                        <select
                          className={field}
                          aria-label={`Rule ${i + 1} value`}
                          value={r.value}
                          onChange={(e) => rule(i, { value: e.target.value })}
                        >
                          {(r.field === 'rating'
                            ? [0, 1, 2, 3, 4, 5].map((n) => [String(n), `${n} stars`])
                            : r.field === 'flag'
                              ? [
                                  ['pick', 'Pick'],
                                  ['reject', 'Rejected'],
                                  ['', 'Unflagged'],
                                ]
                              : r.field === 'review'
                                ? [
                                    ['unreviewed', 'Unreviewed'],
                                    ['reviewed', 'Reviewed'],
                                    ['deferred', 'Deferred'],
                                  ]
                                : r.field === 'label'
                                  ? [
                                      ['', 'No label'],
                                      ...['red', 'yellow', 'green', 'blue', 'purple'].map((v) => [v, v]),
                                    ]
                                  : [
                                      ['1', 'Yes'],
                                      ['0', 'No'],
                                    ]
                          ).map(([value, label]) => (
                            <option key={value} value={value}>
                              {label}
                            </option>
                          ))}
                        </select>
                      ) : (
                        <input
                          type={r.field === 'date' ? 'date' : 'text'}
                          className={field}
                          aria-label={`Rule ${i + 1} value`}
                          value={r.value}
                          onChange={(e) => rule(i, { value: e.target.value })}
                        />
                      )}
                      <button
                        aria-label={t('catalog.removeCondition')}
                        className="text-text-secondary"
                        onClick={() => setDraft({ ...draft, rules: draft.rules?.filter((_, j) => j !== i) })}
                      >
                        <X size={16} />
                      </button>
                    </div>
                  ))}
                  <button
                    className="flex gap-2 items-center py-2 text-text-secondary hover:text-text-primary"
                    onClick={() =>
                      setDraft({
                        ...draft,
                        rules: [...(draft.rules || []), { field: 'rating', operator: 'gte', value: '3' }],
                      })
                    }
                  >
                    <Plus size={15} />
                    {t('catalog.addCondition')}
                  </button>
                </div>
              )}
            </>
          )}
        </div>
        <footer className="border-t border-surface p-4 flex gap-2 items-center justify-end">
          {edit?.id && edit.id !== 'quick' && (
            <button
              className="mr-auto text-sm text-text-secondary hover:text-red-400"
              onClick={() =>
                deleteRequested
                  ? execute(async () => {
                      await invoke('library_collection', { action: 'delete', collection: { id: edit.id }, paths: [] });
                      useCatalogStore.getState().navigate('all');
                      close();
                    })
                  : setDeleteRequested(true)
              }
            >
              {deleteRequested ? 'Confirm deletion' : 'Delete collection…'}
            </button>
          )}
          {working && <Loader2 size={18} className="animate-spin mr-auto text-text-secondary" />}
          {edit?.id && edit.kind === 'collection' && (
            <Button
              className={`${secondary} mr-auto`}
              onClick={() =>
                execute(async () => {
                  await invoke('library_collection', { action: 'target', collection: { id: edit.id }, paths: [] });
                  close();
                })
              }
            >
              {t('catalog.setAsTarget')}
            </Button>
          )}
          <Button className={secondary} onClick={close}>
            {edit ? 'Cancel' : 'Done'}
          </Button>
          {edit && (
            <Button
              disabled={!draft.name?.trim() || working}
              onClick={() =>
                execute(async () => {
                  await invoke('library_collection', { action: 'save', collection: draft, paths: [] });
                  close();
                })
              }
            >
              {t('catalog.saveCollection')}
            </Button>
          )}
        </footer>
      </section>
    </div>
  );
}
