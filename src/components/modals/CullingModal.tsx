import CullingCompare from '../library/CullingCompare';
import { toast } from 'react-toastify';
import { useState, useEffect, useCallback, useRef, useMemo } from 'react';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { useTranslation } from 'react-i18next';
import { CheckCircle, XCircle, Loader2, Users, ChevronLeft, ChevronRight, X } from 'lucide-react';
import { AnimatePresence, motion } from 'framer-motion';
import { CullingSettings, CullingSuggestions, Invokes, Progress } from '../ui/AppProperties';
import Button from '../ui/Button';
import Switch from '../ui/Switch';
import Slider from '../ui/Slider';
import Text from '../ui/Text';
import { TextColors, TextVariants } from '../../types/typography';
import { useProcessStore } from '../../store/useProcessStore';

const EMPTY_THUMBNAILS: Record<string, string> = {};

interface CullingModalProps {
  isOpen: boolean;
  onClose(): void;
  progress: Progress | null;
  suggestions: CullingSuggestions | null;
  error: string | null;
  imagePaths: string[];
  initialSettings?: CullingSettings;
  onApply(paths: string[], decisions: Array<{ id: string; data: unknown }>): Promise<void>;
  onError(error: string): void;
}

function ImageThumbnail({
  path,
  thumbnails,
  isSelected,
  onToggle,
  children,
}: {
  path: string;
  thumbnails: Record<string, string>;
  isSelected: boolean;
  onToggle: () => void;
  children?: React.ReactNode;
}) {
  const { t } = useTranslation();
  const filename = path.split(/[\\/]/).at(-1);
  return (
    <div
      className={`rounded-lg overflow-hidden border-2 min-w-0 ${isSelected ? 'border-red-400' : 'border-transparent'} bg-bg-secondary`}
    >
      <button
        type="button"
        aria-pressed={isSelected}
        aria-label={filename}
        onClick={onToggle}
        className="relative block w-full aspect-[3/2] bg-black/30 group focus-visible:outline focus-visible:outline-accent"
      >
        {thumbnails[path] ? (
          <img src={thumbnails[path]} alt={filename} className="w-full h-full object-contain" />
        ) : (
          <Loader2 className="absolute inset-0 m-auto animate-spin text-text-secondary" />
        )}
        {isSelected && (
          <span className="absolute top-2 right-2 rounded-full bg-black/70 p-1">
            <XCircle size={20} className="text-red-400" />
          </span>
        )}
      </button>
      <div className="p-2 space-y-2 text-xs">
        <div title={path} className="truncate text-text-secondary">
          {filename}
        </div>
        <button
          type="button"
          onClick={onToggle}
          className={`w-full rounded-md px-2 py-1.5 ${isSelected ? 'bg-red-400/15 text-red-300' : 'bg-surface text-text-secondary hover:text-text-primary'}`}
        >
          {isSelected ? t('catalog.selectedForRejection') : t('catalog.markForRejection')}
        </button>
        <div className="space-y-2">{children}</div>
      </div>
    </div>
  );
}

