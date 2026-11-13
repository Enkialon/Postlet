import type {
  Account,
  Contact,
  Conversation,
  Group,
  GroupParticipant,
  MailFolderSyncState,
  Message,
  MessageRecipient,
  MessageWithRecipients,
} from '../../domain/entities';
import type {
  ConversationListItem,
  MessagePreview,
} from '../../domain/dto';
import type {
  AccountId,
  ConversationId,
  EmailAddress,
  GroupId,
  MessageId,
  ThreadId,
} from '../../domain/types';
import type { AppError } from '../../domain/errors';

function now(): number {
  return Date.now();
}

function appError(
  code: AppError['code'],
  message: string,
  retryable = false,
): AppError {
  return { code, message, retryable };
}

/** 开发 / 验收用内存数据仓 */
export class MemoryStore {
  accounts = new Map<AccountId, Account>();
  contacts = new Map<string, Contact>();
  conversations = new Map<string, Conversation>();
  conversationMessages = new Map<string, MessageId[]>();
  messages = new Map<string, Message>();
  recipients = new Map<string, MessageRecipient[]>();
  groups = new Map<string, Group>();
  participants = new Map<string, GroupParticipant[]>();
  folderStates = new Map<AccountId, MailFolderSyncState[]>();
  /** 未加载正文的远端内容缓存（ensureMessageBody 时写入 message.body_content） */
  pendingBodies = new Map<string, string>();

  contactKey(accountId: AccountId, contactId: string): string {
    return `${accountId}:${contactId}`;
  }

  conversationKey(accountId: AccountId, conversationId: string): string {
    return `${accountId}:${conversationId}`;
  }

  messageKey(accountId: AccountId, messageId: string): string {
    return `${accountId}:${messageId}`;
  }

  groupKey(accountId: AccountId, groupId: string): string {
    return `${accountId}:${groupId}`;
  }

  failNotFound(message = '未找到资源'): never {
    throw appError('not_found', message, false);
  }

  failValidation(message: string): never {
    throw appError('validation', message, false);
  }

  failConflict(message: string): never {
    throw appError('conflict', message, false);
  }

  requireAccount(accountId: AccountId): Account {
    const account = this.accounts.get(accountId);
    if (!account) this.failNotFound('账户不存在');
    return account;
  }

  toPreview(message: Message): MessagePreview {
    return {
      id: message.id,
      subject: message.subject,
      body_preview: message.body_preview,
      from_address: message.from_address,
      from_name: message.from_name,
      received_at: message.received_at,
      sent_at: message.sent_at,
      is_read: message.is_read,
      is_outbound: message.is_outbound,
      has_attachments: message.has_attachments,
      importance: message.importance,
    };
  }

  toListItem(conversation: Conversation): ConversationListItem {
    let last_message_preview: MessagePreview | null = null;
    if (conversation.last_message_id) {
      const msg = this.messages.get(
        this.messageKey(conversation.account_id, conversation.last_message_id),
      );
      if (msg && msg.sync_status !== 'deleted_remote') {
        last_message_preview = this.toPreview(msg);
      }
    }

    let contact_display_name: string | null = null;
    if (conversation.contact_id) {
      const contact = this.contacts.get(
        this.contactKey(conversation.account_id, conversation.contact_id),
      );
      contact_display_name = contact?.display_name || null;
    }

    let group_name: string | null = null;
    if (conversation.group_id) {
      const group = this.groups.get(
        this.groupKey(conversation.account_id, conversation.group_id),
      );
      group_name = group?.name || null;
    }

    return {
      conversation,
      last_message_preview,
      contact_display_name,
      group_name,
    };
  }

  withRecipients(
    accountId: AccountId,
    messageId: MessageId,
  ): MessageWithRecipients {
    const message = this.messages.get(this.messageKey(accountId, messageId));
    if (!message || message.sync_status === 'deleted_remote') {
      this.failNotFound('邮件不存在');
    }
    return {
      message,
      recipients: this.recipients.get(this.messageKey(accountId, messageId)) ?? [],
    };
  }

  recomputeConversation(accountId: AccountId, conversationId: ConversationId): void {
    const key = this.conversationKey(accountId, conversationId);
    const conversation = this.conversations.get(key);
    if (!conversation) return;

    const ids = this.conversationMessages.get(key) ?? [];
    const live: Message[] = [];
    for (const id of ids) {
      const msg = this.messages.get(this.messageKey(accountId, id));
      if (msg && msg.sync_status !== 'deleted_remote') live.push(msg);
    }

    live.sort((a, b) => {
      const ta = a.received_at ?? a.sent_at ?? 0;
      const tb = b.received_at ?? b.sent_at ?? 0;
      return tb - ta;
    });

    conversation.unread_count = live.filter((m) => !m.is_read).length;
    conversation.last_message_id = live[0]?.id ?? null;
    conversation.last_message_at =
      live[0]?.received_at ?? live[0]?.sent_at ?? null;
    conversation.updated_at = now();
    this.conversations.set(key, conversation);
  }

