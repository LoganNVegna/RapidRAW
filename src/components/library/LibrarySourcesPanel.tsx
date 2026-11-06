import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'react-toastify';
import { useCatalogStore } from '../../store/useCatalogStore';
import { isWithinSource, setSourceVisible } from './sourceVisibility';

const EMPTY: Array<{ path: string; count: number }> = [];
export default function SourceVisibility({ roots }: { roots: string[] }) {
  const { t } = useTranslation();
  const excluded = useCatalogStore((s) => s.catalog?.excludedSources || EMPTY);
  const [busy, setBusy] = useState(false);
  const change = (path: string, visible: boolean) => {
    setBusy(true);
    void setSourceVisible(path, visible)
      .catch((error) => toast.error(String(error)))
      .finally(() => setBusy(false));
  };
  return (
    <details className="border-t border-surface pt-4">
      <summary className="cursor-pointer text-sm">{t('catalog.librarySources')}</summary>
      <p className="text-sm text-text-secondary my-3">{t('catalog.sourceVisibilityHint')}</p>
      <div className="space-y-3 text-sm">
        {roots
          .filter((path) => !excluded.some((source) => isWithinSource(path, source.path)))
          .map((path) => (
            <div key={path} className="flex items-center gap-3">
              <span className="truncate flex-1" title={path}>
                {path}
              </span>
              <button
                disabled={busy}
                className="text-text-secondary hover:text-text-primary disabled:opacity-40"
                onClick={() => change(path, false)}
              >
                {t('catalog.excludeSource')}
              </button>
            </div>
          ))}
        {excluded.map((source) => (
          <div key={source.path} className="flex items-center gap-3">
            <span className="truncate flex-1 text-text-secondary" title={source.path}>
              {source.path} · {source.count.toLocaleString()} {t('catalog.photos')}
            </span>
            <button
              disabled={busy}
              className="hover:text-text-primary disabled:opacity-40"
              onClick={() => change(source.path, true)}
            >
              {t('catalog.restoreSource')}
            </button>
          </div>
        ))}
      </div>
    </details>
  );
}
