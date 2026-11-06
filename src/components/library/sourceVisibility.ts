import { invoke } from '@tauri-apps/api/core';
import { useCatalogStore } from '../../store/useCatalogStore';
import { useLibraryStore } from '../../store/useLibraryStore';
import { useSettingsStore } from '../../store/useSettingsStore';
import { Invokes } from '../ui/AppProperties';
const normalize = (path: string) => {
  const value = path.replaceAll('\\', '/').replace(/\/$/, '');
  return /^[a-z]:/i.test(value) || value.startsWith('//') ? value.toLowerCase() : value;
};
export const isWithinSource = (path: string | null | undefined, source: string) =>
  !!path && (normalize(path) === normalize(source) || normalize(path).startsWith(normalize(source) + '/'));

export async function setSourceVisible(path: string, visible: boolean) {
  await invoke('library_source_visibility', { path, excluded: !visible });
  const library = useLibraryStore.getState();
  const settings = useSettingsStore.getState().appSettings;
  const roots = [...new Set([...(settings?.rootFolders || []), ...library.rootPaths])].filter(
    (root) => !isWithinSource(root, path),
  );
  if (visible) roots.push(path);
  library.setLibrary({
    rootPaths: roots,
    folderTrees: library.folderTrees.filter((tree) => !isWithinSource(tree.path, path)),
  });
  if (settings)
    await useSettingsStore
      .getState()
      .handleSettingsChange({ ...settings, rootFolders: roots, lastRootPath: roots[0] || null });
  if (visible) {
    const tree = await invoke(Invokes.GetFolderTree, { path, expandedFolders: [path], showImageCounts: false }).catch(
      () => null,
    );
    if (tree) useLibraryStore.getState().setLibrary((s) => ({ folderTrees: [...s.folderTrees, tree] }));
  } else if (
    isWithinSource(library.currentFolderPath, path) ||
    isWithinSource(useCatalogStore.getState().query.folder, path)
  ) {
    useCatalogStore.getState().navigate('all');
    library.setLibrary({
      currentFolderPath: 'Library',
      imageList: [],
      multiSelectedPaths: [],
      libraryActivePath: null,
      activeAlbumId: null,
    });
  }
  useCatalogStore.getState().set({ refresh: Date.now() });
}
