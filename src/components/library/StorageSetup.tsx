import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { useTranslation } from 'react-i18next';
import { toast } from 'react-toastify';
import { StorageLayout, useCatalogStore } from '../../store/useCatalogStore';
import { useSettingsStore } from '../../store/useSettingsStore';
import Button from '../ui/Button';

export default function StorageSetup({ roots }: { roots: string[] }) {
  const { t } = useTranslation();
  const stored = useCatalogStore((s) => s.catalog?.storage);
  const cachePath = useCatalogStore((s) => s.catalog?.cache.directory);
  const running = useCatalogStore((s) => s.catalog?.jobs.some((j) => ['running', 'queued'].includes(j.status)));
  const [layout, setLayout] = useState<StorageLayout>({ enabled: false, cacheKind: 'unknown', sources: [] });
  const [busy, setBusy] = useState(false);
  const [restart, setRestart] = useState(false);
  const signature = JSON.stringify(stored);
  useEffect(() => {
    if (stored) setLayout(stored);
  }, [signature]);
  const kinds = [
    ['unknown', 'storageUnknown'],
    ['ssd', 'storageSsd'],
    ['hdd', 'storageHdd'],
    ['network', 'storageNetwork'],
  ] as const;
  const picker = (value: string, onChange: (value: string) => void, label: string) => (
    <select
      aria-label={label}
      value={value || 'unknown'}
      onChange={(e) => onChange(e.target.value)}
      className="bg-surface rounded-md p-2 text-text-primary shrink-0"
    >
      {kinds.map(([key, label]) => (
        <option key={key} value={key}>
          {t(`catalog.${label}`)}
        </option>
      ))}
    </select>
  );
  return (
    <details className="border-t border-surface pt-4" open={!stored?.sources.length || undefined}>
      <summary className="cursor-pointer text-sm">{t('catalog.storageSetup')}</summary>
      <div className="space-y-4 pt-4 text-sm">
        <p className="text-text-secondary">{t('catalog.storageSetupHint')}</p>
        <div className="text-text-secondary break-all">
          {t('catalog.cacheLocation')}: {cachePath}
        </div>
        <Button
          disabled={busy || running || restart}
          onClick={() =>
            void (async () => {
              const folder = await open({ directory: true, multiple: false });
              const settings = useSettingsStore.getState().appSettings;
              if (typeof folder !== 'string' || !settings) return;
              await invoke('library_storage_layout', { value: { ...layout, enabled: false, cacheKind: 'unknown' } });
              await useSettingsStore.getState().handleSettingsChange({ ...settings, cacheDirectory: folder });
              setRestart(true);
            })().catch((e) => toast.error(String(e)))
          }
        >
          {t('catalog.changeCacheLocation')}
        </Button>
        {restart && (
          <p role="status" className="text-amber-400">
            {t('catalog.restartCacheLocation')}
          </p>
        )}
        <fieldset disabled={restart || busy} className="space-y-3 disabled:opacity-50">
          <label className="flex flex-wrap justify-between items-center gap-3">
            {t('catalog.storageCacheType')}
            {picker(
              layout.cacheKind,
              (cacheKind) => setLayout({ ...layout, cacheKind }),
              t('catalog.storageCacheType'),
            )}
          </label>
          {roots.map((path) => (
            <label key={path} className="flex items-center justify-between gap-3">
              <span title={path} className="truncate min-w-0">
                {path}
              </span>
              {picker(
                layout.sources.find((s) => s.path === path)?.kind || 'unknown',
                (kind) =>
                  setLayout({ ...layout, sources: [...layout.sources.filter((s) => s.path !== path), { path, kind }] }),
                path,
              )}
            </label>
          ))}
          <label className="flex items-center gap-3">
            <input
              type="checkbox"
              checked={layout.enabled}
              onChange={(e) => setLayout({ ...layout, enabled: e.target.checked })}
            />
            {t('catalog.enableWorkingCopies')}
          </label>
          <Button
            onClick={() => {
              setBusy(true);
              void invoke('library_storage_layout', { value: layout })
                .catch((e) => toast.error(String(e)))
                .finally(() => setBusy(false));
            }}
          >
            {t('catalog.saveStorageSetup')}
          </Button>
        </fieldset>
      </div>
    </details>
  );
}
