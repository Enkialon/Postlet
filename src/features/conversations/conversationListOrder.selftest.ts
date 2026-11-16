/**
 * 无测试框架自检：npx --yes tsx src/features/conversations/conversationListOrder.selftest.ts
 */
import type { SessionListItem } from '../../domain/dto';
import {
  applySessionSnapshot,
  appendSessionPage,
  isLikelyNewIncomingActivity,
  isScrollAtTop,
  itemsFromDisplayOrder,
} from './conversationListOrder';

function item(partial: {
  account?: string;
  id: string;
  lastMessageId?: string | null;
  outbound?: boolean;
  unread?: number;
  at?: number;
}): SessionListItem {
  const account = partial.account ?? 'acc';
  const lastId =
    partial.lastMessageId === undefined ? `m-${partial.id}` : partial.lastMessageId;
  return {
    session: {
      id: partial.id,
      account_id: account,
      kind: 'private',
      contact_id: null,
      group_id: null,
      merged_into_id: null,
      normalized_address: `${partial.id}@ex.com`,
      last_message_at: partial.at ?? 1000,
      last_message_id: lastId,
      unread_count: partial.unread ?? 0,
      hidden_at: null,
      created_at: 1,
      updated_at: 1,
    },
    display_name: partial.id,
    last_message_preview: lastId
      ? {
          id: lastId,
          subject: 's',
          body_preview: 'p',
          from_address: 'a@ex.com',
          from_name: 'A',
          received_at: partial.at ?? 1000,
          sent_at: null,
          is_read: false,
          is_outbound: partial.outbound ?? false,
          has_attachments: false,
          importance: 'normal',
        }
      : null,
    contact_display_name: null,
    group_name: null,
  };
}

function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

{
  assert(isScrollAtTop(0, false), 'enter top');
  assert(!isScrollAtTop(80, false), 'not enter');
  assert(isScrollAtTop(90, true), 'leave still top');
  assert(!isScrollAtTop(120, true), 'left top');
}

{
  const prev = item({ id: 'a', lastMessageId: 'm1' });
  const next = item({ id: 'a', lastMessageId: 'm2' });
  assert(isLikelyNewIncomingActivity(prev, next), 'new incoming');
  assert(
    !isLikelyNewIncomingActivity(prev, item({ id: 'a', lastMessageId: 'm2', outbound: true })),
    'outbound not activity',
  );
}

{
  const first = applySessionSnapshot({
    snapshot: [item({ id: 'a' }), item({ id: 'b' })],
    previousByKey: new Map(),
    previousCanonicalOrder: [],
    previousDisplayOrder: [],
    previousPendingActivityKeys: [],
    atTop: true,
    forceApplyOrder: true,
  });
  const second = applySessionSnapshot({
    snapshot: [
      item({ id: 'a', unread: 3, lastMessageId: 'm-a2' }),
      item({ id: 'b' }),
    ],
    previousByKey: first.byKey,
    previousCanonicalOrder: first.canonicalOrder,
    previousDisplayOrder: first.displayOrder,
    previousPendingActivityKeys: [],
    atTop: true,
    forceApplyOrder: false,
  });
  assert(second.byKey.get('acc:a')?.session.unread_count === 3, 'unread');
}

{
  const base = applySessionSnapshot({
    snapshot: [item({ id: 'a' }), item({ id: 'b' })],
    previousByKey: new Map(),
    previousCanonicalOrder: [],
    previousDisplayOrder: [],
    previousPendingActivityKeys: [],
    atTop: true,
    forceApplyOrder: true,
  });
  const next = applySessionSnapshot({
    snapshot: [
      item({ id: 'b', unread: 1, lastMessageId: 'm-b2' }),
      item({ id: 'a' }),
    ],
    previousByKey: base.byKey,
    previousCanonicalOrder: base.canonicalOrder,
    previousDisplayOrder: base.displayOrder,
    previousPendingActivityKeys: [],
    atTop: false,
    forceApplyOrder: false,
  });
  assert(next.byKey.get('acc:b')?.session.unread_count === 1, 'field update');
  assert(next.displayOrder[0] === 'acc:a', 'frozen order');
  assert(next.pendingActivityKeys.includes('acc:b'), 'pending activity');
}

{
  const base = applySessionSnapshot({
    snapshot: [item({ id: 'a' }), item({ id: 'b' })],
    previousByKey: new Map(),
    previousCanonicalOrder: [],
    previousDisplayOrder: [],
    previousPendingActivityKeys: [],
    atTop: true,
    forceApplyOrder: true,
  });
  const next = applySessionSnapshot({
    snapshot: [item({ id: 'b' })],
    previousByKey: base.byKey,
    previousCanonicalOrder: base.canonicalOrder,
    previousDisplayOrder: base.displayOrder,
    previousPendingActivityKeys: [],
    atTop: false,
    forceApplyOrder: false,
  });
  assert(!next.byKey.has('acc:a'), 'removed from snapshot while frozen');
  assert(!next.displayOrder.includes('acc:a'), 'removed from display');
}

{
  const base = applySessionSnapshot({
    snapshot: [item({ id: 'a' })],
    previousByKey: new Map(),
    previousCanonicalOrder: [],
    previousDisplayOrder: [],
    previousPendingActivityKeys: [],
    atTop: true,
    forceApplyOrder: true,
  });
  const next = applySessionSnapshot({
    snapshot: [item({ id: 'a' }), item({ id: 'c' })],
    previousByKey: base.byKey,
    previousCanonicalOrder: base.canonicalOrder,
    previousDisplayOrder: [...base.displayOrder, 'acc:extra'],
    previousPendingActivityKeys: [],
    atTop: false,
    forceApplyOrder: false,
  });
  assert(!next.byKey.has('acc:extra'), 'unknown extra dropped');
}

{
  const base = applySessionSnapshot({
    snapshot: [item({ id: 'a' })],
    previousByKey: new Map(),
    previousCanonicalOrder: [],
    previousDisplayOrder: [],
    previousPendingActivityKeys: [],
    atTop: true,
    forceApplyOrder: true,
  });
  const next = appendSessionPage(
    base.byKey,
    base.displayOrder,
    base.canonicalOrder,
    [item({ id: 'z' })],
  );
  assert(next.displayOrder.includes('acc:z'), 'append');
  const items = itemsFromDisplayOrder(next.byKey, next.displayOrder);
  assert(items.length === 2, 'items length');
}

console.log('conversationListOrder.selftest: all passed');
