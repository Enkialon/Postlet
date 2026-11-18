import type { SessionPort } from '../../ports/sessionPort';
import type { Contact, MessageWithRecipients, Session } from '../../domain/entities';
import type {
  CursorPage,
  EnsureGroupSessionCommand,
  EnsurePrivateSessionCommand,
  ReassignMessagesCommand,
  SessionListItem,
  SessionListQuery,
  SessionMessagesQuery,
} from '../../domain/dto';
import type { AccountId, SessionId } from '../../domain/types';
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

export class MemorySessionAdapter implements SessionPort {
  constructor(private readonly store: MemoryStore) {}

  async listSessions(
    query: SessionListQuery,
  ): Promise<CursorPage<SessionListItem>> {
    if (query.account_id) this.store.requireAccount(query.account_id);
    const allListIds = this.store.idsForAllLists();
    const items = [...this.store.sessions.values()]
      .filter((s) => s.merged_into_id == null)
      .filter((s) =>
        query.account_id
          ? s.account_id === query.account_id
          : allListIds.has(s.account_id),
      )
      .filter((s) => {
        if (s.hidden_at == null) return true;
        const live = this.store.effectiveMessagesForSession(s);
        const lastAt = live[0]?.received_at ?? live[0]?.sent_at ?? null;
        if (lastAt != null && lastAt > s.hidden_at) {
          s.hidden_at = null;
          return true;
        }
        return false;
      })
      .filter((s) => (query.kind ? s.kind === query.kind : true))
      .filter((s) => {
        if (query.include_excluded_folders) return true;
        // 普通范围：至少有一条落在非排除角色的可见消息
        return this.store
          .effectiveMessagesForSession(s)
          .some((m) => {
            const role = this.store.folderRoleOf(m);
            return !['trash', 'junk', 'drafts', 'all'].includes(role ?? '');
          });
      })
      .sort((a, b) => (b.last_message_at ?? 0) - (a.last_message_at ?? 0))
      .map((s) => this.store.toListItem(s));
    return paginate(items, query.cursor, Math.max(1, Math.min(query.limit, 200)));
  }

  async getSession(
    accountId: AccountId,
    sessionId: SessionId,
  ): Promise<SessionListItem> {
    this.store.requireAccount(accountId);
    const session = this.store.sessions.get(
      this.store.sessionKey(accountId, sessionId),
    );
    if (!session || session.merged_into_id) this.store.failNotFound('会话不存在');
    return this.store.toListItem(session);
  }

  async listMessages(
    query: SessionMessagesQuery,
  ): Promise<CursorPage<MessageWithRecipients>> {
    this.store.requireAccount(query.account_id);
    const key = this.store.sessionKey(query.account_id, query.session_id);
    const session = this.store.sessions.get(key);
    if (!session || session.merged_into_id) {
      this.store.failNotFound('会话不存在');
    }
    const messages = this.store.threadMessagesForSession(session);
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

  async ensurePrivateSession(
    cmd: EnsurePrivateSessionCommand,
  ): Promise<Session> {
    this.store.requireAccount(cmd.account_id);
    const address = this.store.normalizeEmail(cmd.normalized_address);
    if (!address.includes('@')) {
      this.store.failValidation('邮箱格式无效');
    }

    const existing = this.store.findPrivateSession(cmd.account_id, address);
    if (existing) {
      if (existing.hidden_at != null) {
        existing.hidden_at = null;
        existing.updated_at = Date.now();
        this.store.sessions.set(
          this.store.sessionKey(cmd.account_id, existing.id),
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
        source: 'observed',
        created_at: t,
        updated_at: t,
      };
      this.store.contacts.set(
        this.store.contactKey(cmd.account_id, contact.id),
        contact,
      );
    }

    const session: Session = {
      id: this.store.nextId('sess'),
      account_id: cmd.account_id,
      kind: 'private',
      normalized_address: address,
      contact_id: contact.id,
      group_id: null,
      merged_into_id: null,
      last_message_at: null,
      last_message_id: null,
      unread_count: 0,
      hidden_at: null,
      created_at: t,
      updated_at: t,
    };
    this.store.sessions.set(
      this.store.sessionKey(cmd.account_id, session.id),
      session,
    );
    return { ...session };
  }

  async ensureGroupSession(cmd: EnsureGroupSessionCommand): Promise<Session> {
    this.store.requireAccount(cmd.account_id);
    const session = this.store.ensureGroupSessionEntity(
      cmd.account_id,
      cmd.group_id,
    );
    if (session.hidden_at != null) {
      session.hidden_at = null;
      session.updated_at = Date.now();
      this.store.sessions.set(
        this.store.sessionKey(cmd.account_id, session.id),
        session,
      );
    }
    return { ...session };
  }

  async hideSession(accountId: AccountId, sessionId: SessionId): Promise<void> {
    this.store.requireAccount(accountId);
    const key = this.store.sessionKey(accountId, sessionId);
    const session = this.store.sessions.get(key);
    if (!session || session.merged_into_id) this.store.failNotFound('会话不存在');
    const t = Date.now();
    session.hidden_at = t;
    session.updated_at = t;
    this.store.sessions.set(key, session);
  }

  async reassignMessages(cmd: ReassignMessagesCommand): Promise<void> {
    this.store.requireAccount(cmd.account_id);
    if (cmd.message_ids.length === 0) return;
    const target = this.store.sessions.get(
      this.store.sessionKey(cmd.account_id, cmd.target_session_id),
    );
    if (!target || target.merged_into_id) {
      this.store.failNotFound('目标会话不存在');
    }
    const t = Date.now();
    const touched = new Set<string>([target.id]);
    for (const messageId of cmd.message_ids) {
      const key = this.store.messageKey(cmd.account_id, messageId);
      const message = this.store.messages.get(key);
      if (!message) {
        this.store.failNotFound('邮件不存在');
      }
      touched.add(message.session_id);
      message.session_id = target.id;
      message.local_updated_at = t;
      this.store.messages.set(key, message);
    }
    if (target.hidden_at != null) {
      target.hidden_at = null;
      target.updated_at = t;
      this.store.sessions.set(
        this.store.sessionKey(cmd.account_id, target.id),
        target,
      );
    }
    for (const sessionId of touched) {
      this.store.recomputeSession(cmd.account_id, sessionId);
    }
  }
}
