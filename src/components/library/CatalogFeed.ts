import type { Asset, Query } from '../../store/useCatalogStore';

type Page = { assets: Asset[]; total: number };
export type FeedState = Page & { loading: boolean; error?: string };

// Keep bounded database requests behind one continuous, virtualized photo list.
export class CatalogFeed {
  state: FeedState = { assets: [], total: 0, loading: false };
  private query?: Query;
  private key = '';
  private generation = 0;
  private offset = 0;

  constructor(
    private fetchPage: (query: Query) => Promise<Page>,
    private changed: (state: FeedState) => void,
  ) {}

  setQuery(query: Query) {
    const key = JSON.stringify({ ...query, offset: 0 });
    this.query = { ...query };
    if (key === this.key) return;
    this.key = key;
    this.generation++;
    this.offset = 0;
    this.publish({ assets: [], total: 0, loading: true });
  }

  cancel() {
    this.generation++;
  }

  refresh(minimum = 0) {
    return this.read(false, minimum);
  }

  more(retry = false) {
    if (this.state.loading || (this.state.error && !retry) || this.offset >= this.state.total) return;
    return this.read(true);
  }

  private publish(state: FeedState) {
    this.state = state;
    this.changed(state);
  }

  private async read(append: boolean, minimum = 0) {
    if (!this.query) return;
    const query = this.query;
    const generation = ++this.generation;
    const target = append
      ? this.offset + query.limit
      : Math.max(query.limit, query.offset + query.limit, this.state.assets.length, minimum);
    let offset = append ? this.offset : 0;
    let assets = append ? [...this.state.assets] : [];
    let total: number;
    this.publish({ ...this.state, loading: true, error: undefined });
    try {
      do {
        const page = await this.fetchPage({ ...query, offset, limit: Math.min(1000, target - offset) });
        if (generation !== this.generation) return;
        total = page.total;
        assets.push(...page.assets);
        offset += page.assets.length;
        if (!page.assets.length) break;
      } while (offset < Math.min(target, total));
      this.offset = offset;
      assets = [...new Map(assets.map((asset) => [asset.assetId, asset])).values()];
      this.publish({ assets, total, loading: false });
    } catch (error) {
      if (generation === this.generation) this.publish({ ...this.state, loading: false, error: String(error) });
    }
  }
}
