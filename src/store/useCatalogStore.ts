import { create } from 'zustand';
import { FilterCriteria, SortCriteria, ImageFile } from '../components/ui/AppProperties';
export type Rule = { field: string; operator: string; value: string };
export type Collection = {
  id: string;
  name: string;
  parent: string | null;
  kind: string;
  rules: Rule[];
  mode: string;
  count?: number;
};
export type Asset = ImageFile & {
  assetId: string;
  available: boolean;
  review: string;
  smallPreview?: string;
  mediumPreview?: string;
  memberships: string[];
};
export type Query = {
  view: string;
  collectionId?: string;
  folder?: string;
  recursive?: boolean;
  search: string;
  offset: number;
  limit: number;
  sort: string;
  rules: Rule[];
  mode: string;
  descending?: boolean;
  colors?: string[];
  tags?: string[];
  tagMode?: string;
};
export type Job = {
  id: string;
  kind: string;
  status: string;
  completed: number;
  total: number;
  failed: number;
  current: string;
  error: string;
};
export type StorageLayout = { enabled: boolean; cacheKind: string; sources: Array<{ path: string; kind: string }> };
export type Catalog = {
  storage?: StorageLayout;
  collections: Collection[];
  counts: Record<string, number>;
  jobs: Job[];
  target: string;
  directory: string;
  roots: string[];
  cache: {
    used: number;
    previewUsed?: number;
    pinned: number;
    free: number;
    directory: string;
    rawLimit: number;
    previewLimit: number;
    reserve: number;
    limits: { rawGib: number; previewGib: number; reserveGib: number };
  };
};
export type Session = {
  kind?: 'culling';
  paths?: string[];
  settings?: import('../components/ui/AppProperties').CullingSettings;
  id?: string;
  name: string;
  query: Query;
  path?: string;
  assetId?: string;
  completed: number;
  resolvedQuery?: Query;
  criteria?: {
    filterCriteria: FilterCriteria;
    sortCriteria: SortCriteria;
    searchCriteria: { tags: string[]; text: string; mode: 'AND' | 'OR' };
  };
};
export const initialQuery: Query = {
  view: 'all',
  search: '',
  offset: 0,
  limit: 250,
  sort: 'capture',
  rules: [],
  mode: 'all',
};
type State = {
  active: boolean;
  query: Query;
  catalog?: Catalog;
  total: number;
  loaded: number;
  loading: boolean;
  loadError?: string;
  loadMore?: (retry?: boolean) => void;
  previewPaths?: string[];
  pendingAssetIndex?: number;
  utilities: boolean;
  edit: Partial<Collection> | null;
  detailPaths?: string[];
  scopeRevision: number;
  sessions: Session[];
  resume?: Session;
  refresh: number;
  pendingSelection?: 'first' | 'last';
  pendingAssetId?: string;
  set: (value: Partial<State>) => void;
  navigate: (view: string, collectionId?: string) => void;
};
export const useCatalogStore = create<State>((set) => ({
  active: true,
  scopeRevision: 0,
  query: initialQuery,
  total: 0,
  loaded: 0,
  loading: false,
  utilities: false,
  edit: null,
  sessions: [],
  refresh: 0,
  set: (value) => set(value),
  navigate: (view, collectionId) =>
    set((state) => ({
      active: true,
      query: { ...initialQuery, view, collectionId },
      resume: undefined,
      scopeRevision: state.scopeRevision + 1,
      pendingSelection: undefined,
      pendingAssetIndex: undefined,
      pendingAssetId: undefined,
    })),
}));
