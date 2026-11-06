import { Catalog, initialQuery, Query } from '../../store/useCatalogStore';

export const isFolderPath = (path: string) => /^(?:[A-Za-z]:[\\/]|[\\/])/.test(path);

export function restoreCatalogQuery(
  state: { currentFolderPath?: string; catalogQuery?: Query } | undefined,
  catalog?: Catalog,
): Query | undefined {
  if (state?.catalogQuery && typeof state.catalogQuery.view === 'string')
    return { ...initialQuery, ...state.catalogQuery, offset: 0, limit: 250 };
  const label = state?.currentFolderPath;
  if (!label || label.startsWith('Album: ') || isFolderPath(label)) return;
  // Older preview builds saved the display title in the physical folder field.
  const collection = catalog?.collections.find((collection) => collection.name === label);
  const views: Record<string, string> = {
    Picks: 'picks',
    Rejected: 'rejected',
    Unreviewed: 'unreviewed',
    'Recently Added': 'recent',
    Unavailable: 'missing',
  };
  return { ...initialQuery, collectionId: collection?.id, view: views[label] || 'all' };
}
