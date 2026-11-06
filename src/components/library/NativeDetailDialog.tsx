import { useTranslation } from 'react-i18next';
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { X, Loader2 } from 'lucide-react';
import { useCatalogStore } from '../../store/useCatalogStore';
import Button from '../ui/Button';

type Detail = { tiles: number[]; explanation: string; source: string };
export default function NativeDetailDialog() {
  const { t } = useTranslation();
  const paths = useCatalogStore((s) => s.detailPaths);
  const [results, setResults] = useState<Record<string, Detail | string>>({});
  const close = () => useCatalogStore.getState().set({ detailPaths: undefined });
  useEffect(() => {
    if (!paths?.length) return;
    let cancelled = false;
    setResults({});
    void (async () => {
      for (const path of paths) {
        if (cancelled) break;
        let result: Detail | string;
        try {
          result = await invoke<Detail>('library_focus_check', { path });
        } catch (error) {
          result = String(error);
        }
        if (!cancelled) setResults((prior) => ({ ...prior, [path]: result }));
      }
    })();
    const escape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.stopImmediatePropagation();
        close();
      }
    };
    window.addEventListener('keydown', escape, true);
    return () => {
      cancelled = true;
      window.removeEventListener('keydown', escape, true);
    };
  }, [paths]);
  if (!paths?.length) return null;
  return (
    <div className="fixed inset-0 z-[110] bg-black/60 backdrop-blur-sm flex items-center justify-center p-6">
      <section
        role="dialog"
        aria-modal="true"
        aria-label={t('catalog.nativeResolutionDetail')}
        className="w-full max-w-2xl max-h-[85vh] overflow-y-auto rounded-xl bg-bg-secondary text-text-primary shadow-2xl p-6 space-y-5"
      >
        <header className="flex items-center justify-between">
          <h2 className="font-semibold">{t('catalog.nativeResolutionDetail')}</h2>
          <button aria-label={t('catalog.close')} onClick={close}>
            <X size={18} />
          </button>
        </header>
        <p className="text-sm text-text-secondary">
          {t('catalog.detailIsMeasuredAcrossNineImageRegionsAtNativeResolution')}
        </p>
        <div className="grid grid-cols-2 gap-4">
          {paths.map((path) => {
            const result = results[path];
            return (
              <div key={path} className="bg-surface/60 rounded-lg p-4 space-y-3">
                <p className="text-xs truncate" title={path}>
                  {path.split(/[\\/]/).pop()}
                </p>
                {!result ? (
                  <Loader2 size={18} className="animate-spin" />
                ) : typeof result === 'string' ? (
                  <p className="text-xs text-amber-400">{result}</p>
                ) : (
                  <div className="grid grid-cols-3 gap-1">
                    {result.tiles.map((value, index) => (
                      <span
                        key={index}
                        className="bg-surface text-xs text-center rounded py-2"
                        title={`Region ${index + 1}`}
                      >
                        {value.toFixed(0)}
                      </span>
                    ))}
                  </div>
                )}
              </div>
            );
          })}
        </div>
        <footer className="flex justify-end">
          <Button onClick={close}>{t('catalog.done')}</Button>
        </footer>
      </section>
    </div>
  );
}