  normalizeEmail(address: string): EmailAddress {
    return address.trim().toLowerCase();
  }

  seedDemo(): void {
    const t = now();
    const accountId: AccountId = 'acc-demo';
    const account: Account = {
      id: accountId,
      email: 'me@example.com',
      display_name: '演示用户',
      aliases: [],
      tenant_id: 'tenant-demo',
      home_account_id: 'home-demo',
      contact_delta_link: null,
      mail_synced_at: t - 60_000,
      contact_synced_at: t - 120_000,
      mail_sync_status: 'idle',
      mail_sync_error: null,
      show_group_messages_in_contact_conversation: false,
      created_at: t - 86_400_000,
      updated_at: t,
    };
    this.accounts.set(accountId, account);

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
    this.contacts.set(this.contactKey(accountId, contactAlice.id), contactAlice);
    this.contacts.set(this.contactKey(accountId, contactBob.id), contactBob);

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
    this.groups.set(this.groupKey(accountId, groupProduct.id), groupProduct);
    this.participants.set(this.groupKey(accountId, groupProduct.id), [
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
        display_name: 'Carol',
        is_observed: true,
        is_send_member: false,
        first_seen_at: t,
        last_seen_at: t,
      },
    ]);

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
      from_name: 'Carol',
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

    const bodies: Record<string, string> = {
      'msg-1': '请确认里程碑与风险项。\n\n谢谢。',
      'msg-2':
        '<p>我这边没问题，附件里有清单。</p><p><a href="https://example.com">查看计划</a></p><script>alert(1)</script>',
    };

    for (const msg of [msg1, msg2, msg3, msg4]) {
      this.messages.set(this.messageKey(accountId, msg.id), msg);
      this.recipients.set(this.messageKey(accountId, msg.id), [
        {
          account_id: accountId,
          message_id: msg.id,
          role: 'to',
          seq: 0,
          normalized_address: msg.is_outbound
            ? (msg.id === 'msg-2' ? 'alice@example.com' : 'carol@example.com')
            : 'me@example.com',
          display_name: msg.is_outbound ? 'Alice Chen' : '演示用户',
        },
      ]);
      if (bodies[msg.id] !== undefined) {
        this.pendingBodies.set(
          this.messageKey(accountId, msg.id),
          bodies[msg.id],
        );
      }
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
    const convGroup: Conversation = {
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

    for (const c of [convAlice, convBob, convGroup]) {
      this.conversations.set(this.conversationKey(accountId, c.id), c);
    }
    this.conversationMessages.set(
      this.conversationKey(accountId, convAlice.id),
      ['msg-2', 'msg-1'],
    );
    this.conversationMessages.set(
      this.conversationKey(accountId, convBob.id),
      ['msg-4'],
    );
    this.conversationMessages.set(
      this.conversationKey(accountId, convGroup.id),
      ['msg-3'],
    );

    this.folderStates.set(accountId, [
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
    ]);
  }

  findContactConversation(
    accountId: AccountId,
    address: EmailAddress,
  ): Conversation | undefined {
    for (const c of this.conversations.values()) {
      if (
        c.account_id === accountId &&
        c.kind === 'contact' &&
        c.normalized_address === address
      ) {
        return c;
      }
    }
    return undefined;
  }

  nextId(prefix: string): string {
    return `${prefix}-${Math.random().toString(36).slice(2, 10)}`;
  }

  findRootGroup(accountId: AccountId, groupId: GroupId): Group {
    let current = this.groups.get(this.groupKey(accountId, groupId));
    if (!current) this.failNotFound('群组不存在');
    const seen = new Set<string>();
    while (current.merged_into_id) {
      if (seen.has(current.id)) this.failConflict('群组合并环');
      seen.add(current.id);
      const next = this.groups.get(
        this.groupKey(accountId, current.merged_into_id),
      );
      if (!next) this.failNotFound('合并目标不存在');
      current = next;
    }
    return current;
  }

  messagesForThread(
    accountId: AccountId,
    threadId: ThreadId,
  ): Message[] {
    return [...this.messages.values()].filter(
      (m) =>
        m.account_id === accountId &&
        m.thread_id === threadId &&
        m.sync_status !== 'deleted_remote',
    );
  }
}

export function createSeededMemoryStore(): MemoryStore {
  const store = new MemoryStore();
  store.seedDemo();
  return store;
}
