import type { MessageWithRecipients, Session } from '../domain/entities';
import type {
  CursorPage,
  EnsureGroupSessionCommand,
  EnsurePrivateSessionCommand,
  ReassignMessagesCommand,
  SessionListItem,
  SessionListQuery,
  SessionMessagesQuery,
} from '../domain/dto';
import type { AccountId, SessionId } from '../domain/types';

/** 会话入站端口（主导航） */
export interface SessionPort {
  listSessions(query: SessionListQuery): Promise<CursorPage<SessionListItem>>;
  getSession(
    accountId: AccountId,
    sessionId: SessionId,
  ): Promise<SessionListItem>;
  listMessages(
    query: SessionMessagesQuery,
  ): Promise<CursorPage<MessageWithRecipients>>;
  /** 尚无远端位置的本地发信记录（发信记录区；含已发送，不混入普通列表） */
  listPendingSends(
    accountId: AccountId,
    sessionId: SessionId,
  ): Promise<MessageWithRecipients[]>;
  ensurePrivateSession(cmd: EnsurePrivateSessionCommand): Promise<Session>;
  ensureGroupSession(cmd: EnsureGroupSessionCommand): Promise<Session>;
  /** 本地隐藏会话（不删邮件）；有更新邮件后会再出现 */
  hideSession(accountId: AccountId, sessionId: SessionId): Promise<void>;
  /** 将选中消息改挂到目标会话 */
  reassignMessages(cmd: ReassignMessagesCommand): Promise<void>;
}
