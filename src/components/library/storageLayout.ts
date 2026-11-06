import type { StorageLayout } from '../../store/useCatalogStore';

const normalize = (path: string) => {
  const value = path.replaceAll('\\', '/').replace(/\/$/, '');
  return /^[A-Za-z]:/.test(value) || value.startsWith('//') ? value.toLowerCase() : value;
};
export function canCacheSource(layout: StorageLayout | undefined, path: string, cachePath?: string) {
  if (!layout?.enabled || layout.cacheKind !== 'ssd') return false;
  const key = normalize(path);
  const source = layout.sources
    .filter((s) => key === normalize(s.path) || key.startsWith(normalize(s.path) + '/'))
    .sort((a, b) => b.path.length - a.path.length)[0];
  if (!source || !['hdd', 'network'].includes(source.kind)) return false;
  if (cachePath && /^[a-z]:/.test(key) && key.slice(0, 2) === normalize(cachePath).slice(0, 2)) return false;
  return true;
}
