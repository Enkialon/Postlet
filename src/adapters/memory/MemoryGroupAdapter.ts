import type { GroupPort } from '../../ports/groupPort';
import type { Group, GroupParticipant } from '../../domain/entities';
import type {
  CreateManualGroupCommand,
  CursorPage,
  GroupListQuery,
  MergeGroupsCommand,
  MoveThreadToGroupCommand,
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

function uniqueEmails(
  store: MemoryStore,
  addresses: string[],
): string[] {
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
    this.store.requireAccount(query.account_id);
    let items = [...this.store.groups.values()].filter(
      (g) => g.account_id === query.account_id,
    );
    if (!query.include_merged) {
      items = items.filter((g) => g.merged_into_id === null);
    }
    items.sort((a, b) => a.name.localeCompare(b.name));
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
      merged_into_id: null,
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
    // 手动建群同时建空群会话，便于群组页直接打开
    this.store.ensureGroupConversationEntity(cmd.account_id, group.id);
    return { ...group };
  }

  async renameGroup(cmd: RenameGroupCommand): Promise<Group> {
    this.store.requireAccount(cmd.account_id);
    const group = this.store.groups.get(
      this.store.groupKey(cmd.account_id, cmd.group_id),
    );
    if (!group) this.store.failNotFound('群组不存在');
    if (group.merged_into_id) this.store.failConflict('不能重命名已合并群组');
    const name = cmd.name.trim();
    if (!name) this.store.failValidation('群组名称不能为空');
    group.name = name;
    group.name_is_manual = true;
    group.updated_at = Date.now();
    this.store.groups.set(this.store.groupKey(cmd.account_id, group.id), group);
    return { ...group };
  }

  async mergeGroups(cmd: MergeGroupsCommand): Promise<Group> {
    this.store.requireAccount(cmd.account_id);
    if (cmd.source_group_id === cmd.target_group_id) {
      this.store.failValidation('源群组与目标群组不能相同');
    }
    const source = this.store.findRootGroup(
      cmd.account_id,
      cmd.source_group_id,
    );
    const target = this.store.findRootGroup(
      cmd.account_id,
      cmd.target_group_id,
    );
    if (source.id === target.id) {
      this.store.failConflict('源与目标已是同一根群组');
    }

    source.merged_into_id = target.id;
    source.updated_at = Date.now();
    this.store.groups.set(this.store.groupKey(cmd.account_id, source.id), source);

    const sourceParts =
      this.store.participants.get(
        this.store.groupKey(cmd.account_id, source.id),
      ) ?? [];
    const targetParts = [
      ...(this.store.participants.get(
        this.store.groupKey(cmd.account_id, target.id),
      ) ?? []),
    ];
    for (const p of sourceParts) {
      if (
        !targetParts.some(
          (t) => t.normalized_address === p.normalized_address,
        )
      ) {
        targetParts.push({ ...p, group_id: target.id });
      }
    }
    this.store.participants.set(
      this.store.groupKey(cmd.account_id, target.id),
      targetParts,
    );

    for (const conv of this.store.conversations.values()) {
      if (
        conv.account_id === cmd.account_id &&
        conv.kind === 'group' &&
        conv.group_id === source.id
      ) {
        conv.group_id = target.id;
        conv.updated_at = Date.now();
        this.store.conversations.set(
          this.store.conversationKey(cmd.account_id, conv.id),
          conv,
        );
      }
    }

    target.updated_at = Date.now();
    this.store.groups.set(this.store.groupKey(cmd.account_id, target.id), target);
    return { ...target };
  }

  async moveThreadToGroup(cmd: MoveThreadToGroupCommand): Promise<void> {
    this.store.requireAccount(cmd.account_id);
    const target = this.store.findRootGroup(
      cmd.account_id,
      cmd.target_group_id,
    );
    const messages = this.store.messagesForThread(
      cmd.account_id,
      cmd.thread_id,
    );
    if (messages.length === 0) this.store.failNotFound('线程不存在');

    let targetConv = [...this.store.conversations.values()].find(
      (c) =>
        c.account_id === cmd.account_id &&
        c.kind === 'group' &&
        c.group_id === target.id,
    );
    if (!targetConv) {
      const t = Date.now();
      targetConv = {
        id: this.store.nextId('conv'),
        account_id: cmd.account_id,
        kind: 'group',
        normalized_address: null,
        contact_id: null,
        group_id: target.id,
        last_message_at: null,
        last_message_id: null,
        unread_count: 0,
        created_at: t,
        updated_at: t,
      };
      this.store.conversations.set(
        this.store.conversationKey(cmd.account_id, targetConv.id),
        targetConv,
      );
      this.store.conversationMessages.set(
        this.store.conversationKey(cmd.account_id, targetConv.id),
        [],
      );
    }

    const messageIds = new Set(messages.map((m) => m.id));
    for (const [convKey, ids] of this.store.conversationMessages) {
      if (!convKey.startsWith(`${cmd.account_id}:`)) continue;
      const next = ids.filter((id) => !messageIds.has(id));
      this.store.conversationMessages.set(convKey, next);
      const conversationId = convKey.slice(cmd.account_id.length + 1);
      this.store.recomputeConversation(cmd.account_id, conversationId);
    }

    const targetKey = this.store.conversationKey(
      cmd.account_id,
      targetConv.id,
    );
    const existing = this.store.conversationMessages.get(targetKey) ?? [];
    const merged = [...new Set([...existing, ...messageIds])];
    this.store.conversationMessages.set(targetKey, merged);
    this.store.recomputeConversation(cmd.account_id, targetConv.id);
  }

  async updateSendMembers(
    cmd: UpdateGroupSendMembersCommand,
  ): Promise<GroupParticipant[]> {
    this.store.requireAccount(cmd.account_id);
    const group = this.store.groups.get(
      this.store.groupKey(cmd.account_id, cmd.group_id),
    );
    if (!group) this.store.failNotFound('群组不存在');
    if (group.merged_into_id) this.store.failConflict('不能编辑已合并群组');

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
