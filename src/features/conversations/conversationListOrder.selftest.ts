/**
 * 无测试框架自检：npx --yes tsx src/features/conversations/conversationListOrder.selftest.ts
 */
import type { ConversationListItem } from '../../domain/dto';
import {
  applyConversationSnapshot,
  appendConversationPage,
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
}): ConversationListItem {
  const account = partial.account ?? 'acc';
  const lastId =
    partial.lastMessageId === undefined ? `m-${partial.id}` : partial.lastMessageId;
  return {
    conversation: {
      id: partial.id,
      account_id: account,
      kind: 'contact',
      contact_id: null,
      group_id: null,
      normalized_address: `${partial.id}@ex.com`,
      last_message_at: partial.at ?? 1000,
      last_message_id: lastId,
      unread_count: partial.unread ?? 0,
      hidden_at: null,
      created_at: 1,
      updated_at: 1,
    },
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
    contact_display_name: partial.id,
    group_name: null,
  };
}

function assert(cond: unknown, msg: string): void {
  if (!cond) throw new Error(msg);
}

function assertEq<T>(actual: T, expected: T, msg: string): void {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) throw new Error(`${msg}: got ${a}, want ${e}`);
}

{
  const a1 = item({ id: 'a', lastMessageId: 'm1', at: 1 });
  const a2 = item({ id: 'a', lastMessageId: 'm2', at: 2, unread: 3 });
  const first = applyConversationSnapshot({
    snapshot: [a1],
    previousByKey: new Map(),
    previousCanonicalOrder: [],
    previousDisplayOrder: [],
    previousPendingActivityKeys: [],
    atTop: true,
    forceApplyOrder: true,
  });
  const second = applyConversationSnapshot({
    snapshot: [a2],
    previousByKey: first.byKey,
    previousCanonicalOrder: first.canonicalOrder,
    previousDisplayOrder: first.displayOrder,
    previousPendingActivityKeys: [],
    atTop: true,
    forceApplyOrder: false,
  });
  assertEq(second.displayOrder, ['acc:a'], 'merge no dup');
  assert(second.byKey.get('acc:a')?.conversation.unread_count === 3, 'unread');
}

{
  const a = item({ id: 'a', lastMessageId: 'ma', at: 1 });
  const b = item({ id: 'b', lastMessageId: 'mb', at: 2 });
  const c = item({ id: 'c', lastMessageId: 'mc', at: 3 });
  const base = applyConversationSnapshot({
    snapshot: [c, b, a],
    previousByKey: new Map(),
    previousCanonicalOrder: [],
    previousDisplayOrder: [],
    previousPendingActivityKeys: [],
    atTop: true,
    forceApplyOrder: true,
  });
  const b2 = item({ id: 'b', lastMessageId: 'mb2', at: 99, unread: 1 });
  const next = applyConversationSnapshot({
    snapshot: [b2, c, a],
    previousByKey: base.byKey,
    previousCanonicalOrder: base.canonicalOrder,
    previousDisplayOrder: base.displayOrder,
    previousPendingActivityKeys: [],
    atTop: false,
    forceApplyOrder: false,
  });
  assertEq(next.displayOrder, ['acc:c', 'acc:b', 'acc:a'], 'freeze order');
  assertEq(next.canonicalOrder, ['acc:b', 'acc:c', 'acc:a'], 'canonical');
  assert(next.byKey.get('acc:b')?.conversation.unread_count === 1, 'field update');
  assert(next.pendingActivityKeys.includes('acc:b'), 'activity');
  assert(!next.orderApplied, 'not applied');
}

{
  const a = item({ id: 'a', lastMessageId: 'ma', at: 1 });
  const b = item({ id: 'b', lastMessageId: 'mb', at: 2 });
  const base = applyConversationSnapshot({
    snapshot: [a, b],
    previousByKey: new Map(),
    previousCanonicalOrder: [],
    previousDisplayOrder: [],
    previousPendingActivityKeys: [],
    atTop: true,
    forceApplyOrder: true,
  });
  const b2 = item({ id: 'b', lastMessageId: 'mb2', at: 99 });
  const next = applyConversationSnapshot({
    snapshot: [b2, a],
    previousByKey: base.byKey,
    previousCanonicalOrder: base.canonicalOrder,
    previousDisplayOrder: ['acc:a', 'acc:b'],
    previousPendingActivityKeys: ['acc:b'],
    atTop: true,
    forceApplyOrder: false,
  });
  assertEq(next.displayOrder, ['acc:b', 'acc:a'], 'at top reorder');
  assertEq(next.pendingActivityKeys, [], 'clear pending');
}

{
  const prev = item({ id: 'a', lastMessageId: 'm1' });
  assert(!isLikelyNewIncomingActivity(prev, item({ id: 'a', lastMessageId: 'm1', unread: 9 })), 'same id');
  assert(
    !isLikelyNewIncomingActivity(prev, item({ id: 'a', lastMessageId: 'm2', outbound: true })),
    'outbound',
  );
}

{
  const a = item({ id: 'a', at: 1 });
  const b = item({ id: 'b', at: 2 });
  const base = applyConversationSnapshot({
    snapshot: [b, a],
    previousByKey: new Map(),
    previousCanonicalOrder: [],
    previousDisplayOrder: [],
    previousPendingActivityKeys: [],
    atTop: true,
    forceApplyOrder: true,
  });
  const next = applyConversationSnapshot({
    snapshot: [a],
    previousByKey: base.byKey,
    previousCanonicalOrder: base.canonicalOrder,
    previousDisplayOrder: base.displayOrder,
    previousPendingActivityKeys: [],
    atTop: false,
    forceApplyOrder: false,
  });
  assertEq(next.displayOrder, ['acc:a'], 'delete while frozen');
  assert(!next.byKey.has('acc:b'), 'b removed');
}

{
  const a = item({ id: 'a', at: 2 });
  const b = item({ id: 'b', at: 1 });
  const base = applyConversationSnapshot({
    snapshot: [a],
    previousByKey: new Map(),
    previousCanonicalOrder: [],
    previousDisplayOrder: [],
    previousPendingActivityKeys: [],
    atTop: true,
    forceApplyOrder: true,
  });
  const next = appendConversationPage(base.byKey, base.displayOrder, base.canonicalOrder, [
    b,
    a,
  ]);
  assertEq(next.displayOrder, ['acc:a', 'acc:b'], 'append');
}

assert(isScrollAtTop(50, false), 'enter top');
assert(isScrollAtTop(80, true), 'hysteresis stay');
assert(!isScrollAtTop(120, true), 'leave top');
assert(!isScrollAtTop(80, false), 'hysteresis out');

{
  const a = item({ id: 'a' });
  const map = new Map([['acc:a', a]]);
  assertEq(itemsFromDisplayOrder(map, ['acc:a', 'acc:missing']), [a], 'skip missing');
}

console.log('conversationListOrder.selftest: all passed');
