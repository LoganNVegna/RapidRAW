import { invoke } from '@tauri-apps/api/core';
import { Check, Folder, HardDrive, ListStart, Pin, PinOff, Undo2, X } from 'lucide-react';
import { toast } from 'react-toastify';
import { Option } from '../ui/AppProperties';
import { useCatalogStore } from '../../store/useCatalogStore';
const run = (action: () => Promise<unknown>) => () => void action().catch((error) => toast.error(String(error)));
export function catalogContextOptions(paths: string[]): Option[] {
  const state = useCatalogStore.getState();
  if (!state.active) return [];
  const collection = state.catalog?.collections.find((c) => c.id === state.query.collectionId);
  return [
    {
      label: 'Check native-resolution detail…',
      icon: Check,
      onClick: () => useCatalogStore.getState().set({ detailPaths: paths.slice(0, 8) }),
    },
    ...(paths.length > 1
      ? [
          {
            label: 'Keep both moments',
            icon: Check,
            onClick: run(() => invoke('library_edit', { paths, field: 'keep', value: true })),
          },
        ]
      : []),
    {
      label: 'Review status',
      icon: Check,
      submenu: ['unreviewed', 'reviewed', 'deferred'].map((value) => ({
        label: value[0].toUpperCase() + value.slice(1),
        onClick: run(() => invoke('library_edit', { paths, field: 'review', value })),
      })),
    },
    {
      label: 'Collections',
      icon: Folder,
      submenu: [
        {
          label: 'Add to target collection (B)',
          onClick: run(() =>
            invoke('library_collection', {
              action: 'toggle',
              collection: { id: state.catalog?.target || 'quick' },
              paths,
            }),
          ),
        },
        ...(state.catalog?.collections
          .filter((c) => c.kind === 'collection')
          .map((c) => ({
            label: `Add to ${c.name}`,
            onClick: run(() => invoke('library_collection', { action: 'add', collection: { id: c.id }, paths })),
          })) || []),
        ...(collection?.kind === 'collection'
          ? [
              {
                label: 'Remove from this collection',
                icon: X,
                onClick: run(() =>
                  invoke('library_collection', { action: 'remove', collection: { id: collection.id }, paths }),
                ),
              },
              {
                label: 'Move selection to beginning',
                icon: ListStart,
                onClick: run(async () => {
                  await invoke('library_collection', { action: 'reorder', collection: { id: collection.id }, paths });
                  useCatalogStore.getState().set({ query: { ...state.query, sort: 'manual', offset: 0 } });
                }),
              },
            ]
          : []),
      ],
    },
    {
      label: 'SSD cache',
      icon: HardDrive,
      submenu: [
        {
          label: 'Cache on SSD',
          icon: HardDrive,
          onClick: run(() => invoke('library_cache', { paths, action: 'cache' })),
        },
        { label: 'Pin on SSD', icon: Pin, onClick: run(() => invoke('library_cache', { paths, action: 'pin' })) },
        { label: 'Unpin', icon: PinOff, onClick: run(() => invoke('library_cache', { paths, action: 'unpin' })) },
      ],
    },
    { label: 'Undo library decision', icon: Undo2, onClick: run(() => invoke('library_undo')) },
  ];
}
