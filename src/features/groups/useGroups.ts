import { inject, reactive, ref, watch } from 'vue';
import type { Group, GroupParticipant } from '../../domain/entities';
import { portsKey, navigationKey } from '../../app/provide';
import {
  createEmptyPageState,
  type PageState,
} from '../../shared/state/pageState';
import { asAppError } from '../../shared/errors/asAppError';

const PAGE_SIZE = 50;

const listState = reactive<PageState<Group>>(createEmptyPageState());
const selected = ref<Group | null>(null);
const participants = ref<GroupParticipant[]>([]);
const actionError = ref<string | null>(null);
const busy = ref(false);
let watchBound = false;

function groupKey(g: Group): string {
  return `${g.account_id}:${g.id}`;
}

export function useGroups() {
  const ports = inject(portsKey)!;
  const nav = inject(navigationKey)!;

  function contextAccountId(): string | null {
    return selected.value?.account_id ?? nav.accountId.value;
  }

  async function loadInitial(): Promise<void> {
    const filter = nav.accountFilter.value;
    const version = ++listState.requestVersion;
    listState.loading = 'initial';
    listState.error = null;
    try {
      const page = await ports.group.listGroups({
        account_id: nav.listAccountId.value,
        cursor: null,
        limit: PAGE_SIZE,
        include_merged: false,
      });
      if (
        version !== listState.requestVersion ||
        filter !== nav.accountFilter.value
      ) {
        return;
      }
      listState.items = page.items;
      listState.nextCursor = page.next_cursor;
      listState.hasMore = page.has_more;
    } catch (e) {
      if (version !== listState.requestVersion) return;
      listState.error = asAppError(e);
    } finally {
      if (version === listState.requestVersion) listState.loading = 'idle';
    }
  }

  async function loadMore(): Promise<void> {
    if (!listState.hasMore || listState.loading !== 'idle') return;
    const filter = nav.accountFilter.value;
    const version = listState.requestVersion;
    listState.loading = 'more';
    try {
      const page = await ports.group.listGroups({
        account_id: nav.listAccountId.value,
        cursor: listState.nextCursor,
        limit: PAGE_SIZE,
        include_merged: false,
      });
      if (
        version !== listState.requestVersion ||
        filter !== nav.accountFilter.value
      ) {
        return;
      }
      const byKey = new Map(listState.items.map((g) => [groupKey(g), g]));
      for (const g of page.items) byKey.set(groupKey(g), g);
      listState.items = [...byKey.values()];
      listState.nextCursor = page.next_cursor;
      listState.hasMore = page.has_more;
    } catch (e) {
      if (version !== listState.requestVersion) return;
      listState.error = asAppError(e);
    } finally {
      if (version === listState.requestVersion) listState.loading = 'idle';
    }
  }

  async function openGroup(group: Group | string): Promise<void> {
    actionError.value = null;
    try {
      let accountId: string;
      let groupId: string;
      if (typeof group === 'string') {
        accountId = contextAccountId() ?? '';
        groupId = group;
        if (!accountId) return;
      } else {
        accountId = group.account_id;
        groupId = group.id;
      }
      nav.setAccount(accountId);
      selected.value = await ports.group.getGroup(accountId, groupId);
      participants.value = await ports.group.listParticipants(
        accountId,
        groupId,
      );
    } catch (e) {
      actionError.value = asAppError(e).message;
    }
  }

  async function createGroup(
    name: string,
    memberAddresses: string[],
  ): Promise<boolean> {
    const accountId =
      nav.accountFilter.value !== 'all'
        ? nav.accountFilter.value
        : nav.accountId.value;
    if (!accountId || busy.value) {
      if (!accountId) {
        actionError.value = '请先在左上角选择一个具体账号再创建群组';
      }
      return false;
    }
    busy.value = true;
    actionError.value = null;
    try {
      nav.setAccount(accountId);
      const group = await ports.group.createManualGroup({
        account_id: accountId,
        name,
        send_member_addresses: memberAddresses,
      });
      await loadInitial();
      await openGroup(group);
      return true;
    } catch (e) {
      actionError.value = asAppError(e).message;
      return false;
    } finally {
      busy.value = false;
    }
  }

  async function openGroupConversation(groupId?: string): Promise<boolean> {
    const group = selected.value;
    const accountId = group?.account_id ?? nav.accountId.value;
    const id = groupId ?? group?.id;
    if (!accountId || !id || busy.value) return false;
    busy.value = true;
    actionError.value = null;
    try {
      nav.setAccount(accountId);
      const conversation = await ports.conversation.ensureGroupConversation({
        account_id: accountId,
        group_id: id,
      });
      nav.selectConversation(conversation.id);
      return true;
    } catch (e) {
      actionError.value = asAppError(e).message;
      return false;
    } finally {
      busy.value = false;
    }
  }

  async function renameGroup(name: string): Promise<boolean> {
    const group = selected.value;
    if (!group || busy.value) return false;
    busy.value = true;
    actionError.value = null;
    try {
      selected.value = await ports.group.renameGroup({
        account_id: group.account_id,
        group_id: group.id,
        name,
      });
      await loadInitial();
      return true;
    } catch (e) {
      actionError.value = asAppError(e).message;
      return false;
    } finally {
      busy.value = false;
    }
  }

  async function updateMembers(
    addresses: string[],
    isManual: boolean,
  ): Promise<boolean> {
    const group = selected.value;
    if (!group || busy.value) return false;
    busy.value = true;
    actionError.value = null;
    try {
      participants.value = await ports.group.updateSendMembers({
        account_id: group.account_id,
        group_id: group.id,
        send_member_addresses: addresses,
        is_manual: isManual,
      });
      selected.value = await ports.group.getGroup(
        group.account_id,
        group.id,
      );
      return true;
    } catch (e) {
      actionError.value = asAppError(e).message;
      return false;
    } finally {
      busy.value = false;
    }
  }

  async function mergeInto(targetGroupId: string): Promise<boolean> {
    const group = selected.value;
    if (!group || busy.value) return false;
    busy.value = true;
    actionError.value = null;
    try {
      const target = await ports.group.mergeGroups({
        account_id: group.account_id,
        source_group_id: group.id,
        target_group_id: targetGroupId,
      });
      await loadInitial();
      await openGroup(target);
      return true;
    } catch (e) {
      actionError.value = asAppError(e).message;
      await loadInitial();
      return false;
    } finally {
      busy.value = false;
    }
  }

  async function moveThread(
    threadId: string,
    targetGroupId: string,
  ): Promise<boolean> {
    const accountId = selected.value?.account_id ?? nav.accountId.value;
    if (!accountId || busy.value) return false;
    busy.value = true;
    actionError.value = null;
    try {
      await ports.group.moveThreadToGroup({
        account_id: accountId,
        thread_id: threadId,
        target_group_id: targetGroupId,
      });
      if (selected.value) await openGroup(selected.value);
      return true;
    } catch (e) {
      actionError.value = asAppError(e).message;
      await loadInitial();
      return false;
    } finally {
      busy.value = false;
    }
  }

  /** 分页加载全部候选根群组（与当前选中群同账号） */
  async function loadAllRootGroups(): Promise<Group[]> {
    const accountId = selected.value?.account_id ?? nav.accountId.value;
    if (!accountId) return [];
    const all: Group[] = [];
    let cursor: string | null = null;
    let hasMore = true;
    while (hasMore) {
      const page = await ports.group.listGroups({
        account_id: accountId,
        cursor,
        limit: PAGE_SIZE,
        include_merged: false,
      });
      all.push(...page.items);
      hasMore = page.has_more;
      cursor = page.next_cursor;
      if (hasMore && !cursor) break;
    }
    return all;
  }

  if (!watchBound) {
    watchBound = true;
    watch(
      () => [nav.accountFilter.value, nav.sessionVersion.value],
      () => {
        selected.value = null;
        participants.value = [];
        listState.requestVersion += 1;
        listState.items = [];
        listState.nextCursor = null;
        listState.hasMore = false;
        listState.error = null;
        listState.loading = 'idle';
        void loadInitial();
      },
    );
  }

  return {
    listState,
    selected,
    participants,
    actionError,
    busy,
    loadInitial,
    loadMore,
    openGroup,
    openGroupConversation,
    createGroup,
    renameGroup,
    updateMembers,
    mergeInto,
    moveThread,
    loadAllRootGroups,
  };
}
