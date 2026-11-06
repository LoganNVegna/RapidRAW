import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Database, Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { toast } from 'react-toastify';
import { useCatalogStore } from '../../store/useCatalogStore';
import { useLibraryStore } from '../../store/useLibraryStore';
import { useSettingsStore } from '../../store/useSettingsStore';
import { LibraryViewMode, RawStatus } from '../ui/AppProperties';
import Button from '../ui/Button';
import Text from '../ui/Text';
import { TextColors, TextVariants } from '../../types/typography';

type FolderStatus = { available: boolean; photos: number; videos: number; folders: number; indexed: number };

export default function CatalogEmptyState() {
  const { t } = useTranslation();
  const { query, catalog, loading, loadError } = useCatalogStore();
  const { filterCriteria: filter, searchCriteria: search } = useLibraryStore();
  const [status, setStatus] = useState<FolderStatus>();
  const [statusError, setStatusError] = useState(false);
  const filtered = !!(
    search.text ||
    search.tags.length ||
    filter.rating ||
    filter.colors.length ||
    (filter.flagStatus && filter.flagStatus !== 'all') ||
    (filter.editedStatus && filter.editedStatus !== 'all') ||
    (filter.rawStatus && filter.rawStatus !== 'all') ||
    query.rules.length
  );
  useEffect(() => {
    let current = true;
    setStatus(undefined);
    setStatusError(false);
    if (query.folder && !filtered && !loading)
      void invoke<FolderStatus>('library_folder_status', { path: query.folder })
        .then((status) => {
          if (current) setStatus(status);
        })
        .catch(() => {
          if (current) setStatusError(true);
        });
    return () => {
      current = false;
    };
  }, [query.folder, filtered, loading]);
  const waiting = loading || (!!query.folder && !filtered && !status && !statusError && !loadError);
  const welcome = !query.folder && !query.collectionId && query.view === 'all' && !catalog?.counts.all;
  const title = loadError
    ? 'loadingFailed'
    : waiting
      ? 'loadingPhotos'
      : filtered
        ? 'noMatchingPhotos'
        : statusError
          ? 'folderCheckFailed'
          : status?.available === false
            ? 'folderUnavailable'
            : query.folder
              ? status?.photos
                ? 'folderNotIndexed'
                : 'noSupportedPhotos'
              : welcome
                ? 'yourPhotoLibrary'
                : 'emptyView';
  const description = filtered
    ? 'clearFiltersHint'
    : statusError
      ? 'folderCheckFailedHint'
      : status?.available === false
        ? 'folderUnavailableHint'
        : query.folder
          ? status?.photos
            ? 'folderNotIndexedHint'
            : status?.videos
              ? 'videosOnlyHint'
              : status?.indexed || status?.folders
                ? 'subfoldersHint'
                : 'emptyFolderHint'
          : welcome
            ? 'prepareLibraryHint'
            : query.collectionId
              ? 'emptyCollectionHint'
              : 'emptyViewHint';
  return (
    <div className="flex-1 flex flex-col items-center justify-center text-center gap-3 p-8">
      {waiting ? (
        <Loader2 size={32} className="animate-spin text-text-secondary" />
      ) : (
        <Database size={32} className="text-text-secondary" />
      )}
      <Text variant={TextVariants.heading}>{t(`catalog.${title}`)}</Text>
      {!waiting && (
        <Text color={TextColors.secondary} className="max-w-md">
          {t(`catalog.${description}`)}
        </Text>
      )}
      {loadError && <Button onClick={() => useCatalogStore.getState().loadMore?.(true)}>{t('catalog.retry')}</Button>}
      {filtered && (
        <Button
          onClick={() => {
            useLibraryStore.getState().setFilterCriteria({
              rating: 0,
              colors: [],
              flagStatus: 'all',
              editedStatus: 'all',
              rawStatus: RawStatus.All,
            });
            useLibraryStore.getState().setSearchCriteria({ text: '', tags: [] });
            useCatalogStore.getState().set({ query: { ...query, rules: [], offset: 0 } });
          }}
        >
          {t('catalog.clearFilters')}
        </Button>
      )}
      {!waiting && !filtered && status?.available && !query.recursive && (status.indexed > 0 || status.folders > 0) && (
        <Button
          onClick={() => {
            const settings = useSettingsStore.getState().appSettings;
            if (settings)
              void useSettingsStore
                .getState()
                .handleSettingsChange({ ...settings, libraryViewMode: LibraryViewMode.Recursive });
          }}
        >
          {t('catalog.includeSubfolders')}
        </Button>
      )}
      {!waiting && !filtered && (welcome || (!!status?.photos && status.available)) && (
        <Button
          onClick={() => {
            if (query.folder)
              void invoke('library_start_job', { kind: 'prepare', roots: [query.folder] }).catch((error) =>
                toast.error(String(error)),
              );
            useCatalogStore.getState().set({ utilities: true });
          }}
        >
          {t('catalog.prepareLibrary')}
        </Button>
      )}
    </div>
  );
}
