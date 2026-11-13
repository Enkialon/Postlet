import type {
  Account,
  Contact,
  Conversation,
  Group,
  Message,
} from '../../domain/entities';
import type { AccountId, MessageId } from '../../domain/types';
import type { MemoryStore } from './memoryStore';

/** 装载可演示分页、空态、合并群组与投影开关的验收数据 */
export function seedDemo(store: MemoryStore): void {
  const t = Date.now();
  const accountId: AccountId = 'acc-demo';
  const account: Account = {
    id: accountId,
    email: 'me@example.com',
    display_name: '演示用户',
    aliases: ['alias@example.com'],
    tenant_id: 'tenant-demo',
    home_account_id: 'home-demo',
    contact_delta_link: 'delta-contacts',
    mail_synced_at: t - 60_000,
    contact_synced_at: t - 120_000,
    mail_sync_status: 'idle',
    mail_sync_error: null,
    show_group_messages_in_contact_conversation: false,
    created_at: t - 86_400_000,
    updated_at: t,
  };
  store.accounts.set(accountId, account);

  const contactAlice: Contact = {
    id: 'ct-alice',
    account_id: accountId,
    graph_id: 'graph-alice',
    display_name: 'Alice Chen',
    normalized_address: 'alice@example.com',
    created_at: t,
    updated_at: t,
  };
  const contactBob: Contact = {
    id: 'ct-bob',
    account_id: accountId,
    graph_id: null,
    display_name: '',
    normalized_address: 'bob@example.com',
    created_at: t,
    updated_at: t,
  };
  const contactCarol: Contact = {
    id: 'ct-carol',
    account_id: accountId,
    graph_id: 'graph-carol',
    display_name: 'Carol Wang',
    normalized_address: 'carol@example.com',
    created_at: t,
    updated_at: t,
  };
  const contactEve: Contact = {
    id: 'ct-eve',
    account_id: accountId,
    graph_id: 'graph-eve',
    display_name: 'Eve Empty',
    normalized_address: 'eve@example.com',
    created_at: t,
    updated_at: t,
  };

  for (const c of [contactAlice, contactBob, contactCarol, contactEve]) {
    store.contacts.set(store.contactKey(accountId, c.id), c);
  }

  // 分页用占位联系人（会话列表 PAGE_SIZE=50）
  for (let i = 1; i <= 52; i += 1) {
    const n = String(i).padStart(2, '0');
    const contact: Contact = {
      id: `ct-pad-${n}`,
      account_id: accountId,
      graph_id: `graph-pad-${n}`,
      display_name: `Pad Contact ${n}`,
      normalized_address: `pad${n}@example.com`,
      created_at: t - i * 1000,
      updated_at: t - i * 1000,
    };
    store.contacts.set(store.contactKey(accountId, contact.id), contact);
  }

  const groupProduct: Group = {
    id: 'grp-product',
    account_id: accountId,
    name: '产品评审',
    name_is_manual: true,
    source: 'manual',
    send_members_is_manual: true,
    merged_into_id: null,
    created_at: t,
    updated_at: t,
  };
  const groupLegacy: Group = {
    id: 'grp-legacy',
    account_id: accountId,
    name: '旧项目组',
    name_is_manual: true,
    source: 'auto',
    send_members_is_manual: false,
    merged_into_id: 'grp-product',
    created_at: t - 7_200_000,
    updated_at: t - 3_600_000,
  };
  const groupOps: Group = {
    id: 'grp-ops',
    account_id: accountId,
    name: '运维值班',
    name_is_manual: true,
    source: 'manual',
    send_members_is_manual: true,
    merged_into_id: null,
    created_at: t - 86_400_000,
    updated_at: t,
  };

  store.groups.set(store.groupKey(accountId, groupProduct.id), groupProduct);
  store.groups.set(store.groupKey(accountId, groupLegacy.id), groupLegacy);
  store.groups.set(store.groupKey(accountId, groupOps.id), groupOps);

  store.participants.set(store.groupKey(accountId, groupProduct.id), [
    {
      account_id: accountId,
      group_id: groupProduct.id,
      normalized_address: 'alice@example.com',
      display_name: 'Alice Chen',
      is_observed: true,
      is_send_member: true,
      first_seen_at: t,
      last_seen_at: t,
    },
    {
      account_id: accountId,
      group_id: groupProduct.id,
      normalized_address: 'carol@example.com',
      display_name: 'Carol Wang',
      is_observed: true,
      is_send_member: false,
      first_seen_at: t,
      last_seen_at: t,
    },
  ]);
  store.participants.set(store.groupKey(accountId, groupLegacy.id), [
    {
      account_id: accountId,
      group_id: groupLegacy.id,
      normalized_address: 'bob@example.com',
      display_name: '',
      is_observed: true,
      is_send_member: true,
      first_seen_at: t - 7_200_000,
      last_seen_at: t - 3_600_000,
    },
  ]);
  store.participants.set(store.groupKey(accountId, groupOps.id), [
    {
      account_id: accountId,
      group_id: groupOps.id,
      normalized_address: 'carol@example.com',
      display_name: 'Carol Wang',
      is_observed: true,
      is_send_member: true,
      first_seen_at: t,
      last_seen_at: t,
    },
  ]);

  function putMessage(
    msg: Message,
    recipients: { address: string; name: string; role?: 'to' | 'cc' }[],
    pendingBody?: string,
  ): void {
    store.messages.set(store.messageKey(accountId, msg.id), msg);
    store.recipients.set(
      store.messageKey(accountId, msg.id),
      recipients.map((r, seq) => ({
        account_id: accountId,
        message_id: msg.id,
        role: r.role ?? 'to',
        seq,
        normalized_address: r.address,
        display_name: r.name,
      })),
    );
    if (pendingBody !== undefined) {
      store.pendingBodies.set(store.messageKey(accountId, msg.id), pendingBody);
    }
  }

  const msg1: Message = {
    id: 'msg-1',
    account_id: accountId,
    graph_id: 'g-msg-1',
    thread_id: 'thr-alice-1',
    graph_change_key: null,
    internet_message_id: null,
    conversation_index: null,
    subject: '下周发布计划',
    body_preview: '请确认里程碑与风险项……',
    body_content_type: 'text',
    body_content: null,
    from_address: 'alice@example.com',
    from_name: 'Alice Chen',
    received_at: t - 3_600_000,
    sent_at: t - 3_600_000,
    created_at_graph: t - 3_600_000,
    is_read: false,
    is_draft: false,
    is_outbound: false,
    has_attachments: true,
    importance: 'normal',
    folder_id: 'inbox',
    parent_folder_name: '收件箱',
    sync_status: 'synced',
    local_updated_at: t,
    remote_updated_at: t,
  };
  const msg2: Message = {
    id: 'msg-2',
    account_id: accountId,
    graph_id: 'g-msg-2',
    thread_id: 'thr-alice-1',
    graph_change_key: null,
    internet_message_id: null,
    conversation_index: null,
    subject: 'Re: 下周发布计划',
    body_preview: '我这边没问题，附件里有清单。',
    body_content_type: 'html',
    body_content: null,
    from_address: 'me@example.com',
    from_name: '演示用户',
    received_at: t - 1_800_000,
    sent_at: t - 1_800_000,
    created_at_graph: t - 1_800_000,
    is_read: true,
    is_draft: false,
    is_outbound: true,
    has_attachments: false,
    importance: 'normal',
    folder_id: 'sent',
    parent_folder_name: '已发送',
    sync_status: 'synced',
    local_updated_at: t,
    remote_updated_at: t,
  };
  const msg3: Message = {
    id: 'msg-3',
    account_id: accountId,
    graph_id: 'g-msg-3',
    thread_id: 'thr-group-1',
    graph_change_key: null,
    internet_message_id: null,
    conversation_index: null,
    subject: '评审纪要',
    body_preview: '会议纪要如下……',
    body_content_type: 'text',
    body_content: '会议纪要如下：\n1. 确认范围\n2. 排期下周',
    from_address: 'carol@example.com',
    from_name: 'Carol Wang',
    received_at: t - 900_000,
    sent_at: t - 900_000,
    created_at_graph: t - 900_000,
    is_read: false,
    is_draft: false,
    is_outbound: false,
    has_attachments: false,
    importance: 'high',
    folder_id: 'inbox',
    parent_folder_name: '收件箱',
    sync_status: 'synced',
    local_updated_at: t,
    remote_updated_at: t,
  };
  const msg4: Message = {
    id: 'msg-4',
    account_id: accountId,
    graph_id: 'g-msg-4',
    thread_id: null,
    graph_change_key: null,
    internet_message_id: null,
    conversation_index: null,
    subject: '无线程占位往来',
    body_preview: '这是一封没有 ThreadId 的邮件。',
    body_content_type: 'text',
    body_content: '',
    from_address: 'bob@example.com',
    from_name: '',
    received_at: t - 7_200_000,
    sent_at: t - 7_200_000,
    created_at_graph: t - 7_200_000,
    is_read: true,
    is_draft: false,
    is_outbound: false,
    has_attachments: false,
    importance: 'low',
    folder_id: 'inbox',
    parent_folder_name: '收件箱',
    sync_status: 'synced',
    local_updated_at: t,
    remote_updated_at: t,
  };
  const msg5: Message = {
    id: 'msg-5',
    account_id: accountId,
    graph_id: 'g-msg-5',
    thread_id: 'thr-ops-1',
    graph_change_key: null,
    internet_message_id: null,
    conversation_index: null,
    subject: '值班交接',
    body_preview: '今晚告警阈值已交接。',
    body_content_type: 'text',
    body_content: '今晚告警阈值已交接，请关注磁盘水位。',
    from_address: 'carol@example.com',
    from_name: 'Carol Wang',
    received_at: t - 400_000,
    sent_at: t - 400_000,
    created_at_graph: t - 400_000,
    is_read: false,
    is_draft: false,
    is_outbound: false,
    has_attachments: false,
    importance: 'normal',
    folder_id: 'inbox',
    parent_folder_name: '收件箱',
    sync_status: 'synced',
    local_updated_at: t,
    remote_updated_at: t,
  };

  putMessage(msg1, [{ address: 'me@example.com', name: '演示用户' }], '请确认里程碑与风险项。\n\n谢谢。');
  putMessage(
    msg2,
    [{ address: 'alice@example.com', name: 'Alice Chen' }],
    '<p>我这边没问题，附件里有清单。</p><p><a href="https://example.com">查看计划</a></p><script>alert(1)</script>',
  );
  putMessage(msg3, [
    { address: 'me@example.com', name: '演示用户' },
    { address: 'alice@example.com', name: 'Alice Chen', role: 'cc' },
  ]);
  putMessage(msg4, [{ address: 'me@example.com', name: '演示用户' }]);
  putMessage(msg5, [{ address: 'me@example.com', name: '演示用户' }]);

  // Alice 会话追加历史，覆盖消息分页（PAGE_SIZE=30）
  const aliceExtraIds: MessageId[] = [];
  for (let i = 1; i <= 34; i += 1) {
    const id = `msg-alice-hist-${String(i).padStart(2, '0')}`;
    const at = t - 10_000_000 - i * 60_000;
    const msg: Message = {
      id,
      account_id: accountId,
      graph_id: `g-${id}`,
      thread_id: 'thr-alice-hist',
      graph_change_key: null,
      internet_message_id: null,
      conversation_index: null,
      subject: `历史往来 #${i}`,
      body_preview: `这是 Alice 历史邮件 ${i} 的预览。`,
      body_content_type: 'text',
      body_content: `正文：Alice 历史邮件 ${i}`,
      from_address: i % 2 === 0 ? 'me@example.com' : 'alice@example.com',
      from_name: i % 2 === 0 ? '演示用户' : 'Alice Chen',
      received_at: at,
      sent_at: at,
      created_at_graph: at,
      is_read: true,
      is_draft: false,
      is_outbound: i % 2 === 0,
      has_attachments: false,
      importance: 'normal',
      folder_id: i % 2 === 0 ? 'sent' : 'inbox',
      parent_folder_name: i % 2 === 0 ? '已发送' : '收件箱',
      sync_status: 'synced',
      local_updated_at: t,
      remote_updated_at: t,
    };
    putMessage(msg, [
      {
        address: i % 2 === 0 ? 'alice@example.com' : 'me@example.com',
        name: i % 2 === 0 ? 'Alice Chen' : '演示用户',
      },
    ]);
    aliceExtraIds.push(id);
  }

  const convAlice: Conversation = {
    id: 'conv-alice',
    account_id: accountId,
    kind: 'contact',
    normalized_address: 'alice@example.com',
    contact_id: contactAlice.id,
    group_id: null,
    last_message_at: msg2.received_at,
    last_message_id: msg2.id,
    unread_count: 1,
    created_at: t,
    updated_at: t,
  };
  const convBob: Conversation = {
    id: 'conv-bob',
    account_id: accountId,
    kind: 'contact',
    normalized_address: 'bob@example.com',
    contact_id: contactBob.id,
    group_id: null,
    last_message_at: msg4.received_at,
    last_message_id: msg4.id,
    unread_count: 0,
    created_at: t,
    updated_at: t,
  };
  const convCarol: Conversation = {
    id: 'conv-carol',
    account_id: accountId,
    kind: 'contact',
    normalized_address: 'carol@example.com',
    contact_id: contactCarol.id,
    group_id: null,
    last_message_at: null,
    last_message_id: null,
    unread_count: 0,
    created_at: t,
    updated_at: t,
  };
  const convEve: Conversation = {
    id: 'conv-eve',
    account_id: accountId,
    kind: 'contact',
    normalized_address: 'eve@example.com',
    contact_id: contactEve.id,
    group_id: null,
    last_message_at: null,
    last_message_id: null,
    unread_count: 0,
    created_at: t,
    updated_at: t,
  };
  const convProduct: Conversation = {
    id: 'conv-product',
    account_id: accountId,
    kind: 'group',
    normalized_address: null,
    contact_id: null,
    group_id: groupProduct.id,
    last_message_at: msg3.received_at,
    last_message_id: msg3.id,
    unread_count: 1,
    created_at: t,
    updated_at: t,
  };
  const convOps: Conversation = {
    id: 'conv-ops',
    account_id: accountId,
    kind: 'group',
    normalized_address: null,
    contact_id: null,
    group_id: groupOps.id,
    last_message_at: msg5.received_at,
    last_message_id: msg5.id,
    unread_count: 1,
    created_at: t,
    updated_at: t,
  };

  for (const c of [
    convAlice,
    convBob,
    convCarol,
    convEve,
    convProduct,
    convOps,
  ]) {
    store.conversations.set(store.conversationKey(accountId, c.id), c);
  }

  store.conversationMessages.set(store.conversationKey(accountId, convAlice.id), [
    'msg-2',
    'msg-1',
    ...aliceExtraIds,
  ]);
  store.conversationMessages.set(store.conversationKey(accountId, convBob.id), [
    'msg-4',
  ]);
  store.conversationMessages.set(
    store.conversationKey(accountId, convCarol.id),
    [],
  );
  store.conversationMessages.set(
    store.conversationKey(accountId, convEve.id),
    [],
  );
  store.conversationMessages.set(
    store.conversationKey(accountId, convProduct.id),
    ['msg-3'],
  );
  store.conversationMessages.set(store.conversationKey(accountId, convOps.id), [
    'msg-5',
  ]);

  for (let i = 1; i <= 52; i += 1) {
    const n = String(i).padStart(2, '0');
    const contactId = `ct-pad-${n}`;
    const address = `pad${n}@example.com`;
    const msgId = `msg-pad-${n}`;
    const at = t - 20_000_000 - i * 30_000;
    const msg: Message = {
      id: msgId,
      account_id: accountId,
      graph_id: `g-${msgId}`,
      thread_id: `thr-pad-${n}`,
      graph_change_key: null,
      internet_message_id: null,
      conversation_index: null,
      subject: `占位会话 ${n}`,
      body_preview: `Pad ${n} 预览`,
      body_content_type: 'text',
      body_content: `Pad ${n} 正文`,
      from_address: address,
      from_name: `Pad Contact ${n}`,
      received_at: at,
      sent_at: at,
      created_at_graph: at,
      is_read: i % 7 !== 0,
      is_draft: false,
      is_outbound: false,
      has_attachments: false,
      importance: 'normal',
      folder_id: 'inbox',
      parent_folder_name: '收件箱',
      sync_status: 'synced',
      local_updated_at: t,
      remote_updated_at: t,
    };
    putMessage(msg, [{ address: 'me@example.com', name: '演示用户' }]);

    const conv: Conversation = {
      id: `conv-pad-${n}`,
      account_id: accountId,
      kind: 'contact',
      normalized_address: address,
      contact_id: contactId,
      group_id: null,
      last_message_at: at,
      last_message_id: msgId,
      unread_count: i % 7 === 0 ? 1 : 0,
      created_at: at,
      updated_at: at,
    };
    store.conversations.set(store.conversationKey(accountId, conv.id), conv);
    store.conversationMessages.set(store.conversationKey(accountId, conv.id), [
      msgId,
    ]);
  }

  store.folderStates.set(accountId, [
    {
      account_id: accountId,
      folder_id: 'inbox',
      delta_link: 'delta-inbox',
      next_link: null,
      synced_at: t - 60_000,
      sync_status: 'idle',
      last_error: null,
      updated_at: t,
    },
    {
      account_id: accountId,
      folder_id: 'archive',
      delta_link: null,
      next_link: 'page-archive-2',
      synced_at: null,
      sync_status: 'error',
      last_error: '演示：归档文件夹增量失败',
      updated_at: t - 30_000,
    },
  ]);

  store.recomputeAllConversations(accountId);
}
