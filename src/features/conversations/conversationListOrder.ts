import type { ConversationListItem } from '../../domain/dto';

export const LIST_TOP_ENTER_PX = 60;
export const LIST_TOP_LEAVE_PX = 100;

export function conversationListKey(item: ConversationListItem): string {
  return `${item.conversation.account_id}:${item.conversation.id}`;
}

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
  previous: ConversationListItem | undefined,
  next: ConversationListItem,
): boolean {
  const preview = next.last_message_preview;
  if (!preview || preview.is_outbound) return false;
  const nextId = next.conversation.last_message_id;
  if (!nextId) return false;
  if (!previous) return true;
  return previous.conversation.last_message_id !== nextId;
}

export function mergeConversationListItem(
  existing: ConversationListItem | undefined,
  next: ConversationListItem,
): ConversationListItem {
  if (!existing) {
    return {
      ...next,
      conversation: { ...next.conversation },
      last_message_preview: next.last_message_preview
        ? { ...next.last_message_preview }
        : null,
      contact_display_name: next.contact_display_name,
      group_name: next.group_name,
    };
  }
  Object.assign(existing.conversation, next.conversation);
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

export interface ApplyConversationSnapshotInput {
  snapshot: ConversationListItem[];
  previousByKey: Map<string, ConversationListItem>;
  previousCanonicalOrder: string[];
  previousDisplayOrder: string[];
  previousPendingActivityKeys: string[];
  atTop: boolean;
  /** 筛选 / 账户切换等：强制应用最新排序 */
  forceApplyOrder: boolean;
}

export interface ApplyConversationSnapshotResult {
  byKey: Map<string, ConversationListItem>;
  canonicalOrder: string[];
  displayOrder: string[];
  pendingActivityKeys: string[];
  orderApplied: boolean;
}

/**
 * 将后端会话页合并进前端状态：始终更新数据；按是否在顶部决定是否应用排序。
 */
export function applyConversationSnapshot(
  input: ApplyConversationSnapshotInput,
): ApplyConversationSnapshotResult {
  const {
    snapshot,
    previousByKey,
    previousCanonicalOrder,
    previousDisplayOrder,
    previousPendingActivityKeys,
    atTop,
    forceApplyOrder,
  } = input;

  const canonicalOrder = snapshot.map(conversationListKey);
  const snapshotKeySet = new Set(canonicalOrder);
  const prevCanonicalSet = new Set(previousCanonicalOrder);

  const byKey = new Map<string, ConversationListItem>();
  const activity: string[] = [];

  for (const item of snapshot) {
    const key = conversationListKey(item);
    const prev = previousByKey.get(key);
    if (
      !forceApplyOrder &&
      !atTop &&
      isLikelyNewIncomingActivity(prev, item)
    ) {
      activity.push(key);
    }
    byKey.set(key, mergeConversationListItem(prev, item));
  }

  // 冻结时保留 loadMore 带入、且仍在 previousByKey 中的项
  if (!forceApplyOrder && !atTop) {
    for (const key of previousDisplayOrder) {
      if (byKey.has(key) || snapshotKeySet.has(key)) continue;
      if (prevCanonicalSet.has(key)) continue; // 曾在首页、现已不在 → 删除
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

export function appendConversationPage(
  previousByKey: Map<string, ConversationListItem>,
  previousDisplayOrder: string[],
  previousCanonicalOrder: string[],
  pageItems: ConversationListItem[],
): {
  byKey: Map<string, ConversationListItem>;
  displayOrder: string[];
  canonicalOrder: string[];
} {
  const byKey = new Map(previousByKey);
  const displayOrder = [...previousDisplayOrder];
  const seen = new Set(displayOrder);

  for (const item of pageItems) {
    const key = conversationListKey(item);
    byKey.set(key, mergeConversationListItem(byKey.get(key), item));
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

export function itemsFromDisplayOrder(
  byKey: Map<string, ConversationListItem>,
  displayOrder: string[],
): ConversationListItem[] {
  const out: ConversationListItem[] = [];
  for (const key of displayOrder) {
    const item = byKey.get(key);
    if (item) out.push(item);
  }
  return out;
}
