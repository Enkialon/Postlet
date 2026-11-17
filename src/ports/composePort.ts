import type {
  ComposeAttachmentDraft,
  ComposeDraft,
  ComposeSendResult,
  ImportAttachmentCommand,
  SaveComposeDraftCommand,
  SendComposedMailCommand,
} from '../domain/dto';
import type { AccountId, MessageId, SendState, SessionId } from '../domain/types';

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
  /** 入队发送；返回 queued 状态与本地消息（不等于服务器已接受） */
  send(cmd: SendComposedMailCommand): Promise<ComposeSendResult>;
  /** 查询某封本地发送邮件的队列状态 */
  getSendState(
    accountId: AccountId,
    messageId: MessageId,
  ): Promise<SendState | null>;
}
