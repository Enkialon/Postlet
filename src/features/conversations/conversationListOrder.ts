import type { SessionListItem } from '../../domain/dto';

export const LIST_TOP_ENTER_PX = 60;
export const LIST_TOP_LEAVE_PX = 100;

export function sessionListKey(item: SessionListItem): string {
  return `${item.session.account_id}:${item.session.id}`;
}

/** @deprecated 兼容旧名 */
export const conversationListKey = sessionListKey;

export function isScrollAtTop(
  scrollTop: number,
  currentlyAtTop: boolean,
): boolean {
  if (currentlyAtTop) {
    return scrollTop <= LIST_TOP_LEAVE_PX;
  }
  return scrollTop <= LIST_TOP_ENTER_PX;
}

/** 保守：仅当最新消息 ID 变化且非 outbound 时视为「新来信活动」 */
export function isLikelyNewIncomingActivity(
  previous: SessionListItem | undefined,
  next: SessionListItem,
): boolean {
  const preview = next.last_message_preview;
  if (!preview || preview.is_outbound) return false;
  const nextId = next.session.last_message_id;
  if (!nextId) return false;
  if (!previous) return true;
  return previous.session.last_message_id !== nextId;
}

export function mergeSessionListItem(
  existing: SessionListItem | undefined,
  next: SessionListItem,
): SessionListItem {
  if (!existing) {
    return {
      ...next,
      session: { ...next.session },
      display_name: next.display_name,
      last_message_preview: next.last_message_preview
        ? { ...next.last_message_preview }
        : null,
      contact_display_name: next.contact_display_name,
      group_name: next.group_name,
    };
  }
  Object.assign(existing.session, next.session);
  existing.display_name = next.display_name;
  existing.contact_display_name = next.contact_display_name;
  existing.group_name = next.group_name;
  if (next.last_message_preview) {
    if (existing.last_message_preview) {
      Object.assign(existing.last_message_preview, next.last_message_preview);
    } else {
      existing.last_message_preview = { ...next.last_message_preview };
    }
  } else {
    existing.last_message_preview = null;
  }
  return existing;
}

/** @deprecated 兼容旧名 */
export const mergeConversationListItem = mergeSessionListItem;

export interface ApplySessionSnapshotInput {
  snapshot: SessionListItem[];
  previousByKey: Map<string, SessionListItem>;
  previousCanonicalOrder: string[];
  previousDisplayOrder: string[];
  previousPendingActivityKeys: string[];
  atTop: boolean;
  /** 筛选 / 账户切换等：强制应用最新排序 */
  forceApplyOrder: boolean;
}

export interface ApplySessionSnapshotResult {
  byKey: Map<string, SessionListItem>;
  canonicalOrder: string[];
  displayOrder: string[];
  pendingActivityKeys: string[];
  orderApplied: boolean;
}

/**
 * 将后端会话页合并进前端状态：始终更新数据；按是否在顶部决定是否应用排序。
 */
export function applySessionSnapshot(
  input: ApplySessionSnapshotInput,
): ApplySessionSnapshotResult {
  const {
    snapshot,
    previousByKey,
    previousCanonicalOrder,
    previousDisplayOrder,
    previousPendingActivityKeys,
    atTop,
    forceApplyOrder,
  } = input;

  const canonicalOrder = snapshot.map(sessionListKey);
  const snapshotKeySet = new Set(canonicalOrder);
  const prevCanonicalSet = new Set(previousCanonicalOrder);

  const byKey = new Map<string, SessionListItem>();
  const activity: string[] = [];

  for (const item of snapshot) {
    const key = sessionListKey(item);
    const prev = previousByKey.get(key);
    if (
      !forceApplyOrder &&
      !atTop &&
      isLikelyNewIncomingActivity(prev, item)
    ) {
      activity.push(key);
    }
    byKey.set(key, mergeSessionListItem(prev, item));
  }

  if (!forceApplyOrder && !atTop) {
    for (const key of previousDisplayOrder) {
      if (byKey.has(key) || snapshotKeySet.has(key)) continue;
      if (prevCanonicalSet.has(key)) continue;
      const kept = previousByKey.get(key);
      if (kept) byKey.set(key, kept);
    }
  }

  const orderApplied = forceApplyOrder || atTop;
  let displayOrder: string[];

  if (orderApplied) {
    displayOrder = [...canonicalOrder];
  } else {
    displayOrder = previousDisplayOrder.filter((key) => {
      if (snapshotKeySet.has(key)) return true;
      if (!prevCanonicalSet.has(key) && byKey.has(key)) return true;
      return false;
    });
  }

  const pending = new Set(
    orderApplied ? [] : previousPendingActivityKeys.filter((k) => byKey.has(k)),
  );
  if (!orderApplied) {
    for (const key of activity) pending.add(key);
  }

  return {
    byKey,
    canonicalOrder,
    displayOrder,
    pendingActivityKeys: [...pending],
    orderApplied,
  };
}

/** @deprecated 兼容旧名 */
export const applyConversationSnapshot = applySessionSnapshot;

export function appendSessionPage(
  previousByKey: Map<string, SessionListItem>,
  previousDisplayOrder: string[],
  previousCanonicalOrder: string[],
  pageItems: SessionListItem[],
): {
  byKey: Map<string, SessionListItem>;
  displayOrder: string[];
  canonicalOrder: string[];
} {
  const byKey = new Map(previousByKey);
  const displayOrder = [...previousDisplayOrder];
  const seen = new Set(displayOrder);

  for (const item of pageItems) {
    const key = sessionListKey(item);
    byKey.set(key, mergeSessionListItem(byKey.get(key), item));
    if (!seen.has(key)) {
      displayOrder.push(key);
      seen.add(key);
    }
  }

  return {
    byKey,
    displayOrder,
    canonicalOrder: previousCanonicalOrder,
  };
}

/** @deprecated 兼容旧名 */
export const appendConversationPage = appendSessionPage;

export function itemsFromDisplayOrder(
  byKey: Map<string, SessionListItem>,
  displayOrder: string[],
): SessionListItem[] {
  const out: SessionListItem[] = [];
  for (const key of displayOrder) {
    const item = byKey.get(key);
    if (item) out.push(item);
  }
  return out;
}
