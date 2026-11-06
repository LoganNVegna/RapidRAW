# Library preview integration

This is an AI-assisted review prototype built on the metadata-store interface from upstream PR #2080. It is not an accepted upstream storage design. The existing sidecar provider remains the default. The isolated Library Preview profile can opt into a catalog-backed provider to keep its edits separate from an existing installation.

## Boundaries

- `metadata_store` remains the interface for editing metadata and virtual copies. Successful changes update a searchable catalog read model. Remote Immich records continue through the existing sidecar/remote behavior.
- The permanent SQLite database owns stable asset IDs, organization, collection membership/order, review sessions, suggestion decisions, undo and resumable job records. File paths are attributes of assets. Relinking verifies recorded size, modification time and a sampled content fingerprint; it never matches only by filename.
- The disposable cache holds rendered previews, versioned analysis and verified source copies. Source-copy reads preserve the logical original path for metadata, file operations and export naming. Cache entries do not make unavailable originals writable.
- Ordinary and smart collections query the indexed library in bounded batches behind continuous virtual scrolling. Metadata refresh retains the loaded range; scope changes discard stale responses. Prepared folders use the same query interface. The original grid, comparison view, Sources panel and editor remain in use.
- Catalog session scopes are saved separately from physical folder paths. A collection title is never sent to filesystem enumeration. Empty folders distinguish filters, unavailable storage, unsupported video-only contents, unindexed photos and nested folders.
- Source working copies are opt-in. Storage setup labels source folders and the cache as unspecified, SSD/flash, hard drive or network storage. Only tagged slower sources with a separate SSD cache are eligible; already-fast and same-volume sources are skipped. Preview caching works on every storage type. Generic budgets are 20 GiB for optional working copies, 10 GiB for previews/analysis and 5 GiB kept free; personal profiles may seed other budgets without changing upstream defaults.
- Background preparation enumerates folders once, batches queue writes, processes source files sequentially and yields to foreground work. Jobs persist pause/resume/cancellation state. An interrupted copy is never published as a usable source.
- Culling uses bounded capture-time/camera candidates, cached provisional analysis and explicit acceptance. Suggested groups have stable identities and saved corrections. Native detail checks describe nine image regions and retain uncertainty about subject focus. Exact duplicates require a separate full-file hash check.
- Culling results mount one group at a time, with direct group navigation and bounded preview requests. Alternatives have unobscured, aspect-preserving thumbnails and separate controls. Two-frame preview comparison synchronizes zoom/pan and clearly distinguishes preview magnification from native-resolution focus analysis. Selecting alternatives never applies flags until the explicit acceptance action.

The optional catalog metadata provider is a local review-build isolation mechanism. Its eventual upstream replacement or integration must be agreed with the authors of #2038/#2080. It must not silently replace a user's selected edit provider or discard remote/server edits. The catalog's indexed organization model is separate from that choice.

## Contribution sequence

The next organization increment adds reversible source exclusions (retaining asset/edit/membership records), and optional likely-copy grouping across indexed folders and collections. Indexed copy keys use capture metadata, file type, size, render revision and available sampled file fingerprints. They are candidates, not proof of byte equality. Virtual copies, RAW/JPEG variants, missing capture metadata and different rendered edits remain independent. The browser groups after scope/filter evaluation and before pagination; expanding a group restores each matching file. Decisions affect only the displayed file. Full-file duplicate verification remains a separate action.

Smart conditions retain backward compatibility with flat rule arrays and add nested All/Any groups, RAW/file-extension comparisons, numeric exposure fields and arbitrary EXIF-key presence/absence or text comparisons. Values and metadata keys use SQL parameters; malformed/empty nested groups are rejected rather than silently widening results. The copy-key schema migration creates a recovery snapshot and derives keys from catalog metadata without scanning source bytes.

Existing independent draft contributions:

1. [#2119: independent stars and flags](https://github.com/CyberTimon/RapidRAW/pull/2119).
2. [#2120: configurable regenerable-cache placement](https://github.com/CyberTimon/RapidRAW/pull/2120).
3. [#2121: explicit, bounded culling suggestions](https://github.com/CyberTimon/RapidRAW/pull/2121).
4. [#2122: contextual review shortcuts and auto-advance](https://github.com/CyberTimon/RapidRAW/pull/2122).

After agreement on the storage boundary, split the integration prototype into catalog identity/querying and lifecycle hooks; collections/order; smart rules and built-in views; persistent review sessions and bulk undo; preparation jobs and accounting; verified source caching; then cached culling analysis and review corrections. Link the smart-album work to [#2097](https://github.com/CyberTimon/RapidRAW/issues/2097). These are intended for official releases, subject to maintainer review.

Do not add a new committed automated-test framework without maintainer agreement. Local fixture verification, measurements and native UI checks must be reported honestly; an automated UI check is not a human recording. Personal photos, catalogs and machine-specific profiles must not be included in contributions.

## Local build

Copy `src-tauri/library-preview.example.json` to the ignored `src-tauri/library-preview.local.json`, then configure explicitly chosen photo roots and separate permanent/cache directories. Build with:

```sh
npm install
npm run tauri -- build --debug --no-bundle --config src-tauri/tauri.library-preview.conf.json
```

Keep the executable, `resources`, `lensfun_db`, and the local profile together. The preview has its own application identifier and no file associations. Existing preview settings take precedence over the first-run profile. No Lightroom import is included.

Back up the permanent Library directory with the user's normal backup system. Recovery snapshots are written inside `Library/Backups`, and schema migration snapshots inside `Library/Migrations`. For recovery, close the preview, retain a copy of the current database and its WAL files, then restore a verified snapshot as `library.sqlite`. Cached source copies and previews are disposable and are not backups.
