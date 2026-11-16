import type {
  Account,
  Contact,
  Group,
  GroupParticipant,
  MailFolderSyncState,
  Message,
  MessageRecipient,
  MessageWithRecipients,
  Session,
} from '../../domain/entities';
import type { MessagePreview, SessionListItem } from '../../domain/dto';
import type {
  AccountId,
  EmailAddress,
  GroupId,
  MessageId,
  SessionId,
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

/** 浏览器回退 / 单测用内存数据仓 */
export class MemoryStore {
  accounts = new Map<AccountId, Account>();
  /** 已登录会话；退出后账户行仍在，列表只认此集合 */
  signedInIds = new Set<AccountId>();
  contacts = new Map<string, Contact>();
  sessions = new Map<string, Session>();
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

  sessionKey(accountId: AccountId, sessionId: string): string {
    return `${accountId}:${sessionId}`;
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

  resolveDisplayName(session: Session): string {
    if (session.kind === 'group') {
      if (session.group_id) {
        const group = this.groups.get(
          this.groupKey(session.account_id, session.group_id),
        );
        if (group?.name.trim()) return group.name.trim();
      }
      return '未命名群组';
    }
    let contactName: string | null = null;
    if (session.contact_id) {
      contactName =
        this.contacts.get(
          this.contactKey(session.account_id, session.contact_id),
        )?.display_name ?? null;
    }
    const name = contactName?.trim();
    if (name) return name;
    return session.normalized_address?.trim() || '未知联系人';
  }

  toListItem(session: Session): SessionListItem {
    const live = this.effectiveMessagesForSession(session);
    const last = live[0] ?? null;
    const snapshot: Session = {
      ...session,
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
      session: snapshot,
      display_name: this.resolveDisplayName(snapshot),
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

  /** 列表预览：仅本 Session 非软删消息 */
  effectiveMessagesForSession(session: Session): Message[] {
    return this.messagesForSession(session.account_id, session.id, false);
  }

  /** 聊天线程列表：含已删，是否展示由前端决定 */
  threadMessagesForSession(session: Session): Message[] {
    return this.messagesForSession(session.account_id, session.id, true);
  }

  private messagesForSession(
    accountId: AccountId,
    sessionId: SessionId,
    includeDeleted: boolean,
  ): Message[] {
    const live: Message[] = [];
    for (const msg of this.messages.values()) {
      if (msg.account_id !== accountId || msg.session_id !== sessionId) continue;
      if (!includeDeleted && msg.sync_status === 'deleted_remote') continue;
      live.push(msg);
    }
    live.sort((a, b) => {
      const ta = a.received_at ?? a.sent_at ?? 0;
      const tb = b.received_at ?? b.sent_at ?? 0;
      return tb - ta;
    });
    return live;
  }

  recomputeSession(accountId: AccountId, sessionId: SessionId): void {
    const key = this.sessionKey(accountId, sessionId);
    const session = this.sessions.get(key);
    if (!session) return;
    if (session.merged_into_id) return;

    const live = this.effectiveMessagesForSession(session);
    session.unread_count = live.filter((m) => !m.is_read).length;
    session.last_message_id = live[0]?.id ?? null;
    session.last_message_at =
      live[0]?.received_at ?? live[0]?.sent_at ?? null;
    session.updated_at = now();
    this.sessions.set(key, session);
  }

  recomputeAllSessions(accountId: AccountId): void {
    for (const session of this.sessions.values()) {
      if (session.account_id === accountId) {
        this.recomputeSession(accountId, session.id);
      }
    }
  }

  findRootSession(accountId: AccountId, sessionId: SessionId): Session {
    let current = this.sessions.get(this.sessionKey(accountId, sessionId));
    if (!current) this.failNotFound('会话不存在');
    const seen = new Set<string>();
    while (current.merged_into_id) {
      if (seen.has(current.id)) this.failConflict('会话合并环');
      seen.add(current.id);
      const next = this.sessions.get(
        this.sessionKey(accountId, current.merged_into_id),
      );
      if (!next) this.failNotFound('合并目标不存在');
      current = next;
    }
    return current;
  }

  findGroupSession(
    accountId: AccountId,
    groupId: GroupId,
  ): Session | undefined {
    for (const s of this.sessions.values()) {
      if (
        s.account_id === accountId &&
        s.kind === 'group' &&
        s.group_id === groupId &&
        s.merged_into_id == null
      ) {
        return s;
      }
    }
    return undefined;
  }

  ensureGroupSessionEntity(
    accountId: AccountId,
    groupId: GroupId,
  ): Session {
    if (!this.groups.has(this.groupKey(accountId, groupId))) {
      this.failNotFound('群组不存在');
    }
    const existing = this.findGroupSession(accountId, groupId);
    if (existing) return existing;

    const t = now();
    const session: Session = {
      id: this.nextId('sess'),
      account_id: accountId,
      kind: 'group',
      normalized_address: null,
      contact_id: null,
      group_id: groupId,
      merged_into_id: null,
      last_message_at: null,
      last_message_id: null,
      unread_count: 0,
      hidden_at: null,
      created_at: t,
      updated_at: t,
    };
    this.sessions.set(this.sessionKey(accountId, session.id), session);
    return session;
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

  findPrivateSession(
    accountId: AccountId,
    address: EmailAddress,
  ): Session | undefined {
    for (const s of this.sessions.values()) {
      if (
        s.account_id === accountId &&
        s.kind === 'private' &&
        s.normalized_address === address &&
        s.merged_into_id == null
      ) {
        return s;
      }
    }
    return undefined;
  }

  nextId(prefix: string): string {
    return `${prefix}-${Math.random().toString(36).slice(2, 10)}`;
  }
}
