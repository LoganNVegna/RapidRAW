import { useEffect, useState } from 'react';
import { Maximize, Minus, Plus, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';

type Frame = { path: string; url?: string };
export default function CullingCompare({ frames, close }: { frames: Frame[]; close: () => void }) {
  const { t } = useTranslation();
  const [zoom, setZoom] = useState(1);
  const [pan, setPan] = useState({ x: 0, y: 0 });
  useEffect(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.stopImmediatePropagation();
        close();
      }
    };
    window.addEventListener('keydown', escape, true);
    return () => window.removeEventListener('keydown', escape, true);
  }, [close]);
  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={t('catalog.compareFrames')}
      className="fixed inset-0 z-[120] bg-bg-primary text-text-primary flex flex-col p-4 gap-3"
      onClick={(event) => event.stopPropagation()}
    >
      <header className="flex items-center gap-4 shrink-0">
        <h2 className="font-semibold mr-auto">{t('catalog.compareFrames')}</h2>
        <span className="text-xs text-text-secondary">{t('catalog.synchronizedPreviewZoom')}</span>
        <button
          aria-label={t('catalog.zoomOut')}
          onClick={() => setZoom((value) => Math.max(1, value / 1.5))}
          className="p-2"
        >
          <Minus size={20} />
        </button>
        <span className="w-16 text-center text-sm">{zoom.toFixed(1)}×</span>
        <button
          aria-label={t('catalog.zoomIn')}
          onClick={() => setZoom((value) => Math.min(8, value * 1.5))}
          className="p-2"
        >
          <Plus size={20} />
        </button>
        <button
          aria-label={t('catalog.fitPhotos')}
          onClick={() => {
            setZoom(1);
            setPan({ x: 0, y: 0 });
          }}
          className="p-2"
        >
          <Maximize size={20} />
        </button>
        <button aria-label={t('catalog.close')} onClick={close} className="p-2">
          <X size={22} />
        </button>
      </header>
      <p className="text-xs text-text-secondary">{t('catalog.previewComparisonHint')}</p>
      <div className="grid grid-cols-2 gap-3 flex-1 min-h-0">
        {frames.map((frame) => (
          <figure key={frame.path} className="min-w-0 min-h-0 flex flex-col gap-2">
            <div
              className="flex-1 min-h-0 overflow-hidden bg-black/30 rounded-lg cursor-grab active:cursor-grabbing touch-none"
              onWheel={(event) => {
                event.preventDefault();
                setZoom((value) => Math.max(1, Math.min(8, value * (event.deltaY < 0 ? 1.15 : 1 / 1.15))));
              }}
              onPointerDown={(event) => {
                event.currentTarget.setPointerCapture(event.pointerId);
              }}
              onPointerMove={(event) => {
                if (!event.currentTarget.hasPointerCapture(event.pointerId) || zoom === 1) return;
                const bounds = event.currentTarget.getBoundingClientRect();
                setPan((value) => ({
                  x: Math.max(-100, Math.min(100, value.x + (event.movementX / bounds.width) * 100)),
                  y: Math.max(-100, Math.min(100, value.y + (event.movementY / bounds.height) * 100)),
                }));
              }}
            >
              <img
                src={frame.url}
                alt={frame.path.split(/[\\/]/).at(-1)}
                draggable={false}
                className="w-full h-full object-contain select-none pointer-events-none"
                style={{
                  transform: `translate(${zoom === 1 ? 0 : pan.x}%, ${zoom === 1 ? 0 : pan.y}%) scale(${zoom})`,
                }}
              />
            </div>
            <figcaption title={frame.path} className="truncate text-xs text-text-secondary">
              {frame.path.split(/[\\/]/).at(-1)}
            </figcaption>
          </figure>
        ))}
      </div>
    </div>
  );
}
