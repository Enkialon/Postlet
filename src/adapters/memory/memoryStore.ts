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
import { seedDemo as loadSeedDemo } from './seedDemo';

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
  /** 已登录会话；退出后账户行仍在，列表只认此集合 */
  signedInIds = new Set<AccountId>();
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
    // 读路径按投影开关即时计算，避免列表与详情未读漂移
    const live = this.effectiveMessagesForConversation(conversation);
    const last = live[0] ?? null;
    const snapshot: Conversation = {
      ...conversation,
      unread_count: live.filter((m) => !m.is_read).length,
      last_message_id: last?.id ?? null,
      last_message_at: last?.received_at ?? last?.sent_at ?? null,
    };

    let contact_display_name: string | null = null;
    if (snapshot.contact_id) {
      const contact = this.contacts.get(
        this.contactKey(snapshot.account_id, snapshot.contact_id),
      );
      contact_display_name = contact?.display_name || null;
    }

    let group_name: string | null = null;
    if (snapshot.group_id) {
      const group = this.groups.get(
        this.groupKey(snapshot.account_id, snapshot.group_id),
      );
      group_name = group?.name || null;
    }

    return {
      conversation: snapshot,
      last_message_preview: last ? this.toPreview(last) : null,
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

  messageInvolvesAddress(accountId: AccountId, messageId: MessageId, address: EmailAddress): boolean {
    const msg = this.messages.get(this.messageKey(accountId, messageId));
    if (!msg || msg.sync_status === 'deleted_remote') return false;
    if (msg.from_address === address) return true;
    const recipients = this.recipients.get(this.messageKey(accountId, messageId)) ?? [];
    return recipients.some((r) => r.normalized_address === address);
  }

  /** 联系人会话在开关开启时合并群组投影邮件 */
  effectiveMessagesForConversation(conversation: Conversation): Message[] {
    const accountId = conversation.account_id;
    const key = this.conversationKey(accountId, conversation.id);
    const ids = new Set(this.conversationMessages.get(key) ?? []);

    const account = this.accounts.get(accountId);
    if (
      account?.show_group_messages_in_contact_conversation &&
      conversation.kind === 'contact' &&
      conversation.normalized_address
    ) {
      const address = conversation.normalized_address;
      for (const conv of this.conversations.values()) {
        if (
          conv.account_id !== accountId ||
          conv.kind !== 'group' ||
          !conv.group_id
        ) {
          continue;
        }
        const groupIds =
          this.conversationMessages.get(
            this.conversationKey(accountId, conv.id),
          ) ?? [];
        for (const id of groupIds) {
          if (this.messageInvolvesAddress(accountId, id, address)) {
            ids.add(id);
          }
        }
      }
    }

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
    return live;
  }

  recomputeConversation(accountId: AccountId, conversationId: ConversationId): void {
    const key = this.conversationKey(accountId, conversationId);
    const conversation = this.conversations.get(key);
    if (!conversation) return;

    const live = this.effectiveMessagesForConversation(conversation);
    conversation.unread_count = live.filter((m) => !m.is_read).length;
    conversation.last_message_id = live[0]?.id ?? null;
    conversation.last_message_at =
      live[0]?.received_at ?? live[0]?.sent_at ?? null;
    conversation.updated_at = now();
    this.conversations.set(key, conversation);
  }

  recomputeAllConversations(accountId: AccountId): void {
    for (const conv of this.conversations.values()) {
      if (conv.account_id === accountId) {
        this.recomputeConversation(accountId, conv.id);
      }
    }
  }

  findGroupConversation(
    accountId: AccountId,
    groupId: GroupId,
  ): Conversation | undefined {
    const root = this.findRootGroup(accountId, groupId);
    for (const c of this.conversations.values()) {
      if (
        c.account_id === accountId &&
        c.kind === 'group' &&
        c.group_id === root.id
      ) {
        return c;
      }
    }
    return undefined;
  }

  ensureGroupConversationEntity(
    accountId: AccountId,
    groupId: GroupId,
  ): Conversation {
    const root = this.findRootGroup(accountId, groupId);
    const existing = this.findGroupConversation(accountId, root.id);
    if (existing) return existing;

    const t = now();
    const conversation: Conversation = {
      id: this.nextId('conv'),
      account_id: accountId,
      kind: 'group',
      normalized_address: null,
      contact_id: null,
      group_id: root.id,
      last_message_at: null,
      last_message_id: null,
      unread_count: 0,
      created_at: t,
      updated_at: t,
    };
    this.conversations.set(
      this.conversationKey(accountId, conversation.id),
      conversation,
    );
    this.conversationMessages.set(
      this.conversationKey(accountId, conversation.id),
      [],
    );
    return conversation;
  }

  clearAll(): void {
    this.accounts.clear();
    this.signedInIds.clear();
    this.contacts.clear();
    this.conversations.clear();
    this.conversationMessages.clear();
    this.messages.clear();
    this.recipients.clear();
    this.groups.clear();
    this.participants.clear();
    this.folderStates.clear();
    this.pendingBodies.clear();
  }

  markSignedIn(accountId: AccountId): void {
    this.signedInIds.add(accountId);
  }

  markSignedOut(accountId: AccountId): void {
    this.signedInIds.delete(accountId);
  }

  /** 「全部账号」列表：已登录且 include_in_all */
  idsForAllLists(): Set<AccountId> {
    return new Set(
      [...this.signedInIds].filter(
        (id) => this.accounts.get(id)?.include_in_all !== false,
      ),
    );
  }

  normalizeEmail(address: string): EmailAddress {
    return address.trim().toLowerCase();
  }

  seedDemo(): void {
    loadSeedDemo(this);
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
