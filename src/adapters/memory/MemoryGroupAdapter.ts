import type { GroupPort } from '../../ports/groupPort';
import type { Group, GroupParticipant, Session } from '../../domain/entities';
import type {
  CreateManualGroupCommand,
  CursorPage,
  GroupListQuery,
  MergeSessionsCommand,
  ReassignMessagesToGroupCommand,
  RenameGroupCommand,
  UpdateGroupSendMembersCommand,
} from '../../domain/dto';
import type { AccountId, GroupId } from '../../domain/types';
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

function uniqueEmails(store: MemoryStore, addresses: string[]): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const raw of addresses) {
    const email = store.normalizeEmail(raw);
    if (!email) continue;
    if (!email.includes('@')) store.failValidation(`邮箱格式无效: ${raw}`);
    if (seen.has(email)) continue;
    seen.add(email);
    out.push(email);
  }
  return out;
}

export class MemoryGroupAdapter implements GroupPort {
  constructor(private readonly store: MemoryStore) {}

  async listGroups(query: GroupListQuery): Promise<CursorPage<Group>> {
    if (query.account_id) this.store.requireAccount(query.account_id);
    const allListIds = this.store.idsForAllLists();
    const items = [...this.store.groups.values()]
      .filter((g) =>
        query.account_id
          ? g.account_id === query.account_id
          : allListIds.has(g.account_id),
      )
      .sort((a, b) => a.name.localeCompare(b.name));
    return paginate(items, query.cursor, Math.max(1, Math.min(query.limit, 200)));
  }

  async getGroup(accountId: AccountId, groupId: GroupId): Promise<Group> {
    this.store.requireAccount(accountId);
    const group = this.store.groups.get(this.store.groupKey(accountId, groupId));
    if (!group) this.store.failNotFound('群组不存在');
    return { ...group };
  }

  async listParticipants(
    accountId: AccountId,
    groupId: GroupId,
  ): Promise<GroupParticipant[]> {
    this.store.requireAccount(accountId);
    if (!this.store.groups.has(this.store.groupKey(accountId, groupId))) {
      this.store.failNotFound('群组不存在');
    }
    return [
      ...(this.store.participants.get(this.store.groupKey(accountId, groupId)) ??
        []),
    ];
  }

  async createManualGroup(cmd: CreateManualGroupCommand): Promise<Group> {
    this.store.requireAccount(cmd.account_id);
    const name = cmd.name.trim();
    if (!name) this.store.failValidation('群组名称不能为空');
    const emails = uniqueEmails(this.store, cmd.send_member_addresses);
    const t = Date.now();
    const group: Group = {
      id: this.store.nextId('grp'),
      account_id: cmd.account_id,
      name,
      name_is_manual: true,
      source: 'manual',
      send_members_is_manual: true,
      created_at: t,
      updated_at: t,
    };
    this.store.groups.set(this.store.groupKey(cmd.account_id, group.id), group);
    this.store.participants.set(
      this.store.groupKey(cmd.account_id, group.id),
      emails.map((email) => ({
        account_id: cmd.account_id,
        group_id: group.id,
        normalized_address: email,
        display_name: '',
        is_observed: false,
        is_send_member: true,
        first_seen_at: t,
        last_seen_at: t,
      })),
    );
    this.store.ensureGroupSessionEntity(cmd.account_id, group.id);
    return { ...group };
  }

  async renameGroup(cmd: RenameGroupCommand): Promise<Group> {
    this.store.requireAccount(cmd.account_id);
    const group = this.store.groups.get(
      this.store.groupKey(cmd.account_id, cmd.group_id),
    );
    if (!group) this.store.failNotFound('群组不存在');
    const name = cmd.name.trim();
    if (!name) this.store.failValidation('群组名称不能为空');
    group.name = name;
    group.name_is_manual = true;
    group.updated_at = Date.now();
    this.store.groups.set(this.store.groupKey(cmd.account_id, group.id), group);
    return { ...group };
  }

