import type { MessageWithRecipients } from '../domain/entities';
import type {
  DeleteMessagesCommand,
  MarkMessagesReadCommand,
} from '../domain/dto';
import type { AccountId, MessageId } from '../domain/types';

/** 邮件入站端口 */
export interface MessagePort {
  getMessage(
    accountId: AccountId,
    messageId: MessageId,
  ): Promise<MessageWithRecipients>;
  /** 按需拉取正文 */
  ensureMessageBody(
    accountId: AccountId,
    messageId: MessageId,
  ): Promise<MessageWithRecipients>;
  markRead(cmd: MarkMessagesReadCommand): Promise<void>;
  /** 删除邮件（本地软删 + 推远端） */
  deleteMessages(cmd: DeleteMessagesCommand): Promise<void>;
}
