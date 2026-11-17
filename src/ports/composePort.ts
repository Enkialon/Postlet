import type {
  ComposeAttachmentDraft,
  ComposeDraft,
  ImportAttachmentCommand,
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
  /** 导入附件内容并挂到会话草稿；返回稳定 content_id 引用 */
  importAttachment(
    cmd: ImportAttachmentCommand,
  ): Promise<ComposeAttachmentDraft>;
  /** 发送后返回新建的 outbound 邮件聚合 */
  send(cmd: SendComposedMailCommand): Promise<MessageWithRecipients>;
}