export default function CullingModal({
  isOpen,
  onClose: closeRequested,
  progress,
  suggestions,
  error,
  imagePaths,
  initialSettings,
  onApply,
  onError,
}: CullingModalProps) {
  const { t } = useTranslation();
  const [isMounted, setIsMounted] = useState(false);
  const [show, setShow] = useState(false);
  const smallThumbnails = useProcessStore((s) => (isMounted ? s.thumbnails : EMPTY_THUMBNAILS));
  const mediumThumbnails = useProcessStore((s) => (isMounted ? s.mediumThumbnails : EMPTY_THUMBNAILS));
  const thumbnails = useMemo(() => ({ ...smallThumbnails, ...mediumThumbnails }), [smallThumbnails, mediumThumbnails]);
  const [comparison, setComparison] = useState<string[] | null>(null);
  const [stage, setStage] = useState<'settings' | 'progress' | 'results'>('settings');
  const requestId = useRef(0);
  const onClose = () => {
    setComparison(null);
    requestId.current += 1;
    if (stage === 'progress') invoke(Invokes.CancelCulling).catch((error) => toast.error(String(error)));
    closeRequested();
  };

  const [settings, setSettings] = useState<CullingSettings>({
    groupSimilar: true,
    similarityThreshold: 28,
    filterBlurry: true,
    blurThreshold: 100.0,
    burstWindowSeconds: 5,
  });

  useEffect(() => {
    if (isOpen && initialSettings) setSettings(initialSettings);
  }, [isOpen, initialSettings]);
  const [selectedRejects, setSelectedRejects] = useState<Set<string>>(new Set());
  const [isApplying, setIsApplying] = useState(false);
  const [groups, setGroups] = useState<CullingSuggestions['similarGroups']>([]);
  const [activeTab, setActiveTab] = useState<'similar' | 'blurry'>('similar');
  const [page, setPage] = useState(0);
  const [dismissedBlurry, setDismissedBlurry] = useState(new Set<string>());
  useEffect(() => setDismissedBlurry(new Set()), [suggestions]);
  useEffect(() => setPage(0), [activeTab, suggestions]);
  const visibleGroups = groups.slice(page, page + 1);
  const filteredBlurry = (suggestions?.blurryImages || []).filter((i) => !dismissedBlurry.has(i.path));
  const visibleBlurry = filteredBlurry.slice(page * 50, page * 50 + 50);
  const visiblePaths = (
    activeTab === 'similar'
      ? visibleGroups.flatMap((g) => [g.representative.path, ...g.duplicates.map((d) => d.path)])
      : visibleBlurry.map((i) => i.path)
  ).join('\n');
  useEffect(() => {
    if (stage !== 'results' || !visiblePaths) return;
    let cancelled = false;
    void (async () => {
      const paths = visiblePaths.split('\n').filter((p) => !useProcessStore.getState().mediumThumbnails[p]);
      for (let i = 0; i < paths.length && !cancelled; i += 250) {
        const previews = await invoke<Record<string, { smallPreview: string; mediumPreview?: string }>>(
          'library_previews',
          {
            paths: paths.slice(i, i + 250),
          },
        );
        if (cancelled) return;
        useProcessStore.getState().setProcess((s) => ({
          mediumThumbnails: {
            ...s.mediumThumbnails,
            ...Object.fromEntries(
              Object.entries(previews)
                .filter(([, value]) => value.mediumPreview)
                .map(([path, value]) => [path, convertFileSrc(value.mediumPreview!)]),
            ),
          },
          thumbnails: {
            ...s.thumbnails,
            ...Object.fromEntries(
              Object.entries(previews).map(([path, value]) => [path, convertFileSrc(value.smallPreview)]),
            ),
          },
        }));
      }
    })().catch((error) => toast.error(String(error)));
    return () => {
      cancelled = true;
    };
  }, [visiblePaths, stage]);
  const pages = Math.ceil(activeTab === 'similar' ? groups.length : filteredBlurry.length / 50);
  useEffect(() => {
    if (page >= pages) setPage(Math.max(0, pages - 1));
  }, [pages, page]);

  useEffect(() => {
    if (isOpen) {
      setIsMounted(true);
      const timer = setTimeout(() => setShow(true), 10);
      return () => clearTimeout(timer);
    } else {
      setShow(false);
      const timer = setTimeout(() => {
        setIsMounted(false);
        setStage('settings');
        setSelectedRejects(new Set());
      }, 300);
      return () => clearTimeout(timer);
    }
  }, [isOpen]);

  useEffect(() => {
    if (suggestions || error) {
      setStage('results');
    } else if (progress) {
      setStage('progress');
    } else if (isOpen) {
      setStage('settings');
    }
  }, [progress, suggestions, error, isOpen]);

  useEffect(() => {
    if (stage === 'results' && suggestions) {
      setSelectedRejects(new Set());
      setGroups(suggestions.similarGroups);
      setActiveTab(suggestions.similarGroups.length > 0 ? 'similar' : 'blurry');
    }
  }, [stage, suggestions]);

  const handleStartCulling = useCallback(async () => {
    const request = ++requestId.current;
    try {
      await invoke(Invokes.CullImages, { paths: imagePaths, settings });
    } catch (err) {
      console.error('Culling failed to start:', err);
      if (requestId.current === request) onError(String(err));
    }
  }, [imagePaths, settings, onError]);

  const handleToggleReject = (path: string) => {
    setSelectedRejects((prev) => {
      const newSet = new Set(prev);
      if (newSet.has(path)) {
        newSet.delete(path);
      } else {
        newSet.add(path);
      }
      return newSet;
    });
  };

  const handleApply = async () => {
    setIsApplying(true);
    const decisions = groups
      .filter((g) => g.duplicates.some((d) => selectedRejects.has(d.path)))
      .map((g) => ({
        id: g.id,
        data: {
          ...g,
          rejected: [...selectedRejects].filter((p) => g.duplicates.some((d) => d.path === p)),
          provenance: 'preview-provisional-v3',
        },
      }));
    const blurry = (suggestions?.blurryImages || [])
      .filter((i) => selectedRejects.has(i.path))
      .map((i) => ({ id: `blur:${i.assetId || i.path}:${i.sourceRevision}`, data: i }));
    try {
      await onApply([...selectedRejects], [...decisions, ...blurry]);
    } catch (error) {
      onError(String(error));
    } finally {
      setIsApplying(false);
    }
  };

  const numSimilar = suggestions?.similarGroups.reduce((acc, group) => acc + group.duplicates.length, 0) || 0;
  const numBlurry = suggestions?.blurryImages.length || 0;

  useEffect(() => {
    if (!isOpen || stage !== 'results' || comparison) return;
    const handler = (event: KeyboardEvent) => {
      if (
        (event.target as HTMLElement).closest('input,select,textarea') ||
        event.ctrlKey ||
        event.metaKey ||
        event.altKey
      )
        return;
      if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
        event.preventDefault();
        event.stopImmediatePropagation();
        setPage((current) => Math.max(0, Math.min(pages - 1, current + (event.key === 'ArrowRight' ? 1 : -1))));
      }
    };
    window.addEventListener('keydown', handler, true);
    return () => window.removeEventListener('keydown', handler, true);
  }, [isOpen, stage, pages, comparison]);

  const navigation = pages > 0 && (
    <div className="flex flex-wrap items-center gap-3 py-3 border-b border-surface">
      <button
        aria-label={t(activeTab === 'similar' ? 'catalog.previousGroup' : 'catalog.previousPage')}
        className="p-2 rounded-md hover:bg-surface disabled:opacity-30"
        disabled={page === 0}
        onClick={() => setPage((current) => current - 1)}
      >
        <ChevronLeft size={20} />
      </button>
      <label className="flex items-center gap-2 text-sm">
        {t(activeTab === 'similar' ? 'catalog.jumpToGroup' : 'catalog.previousPage')}
        <input
          aria-label={t('catalog.jumpToGroup')}
          type="number"
          min={1}
          max={pages}
          value={page + 1}
          onChange={(event) => {
            const value = Number(event.target.value);
            if (value >= 1 && value <= pages) setPage(value - 1);
          }}
          className="w-20 rounded-md bg-surface px-2 py-1 text-center"
        />
        / {pages.toLocaleString()}
      </label>
      <button
        aria-label={t(activeTab === 'similar' ? 'catalog.nextGroup' : 'catalog.nextPage')}
        className="p-2 rounded-md hover:bg-surface disabled:opacity-30"
        disabled={page + 1 >= pages}
        onClick={() => setPage((current) => current + 1)}
      >
        <ChevronRight size={20} />
      </button>
      {activeTab === 'similar' && visibleGroups[0] && (
        <button
          className="ml-auto text-sm px-3 py-2 rounded-md hover:bg-surface"
          onClick={() => {
            const paths = visibleGroups[0].duplicates.map((image) => image.path);
            setSelectedRejects((current) => {
              const next = new Set(current);
              const clear = paths.every((path) => current.has(path));
              paths.forEach((path) => (clear ? next.delete(path) : next.add(path)));
              return next;
            });
          }}
        >
          {t(
            visibleGroups[0].duplicates.every((image) => selectedRejects.has(image.path))
              ? 'catalog.clearGroupSelection'
              : 'catalog.selectGroupAlternatives',
          )}
        </button>
      )}
    </div>
  );

  const renderSettings = () => (
    <>
      <div className="flex items-center justify-center mb-4">
        <Users className="w-12 h-12 text-accent" />
      </div>
      <Text variant={TextVariants.title} className="mb-6 text-center">
        {t('modals.culling.title')}
      </Text>
      <div className="space-y-6 text-sm">
        <div>
          <Switch
            label={t('modals.culling.groupSimilar')}
            checked={settings.groupSimilar}
            onChange={(v) => setSettings((s) => ({ ...s, groupSimilar: v }))}
          />
          {settings.groupSimilar && (
            <div className="mt-2 pl-4 border-l-2 border-border-color ml-1">
              <Slider
                label={t('modals.culling.similarityThreshold')}
                min={1}
                max={64}
                step={1}
                value={settings.similarityThreshold}
                defaultValue={28}
                onChange={(e) => setSettings((s) => ({ ...s, similarityThreshold: Number(e.target.value) }))}
                fillOrigin="min"
              />
              <Text variant={TextVariants.small} className="mt-1">
                {t('modals.culling.similarityThresholdDesc')}
              </Text>
              <Slider
                label={t('modals.culling.burstWindow')}
                min={1}
                max={60}
                step={1}
                value={settings.burstWindowSeconds ?? 5}
                defaultValue={5}
                onChange={(event) =>
                  setSettings((current) => ({ ...current, burstWindowSeconds: Number(event.target.value) }))
                }
                fillOrigin="min"
              />
            </div>
          )}
        </div>
        <div>
          <Switch
            label={t('modals.culling.filterBlurry')}
            checked={settings.filterBlurry}
            onChange={(v) => setSettings((s) => ({ ...s, filterBlurry: v }))}
          />
          {settings.filterBlurry && (
            <div className="mt-2  pl-4 border-l-2 border-border-color ml-1">
              <Slider
                label={t('modals.culling.blurThreshold')}
                min={25}
                max={500}
                step={25}
                value={settings.blurThreshold}
                defaultValue={100.0}
                onChange={(e) => setSettings((s) => ({ ...s, blurThreshold: Number(e.target.value) }))}
                fillOrigin="min"
              />
              <Text variant={TextVariants.small} className="mt-1">
                {t('modals.culling.blurThresholdDesc')}
              </Text>
            </div>
          )}
        </div>
      </div>
      <div className="flex justify-end gap-3 mt-8">
        <Switch
          label="Include reviewed suggestions"
          checked={settings.includeReviewed ?? false}
          onChange={(value) => setSettings((s) => ({ ...s, includeReviewed: value }))}
        />
        <button
          className="px-4 py-2 rounded-md text-text-secondary hover:bg-surface transition-colors"
          onClick={onClose}
        >
          {t('modals.culling.cancel')}
        </button>
        <Button onClick={handleStartCulling}>{t('modals.culling.startCulling')}</Button>
      </div>
    </>
  );

  const renderProgress = () => (
    <div className="flex flex-col items-center justify-center min-h-48 gap-3">
      <Loader2 className="w-16 h-16 text-accent animate-spin" />
      <p className="mt-4 text-text-primary">{progress?.stage || t('modals.culling.starting')}</p>
      {progress && progress.total > 0 && (
        <div className="w-full bg-surface rounded-full h-2.5 mt-2">
          <div
            className="bg-accent h-2.5 rounded-full"
            style={{ width: `${((progress.current ?? progress.completed ?? 0) / progress.total) * 100}%` }}
          />
        </div>
      )}
      <Button onClick={onClose}>{t('modals.culling.cancel')}</Button>
    </div>
  );

  const renderResults = () => {
    if (error) {
      return (
        <div className="flex flex-col items-center justify-center h-48">
          <XCircle className="w-16 h-16 text-red-500" />
          <Text variant={TextVariants.heading} className="mt-4 text-center">
            {t('modals.culling.cullingFailed')}
          </Text>
          <Text>{error}</Text>
          <div className="mt-6">
            <Button onClick={onClose}>{t('modals.culling.close')}</Button>
          </div>
        </div>
      );
    }

    if (!suggestions) return null;

    const totalSuggestions = numSimilar + numBlurry;
    if (totalSuggestions === 0) {
      return (
        <div className="flex flex-col items-center justify-center h-48">
          <CheckCircle className="w-16 h-16 text-green-500" />
          <Text variant={TextVariants.heading} className="mt-4">
            {t(suggestions.failedPaths.length > 0 ? 'modals.culling.cullingFailed' : 'modals.culling.noIssuesFound')}
          </Text>
          <Text>
            {suggestions.failedPaths.length > 0
              ? t('modals.culling.partialFailure', { count: suggestions.failedPaths.length })
              : t('modals.culling.noIssuesDesc')}
          </Text>
          <div className="mt-6">
            <Button onClick={onClose}>{t('modals.culling.done')}</Button>
          </div>
        </div>
      );
    }

    return (
      <>
        <Text variant={TextVariants.title} className="mb-4">
          {t('modals.culling.cullingSuggestions')}
        </Text>
        {suggestions.failedPaths.length > 0 && (
          <Text role="status">{t('modals.culling.partialFailure', { count: suggestions.failedPaths.length })}</Text>
        )}
        <div className="border-b border-surface shrink-0">
          <nav className="-mb-px flex space-x-4" aria-label="Tabs">
            {numSimilar > 0 && (
              <button
                onClick={() => setActiveTab('similar')}
                className={`${
                  activeTab === 'similar'
                    ? 'border-accent text-accent'
                    : 'border-transparent text-text-secondary hover:text-text-primary hover:border-gray-300'
                } whitespace-nowrap py-2 px-1 border-b-2 font-medium text-sm`}
              >
                {t('modals.culling.similarGroupsTab')}{' '}
                <span className="bg-surface text-text-secondary rounded-full px-2 py-0.5 text-xs">{groups.length}</span>
              </button>
            )}
            {numBlurry > 0 && (
              <button
                onClick={() => setActiveTab('blurry')}
                className={`${
                  activeTab === 'blurry'
                    ? 'border-accent text-accent'
                    : 'border-transparent text-text-secondary hover:text-text-primary hover:border-gray-300'
                } whitespace-nowrap py-2 px-1 border-b-2 font-medium text-sm`}
              >
                {t('modals.culling.blurryImagesTab')}{' '}
                <span className="bg-surface text-text-secondary rounded-full px-2 py-0.5 text-xs">{numBlurry}</span>
              </button>
            )}
          </nav>
        </div>

        {navigation}
        <div
          key={activeTab + page}
          className="bg-bg-primary rounded-lg p-3 flex-1 min-h-0 overflow-y-auto custom-scrollbar"
        >
          <AnimatePresence mode="wait">
            <motion.div
              key={activeTab}
              initial={{ opacity: 0, y: 10 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -10 }}
              transition={{ duration: 0.2 }}
            >
              {activeTab === 'similar' && (
                <div className="space-y-4">
                  {visibleGroups.map((group) => (
                    <div key={group.id} className="bg-surface rounded-lg p-3">
                      <div className="flex gap-2 mb-2">
                        <button
                          className="px-2 py-1 rounded border border-white/20"
                          onClick={() => {
                            setGroups((current) => current.filter((g) => g.id !== group.id));
                            setSelectedRejects((current) => {
                              const next = new Set(current);
                              group.duplicates.forEach((d) => next.delete(d.path));
                              return next;
                            });
                            invoke('library_suggestion', {
                              id: group.id,
                              state: 'dismissed',
                              data: { ...group, reason: 'different-moments-or-intentional-effect' },
                            }).catch(console.error);
                          }}
                        >
                          {t('catalog.keepGroupSplitIncorrectGroup')}
                        </button>
                      </div>
                      <Text variant={TextVariants.heading} className="mb-2">
                        {t('modals.culling.groupHeader', { index: page + 1 })}
                      </Text>
                      <Text variant={TextVariants.small} className="mb-2">
                        {t('modals.culling.provisionalNotice')}
                        {group.reasons.includes('detail') && ` ${t('modals.culling.detailReason')}`}
                        {group.reasons.includes('exposure') && ` ${t('modals.culling.exposureReason')}`}
                      </Text>
                      <div className="grid grid-cols-1 md:grid-cols-[minmax(200px,1fr)_minmax(0,3fr)] gap-5">
                        <div>
                          <Text variant={TextVariants.label} className="mb-1">
                            {t('modals.culling.suggestedKeeper')}
                          </Text>
                          <div className="relative rounded-md overflow-hidden border-2 border-green-500 aspect-[3/2] bg-black/30">
                            <img
                              src={thumbnails[group.representative.path]}
                              alt={group.representative.path.split(/[\\/]/).at(-1)}
                              className="w-full h-full object-contain"
                            />
                            <Text
                              as="div"
                              variant={TextVariants.small}
                              color={TextColors.white}
                              className="absolute bottom-0 left-0 right-0 p-1 bg-black/60"
                            >
                              {t('modals.culling.score', { score: group.representative.qualityScore.toFixed(2) })}
                            </Text>
                          </div>
                          <p className="truncate text-xs text-text-secondary mt-2" title={group.representative.path}>
                            {group.representative.path.split(/[\\/]/).at(-1)}
                          </p>
                        </div>
                        <div>
                          <Text variant={TextVariants.label} className="mb-1">
                            {t('modals.culling.similarAlternatives', { count: group.duplicates.length })}
                          </Text>
                          <div className="grid grid-cols-[repeat(auto-fill,minmax(170px,1fr))] gap-3 items-start">
                            {group.duplicates.map((dup) => (
                              <ImageThumbnail
                                key={dup.path}
                                path={dup.path}
                                thumbnails={thumbnails}
                                isSelected={selectedRejects.has(dup.path)}
                                onToggle={() => handleToggleReject(dup.path)}
                              >
                                {t('modals.culling.score', { score: dup.qualityScore.toFixed(2) })}
                                <button
                                  className="block w-full rounded bg-surface px-2 py-1.5"
                                  onClick={() => setComparison([group.representative.path, dup.path])}
                                >
                                  {t('catalog.compareFrames')}
                                </button>
                                <button
                                  title="Exclude this frame from the group"
                                  className="block w-full rounded bg-bg-primary px-1 py-1 text-text-primary"
                                  onClick={(event) => {
                                    event.stopPropagation();
                                    const updated = {
                                      ...group,
                                      duplicates: group.duplicates.filter((d) => d.path !== dup.path),
                                    };
                                    setGroups((current) => current.map((g) => (g.id === group.id ? updated : g)));
                                    invoke('library_suggestion', {
                                      id: group.id,
                                      state: 'pending',
                                      data: updated,
                                    }).catch(console.error);
                                    setSelectedRejects((current) => {
                                      const next = new Set(current);
                                      next.delete(dup.path);
                                      return next;
                                    });
                                  }}
                                >
                                  {t('catalog.keepSeparately')}
                                </button>
                                <button
                                  className="block w-full rounded bg-bg-primary px-1 py-1 text-text-primary"
                                  onClick={(event) => {
                                    event.stopPropagation();
                                    const updated = {
                                      ...group,
                                      representative: dup,
                                      duplicates: [
                                        group.representative,
                                        ...group.duplicates.filter((d) => d.path !== dup.path),
                                      ],
                                      reasons: [],
                                    };
                                    setGroups((current) => current.map((g) => (g.id === group.id ? updated : g)));
                                    invoke('library_suggestion', {
                                      id: group.id,
                                      state: 'pending',
                                      data: updated,
                                    }).catch(console.error);
                                    setSelectedRejects((current) => {
                                      const next = new Set(current);
                                      next.delete(dup.path);
                                      return next;
                                    });
                                  }}
                                >
                                  {t('modals.culling.chooseKeeper')}
                                </button>
                              </ImageThumbnail>
                            ))}
                          </div>
                        </div>
                      </div>
                    </div>
                  ))}
                </div>
              )}
              {activeTab === 'blurry' && (
                <div className="grid grid-cols-[repeat(auto-fill,minmax(170px,1fr))] gap-3 items-start">
                  {visibleBlurry.map((img) => (
                    <ImageThumbnail
                      key={img.path}
                      path={img.path}
                      thumbnails={thumbnails}
                      isSelected={selectedRejects.has(img.path)}
                      onToggle={() => handleToggleReject(img.path)}
                    >
                      {t('modals.culling.sharpness', { sharpness: img.sharpnessMetric.toFixed(0) })}
                      <button
                        className="block w-full rounded bg-bg-primary px-1 py-1 text-text-primary"
                        onClick={(event) => {
                          event.stopPropagation();
                          invoke('library_suggestion', {
                            id: `blur:${img.assetId || img.path}:${img.sourceRevision}`,
                            state: 'dismissed',
                            data: { ...img, reason: 'intentional-effect' },
                          })
                            .then(() => {
                              setDismissedBlurry((current) => new Set([...current, img.path]));
                              setSelectedRejects((current) => {
                                const next = new Set(current);
                                next.delete(img.path);
                                return next;
                              });
                            })
                            .catch(console.error);
                        }}
                      >
                        {t('catalog.keepIntentionalEffect')}
                      </button>
                    </ImageThumbnail>
                  ))}
                </div>
              )}
            </motion.div>
          </AnimatePresence>
        </div>

        <div className="flex flex-wrap justify-between items-center gap-3 pt-4 shrink-0">
          <div className="flex-1">
            <Text variant={TextVariants.small}>{t('modals.culling.reviewSelectionNotice')}</Text>
          </div>
          <div className="flex gap-3">
            <button
              className="px-4 py-2 rounded-md text-text-secondary hover:bg-surface transition-colors"
              onClick={onClose}
            >
              {t('modals.culling.cancel')}
            </button>
            <Button onClick={handleApply} disabled={selectedRejects.size === 0 || isApplying}>
              {isApplying && <Loader2 size={16} className="animate-spin" />}
              {t('modals.culling.rejectSelected', { count: selectedRejects.size })}
            </Button>
          </div>
        </div>
      </>
    );
  };

  const renderContent = () => {
    switch (stage) {
      case 'settings':
        return renderSettings();
      case 'progress':
        return renderProgress();
      case 'results':
        return renderResults();
      default:
        return null;
    }
  };

  if (!isMounted) return null;

  return (
    <div
      className={`fixed inset-0 flex items-center justify-center z-50 bg-black/30 backdrop-blur-xs transition-opacity duration-300 ease-in-out ${
        show ? 'opacity-100' : 'opacity-0'
      }`}
      onClick={onClose}
      role="dialog"
      aria-modal="true"
    >
      <div
        className={`relative bg-bg-secondary rounded-xl shadow-xl p-5 m-4 w-full ${stage === 'results' ? 'max-w-6xl h-[90vh] flex flex-col' : 'max-w-xl max-h-[90vh] overflow-y-auto'} transform transition-all duration-300 ease-out ${
          show ? 'scale-100 opacity-100 translate-y-0' : 'scale-95 opacity-0 -translate-y-4'
        }`}
        onClick={(e) => e.stopPropagation()}
      >
        <button
          aria-label={t('catalog.close')}
          className="absolute top-3 right-3 p-2 rounded-md hover:bg-surface text-text-secondary z-10"
          onClick={onClose}
        >
          <X size={20} />
        </button>
        {comparison && (
          <CullingCompare
            frames={comparison.map((path) => ({ path, url: thumbnails[path] }))}
            close={() => setComparison(null)}
          />
        )}
        {renderContent()}
      </div>
    </div>
  );
}
