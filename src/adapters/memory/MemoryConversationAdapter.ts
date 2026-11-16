import type { ConversationPort } from '../../ports/conversationPort';
import type { Contact, Conversation, MessageWithRecipients } from '../../domain/entities';
import type {
  ConversationListItem,
  ConversationListQuery,
  ConversationMessagesQuery,
  CursorPage,
  EnsureContactConversationCommand,
  EnsureGroupConversationCommand,
} from '../../domain/dto';
import type { AccountId, ConversationId } from '../../domain/types';
import type { MemoryStore } from './memoryStore';

function paginate<T>(
  items: T[],
  cursor: string | null,
  limit: number,
): CursorPage<T> {
  const start = cursor ? Number.parseInt(cursor, 10) : 0;
  const safeStart = Number.isFinite(start) && start >= 0 ? start : 0;
  const slice = items.slice(safeStart, safeStart + limit);
  const next = safeStart + slice.length;
  const has_more = next < items.length;
  return {
    items: slice,
    next_cursor: has_more ? String(next) : null,
    has_more,
  };
}

export class MemoryConversationAdapter implements ConversationPort {
  constructor(private readonly store: MemoryStore) {}

  async listConversations(
    query: ConversationListQuery,
  ): Promise<CursorPage<ConversationListItem>> {
    if (query.account_id) this.store.requireAccount(query.account_id);
    const allListIds = this.store.idsForAllLists();
    let items = [...this.store.conversations.values()]
      .filter((c) =>
        query.account_id
          ? c.account_id === query.account_id
          : allListIds.has(c.account_id),
      )
      .filter((c) => {
        if (c.hidden_at == null) return true;
        const live = this.store.effectiveMessagesForConversation(c);
        const lastAt = live[0]?.received_at ?? live[0]?.sent_at ?? null;
        if (lastAt != null && lastAt > c.hidden_at) {
          c.hidden_at = null;
          return true;
        }
        return false;
      })
      .filter((c) => (query.kind ? c.kind === query.kind : true))
      .sort((a, b) => (b.last_message_at ?? 0) - (a.last_message_at ?? 0))
      .map((c) => this.store.toListItem(c));
    return paginate(items, query.cursor, Math.max(1, Math.min(query.limit, 200)));
  }

  async getConversation(
    accountId: AccountId,
    conversationId: ConversationId,
  ): Promise<ConversationListItem> {
    this.store.requireAccount(accountId);
    const conversation = this.store.conversations.get(
      this.store.conversationKey(accountId, conversationId),
    );
    if (!conversation) this.store.failNotFound('会话不存在');
    return this.store.toListItem(conversation);
  }

  async listMessages(
    query: ConversationMessagesQuery,
  ): Promise<CursorPage<MessageWithRecipients>> {
    this.store.requireAccount(query.account_id);
    const key = this.store.conversationKey(
      query.account_id,
      query.conversation_id,
    );
    const conversation = this.store.conversations.get(key);
    if (!conversation) {
      this.store.failNotFound('会话不存在');
    }
    const messages = this.store.threadMessagesForConversation(conversation);
    const ordered =
      query.direction === 'newer' ? [...messages].reverse() : messages;

    const page = paginate(
      ordered,
      query.cursor,
      Math.max(1, Math.min(query.limit, 200)),
    );
    return {
      ...page,
      items: page.items.map((m) => ({
        message: { ...m },
        recipients: [
          ...(this.store.recipients.get(
            this.store.messageKey(query.account_id, m.id),
          ) ?? []),
        ],
      })),
    };
  }

  async ensureContactConversation(
    cmd: EnsureContactConversationCommand,
  ): Promise<Conversation> {
    this.store.requireAccount(cmd.account_id);
    const address = this.store.normalizeEmail(cmd.normalized_address);
    if (!address.includes('@')) {
      this.store.failValidation('邮箱格式无效');
    }

    const existing = this.store.findContactConversation(cmd.account_id, address);
    if (existing) {
      if (existing.hidden_at != null) {
        existing.hidden_at = null;
        existing.updated_at = Date.now();
        this.store.conversations.set(
          this.store.conversationKey(cmd.account_id, existing.id),
          existing,
        );
      }
      return { ...existing };
    }

    const t = Date.now();
    let contact: Contact | undefined;
    for (const c of this.store.contacts.values()) {
      if (
        c.account_id === cmd.account_id &&
        c.normalized_address === address
      ) {
        contact = c;
        break;
      }
    }
    if (!contact) {
      contact = {
        id: this.store.nextId('ct'),
        account_id: cmd.account_id,
        graph_id: null,
        display_name: cmd.display_name?.trim() || '',
        normalized_address: address,
        created_at: t,
        updated_at: t,
      };
      this.store.contacts.set(
        this.store.contactKey(cmd.account_id, contact.id),
        contact,
      );
    }

    const conversation: Conversation = {
      id: this.store.nextId('conv'),
      account_id: cmd.account_id,
      kind: 'contact',
      normalized_address: address,
      contact_id: contact.id,
      group_id: null,
      last_message_at: null,
      last_message_id: null,
      unread_count: 0,
      hidden_at: null,
      created_at: t,
      updated_at: t,
    };
    this.store.conversations.set(
      this.store.conversationKey(cmd.account_id, conversation.id),
      conversation,
    );
    this.store.conversationMessages.set(
      this.store.conversationKey(cmd.account_id, conversation.id),
      [],
    );
    return { ...conversation };
  }

  async ensureGroupConversation(
    cmd: EnsureGroupConversationCommand,
  ): Promise<Conversation> {
    this.store.requireAccount(cmd.account_id);
    const conversation = this.store.ensureGroupConversationEntity(
      cmd.account_id,
      cmd.group_id,
    );
    if (conversation.hidden_at != null) {
      conversation.hidden_at = null;
      conversation.updated_at = Date.now();
      this.store.conversations.set(
        this.store.conversationKey(cmd.account_id, conversation.id),
        conversation,
      );
    }
    return { ...conversation };
  }

  async hideConversation(
    accountId: AccountId,
    conversationId: ConversationId,
  ): Promise<void> {
    this.store.requireAccount(accountId);
    const key = this.store.conversationKey(accountId, conversationId);
    const conversation = this.store.conversations.get(key);
    if (!conversation) this.store.failNotFound('会话不存在');
    const t = Date.now();
    conversation.hidden_at = t;
    conversation.updated_at = t;
    this.store.conversations.set(key, conversation);
  }
}
