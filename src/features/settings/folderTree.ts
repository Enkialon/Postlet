import type { MailFolderSyncState } from '../../domain/entities';
import type { FolderId } from '../../domain/types';

export interface FolderTreeNode extends MailFolderSyncState {
  children: FolderTreeNode[];
  depth: number;
}

/** 将扁平文件夹列表建成多级树（父不在集合内时视为根）。 */
export function buildFolderTree(
  folders: MailFolderSyncState[],
): FolderTreeNode[] {
  const map = new Map<FolderId, FolderTreeNode>();
  for (const f of folders) {
    map.set(f.folder_id, { ...f, children: [], depth: 0 });
  }
  const roots: FolderTreeNode[] = [];
  for (const f of folders) {
    const node = map.get(f.folder_id)!;
    const parentId = f.parent_folder_id;
    if (parentId && map.has(parentId)) {
      map.get(parentId)!.children.push(node);
    } else {
      roots.push(node);
    }
  }

  function assignDepth(nodes: FolderTreeNode[], depth: number): void {
    for (const n of nodes) {
      n.depth = depth;
      n.children.sort((a, b) =>
        (a.display_name || a.folder_id).localeCompare(
          b.display_name || b.folder_id,
          'zh-CN',
          { sensitivity: 'base' },
        ),
      );
      assignDepth(n.children, depth + 1);
    }
  }

  roots.sort((a, b) =>
    (a.display_name || a.folder_id).localeCompare(
      b.display_name || b.folder_id,
      'zh-CN',
      { sensitivity: 'base' },
    ),
  );
  assignDepth(roots, 0);
  return roots;
}

/** 深度优先展平，便于模板渲染缩进行。 */
export function flattenFolderTree(
  roots: FolderTreeNode[],
): FolderTreeNode[] {
  const out: FolderTreeNode[] = [];
  function walk(nodes: FolderTreeNode[]): void {
    for (const n of nodes) {
      out.push(n);
      walk(n.children);
    }
  }
  walk(roots);
  return out;
}
