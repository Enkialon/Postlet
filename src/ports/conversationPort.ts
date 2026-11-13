import type { Conversation, MessageWithRecipients } from '../domain/entities';
import type {
  ConversationListItem,
  ConversationListQuery,
  ConversationMessagesQuery,
  CursorPage,
  EnsureContactConversationCommand,
} from '../domain/dto';
import type { AccountId, ConversationId } from '../domain/types';

/** 会话入站端口（主导航） */
export interface ConversationPort {
  listConversations(
    query: ConversationListQuery,
  ): Promise<CursorPage<ConversationListItem>>;
  getConversation(
    accountId: AccountId,
    conversationId: ConversationId,
  ): Promise<ConversationListItem>;
  listMessages(
    query: ConversationMessagesQuery,
  ): Promise<CursorPage<MessageWithRecipients>>;
  ensureContactConversation(
    cmd: EnsureContactConversationCommand,
  ): Promise<Conversation>;
}
