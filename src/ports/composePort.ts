import type {
  ComposeDraft,
  SaveComposeDraftCommand,
  SendComposedMailCommand,
} from '../domain/dto';
import type { AccountId, ConversationId } from '../domain/types';
import type { MessageWithRecipients } from '../domain/entities';

/** 写信 / 草稿入站端口（本阶段 Memory Mock，不接 Graph） */
export interface ComposePort {
  getDraft(
    accountId: AccountId,
    conversationId: ConversationId,
  ): Promise<ComposeDraft | null>;
  saveDraft(cmd: SaveComposeDraftCommand): Promise<ComposeDraft>;
  clearDraft(
    accountId: AccountId,
    conversationId: ConversationId,
  ): Promise<void>;
  /** 发送后返回新建的 outbound 邮件聚合 */
  send(cmd: SendComposedMailCommand): Promise<MessageWithRecipients>;
}
