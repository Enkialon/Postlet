import type {
  ComposeDraft,
  SaveComposeDraftCommand,
  SendComposedMailCommand,
} from '../domain/dto';
import type { AccountId, SessionId } from '../domain/types';
import type { MessageWithRecipients } from '../domain/entities';

/** 写信 / 草稿入站端口 */
export interface ComposePort {
  getDraft(
    accountId: AccountId,
    sessionId: SessionId,
  ): Promise<ComposeDraft | null>;
  saveDraft(cmd: SaveComposeDraftCommand): Promise<ComposeDraft>;
  clearDraft(accountId: AccountId, sessionId: SessionId): Promise<void>;
  /** 发送后返回新建的 outbound 邮件聚合 */
  send(cmd: SendComposedMailCommand): Promise<MessageWithRecipients>;
}