  async mergeSessions(cmd: MergeSessionsCommand): Promise<Session> {
    this.store.requireAccount(cmd.account_id);
    if (cmd.source_session_id === cmd.target_session_id) {
      this.store.failValidation('源会话与目标会话不能相同');
    }
    const source = this.store.findRootSession(
      cmd.account_id,
      cmd.source_session_id,
    );
    const target = this.store.findRootSession(
      cmd.account_id,
      cmd.target_session_id,
    );
    if (source.id === target.id) {
      this.store.failConflict('源与目标已是同一根会话');
    }
    if (source.kind !== 'group' || target.kind !== 'group') {
      this.store.failValidation('仅支持合并群聊会话');
    }

    const t = Date.now();
    for (const msg of this.store.messages.values()) {
      if (
        msg.account_id === cmd.account_id &&
        msg.session_id === source.id
      ) {
        msg.session_id = target.id;
        msg.local_updated_at = t;
        this.store.messages.set(
          this.store.messageKey(cmd.account_id, msg.id),
          msg,
        );
      }
    }

    source.merged_into_id = target.id;
    source.updated_at = t;
    this.store.sessions.set(
      this.store.sessionKey(cmd.account_id, source.id),
      source,
    );
    this.store.recomputeSession(cmd.account_id, target.id);
    return { ...target };
  }

  async reassignMessagesToGroup(
    cmd: ReassignMessagesToGroupCommand,
  ): Promise<void> {
    this.store.requireAccount(cmd.account_id);
    if (cmd.message_ids.length === 0) return;
    const target = this.store.ensureGroupSessionEntity(
      cmd.account_id,
      cmd.target_group_id,
    );
    const t = Date.now();
    const touched = new Set<string>([target.id]);
    for (const messageId of cmd.message_ids) {
      const key = this.store.messageKey(cmd.account_id, messageId);
      const message = this.store.messages.get(key);
      if (!message || message.sync_status === 'deleted_remote') {
        this.store.failNotFound('邮件不存在');
      }
      touched.add(message.session_id);
      message.session_id = target.id;
      message.local_updated_at = t;
      this.store.messages.set(key, message);
    }
    for (const sessionId of touched) {
      this.store.recomputeSession(cmd.account_id, sessionId);
    }
  }

  async updateSendMembers(
    cmd: UpdateGroupSendMembersCommand,
  ): Promise<GroupParticipant[]> {
    this.store.requireAccount(cmd.account_id);
    const group = this.store.groups.get(
      this.store.groupKey(cmd.account_id, cmd.group_id),
    );
    if (!group) this.store.failNotFound('群组不存在');

    const emails = uniqueEmails(this.store, cmd.send_member_addresses);
    const t = Date.now();
    const current =
      this.store.participants.get(
        this.store.groupKey(cmd.account_id, cmd.group_id),
      ) ?? [];
    const byAddress = new Map(
      current.map((p) => [p.normalized_address, { ...p }]),
    );

    for (const p of byAddress.values()) {
      p.is_send_member = false;
    }
    for (const email of emails) {
      const existing = byAddress.get(email);
      if (existing) {
        existing.is_send_member = true;
        existing.last_seen_at = t;
      } else {
        byAddress.set(email, {
          account_id: cmd.account_id,
          group_id: cmd.group_id,
          normalized_address: email,
          display_name: '',
          is_observed: false,
          is_send_member: true,
          first_seen_at: t,
          last_seen_at: t,
        });
      }
    }

    const next = [...byAddress.values()];
    this.store.participants.set(
      this.store.groupKey(cmd.account_id, cmd.group_id),
      next,
    );
    group.send_members_is_manual = cmd.is_manual;
    group.updated_at = t;
    this.store.groups.set(this.store.groupKey(cmd.account_id, group.id), group);
    return next;
  }
}
