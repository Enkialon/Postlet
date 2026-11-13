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

export function useGroups() {
  const ports = inject(portsKey)!;
  const nav = inject(navigationKey)!;

  async function loadInitial(): Promise<void> {
    const accountId = nav.accountId.value;
    if (!accountId) return;
    const version = ++listState.requestVersion;
    listState.loading = 'initial';
    listState.error = null;
    try {
      const page = await ports.group.listGroups({
        account_id: accountId,
        cursor: null,
        limit: PAGE_SIZE,
        include_merged: false,
      });
      if (version !== listState.requestVersion) return;
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
    const accountId = nav.accountId.value;
    if (!accountId || !listState.hasMore || listState.loading !== 'idle') return;
    const version = listState.requestVersion;
    listState.loading = 'more';
    try {
      const page = await ports.group.listGroups({
        account_id: accountId,
        cursor: listState.nextCursor,
        limit: PAGE_SIZE,
        include_merged: false,
      });
      if (version !== listState.requestVersion) return;
      const byId = new Map(listState.items.map((g) => [g.id, g]));
      for (const g of page.items) byId.set(g.id, g);
      listState.items = [...byId.values()];
      listState.nextCursor = page.next_cursor;
      listState.hasMore = page.has_more;
    } catch (e) {
      if (version !== listState.requestVersion) return;
      listState.error = asAppError(e);
    } finally {
      if (version === listState.requestVersion) listState.loading = 'idle';
    }
  }

  async function openGroup(groupId: string): Promise<void> {
    const accountId = nav.accountId.value;
    if (!accountId) return;
    actionError.value = null;
    try {
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
    const accountId = nav.accountId.value;
    if (!accountId || busy.value) return false;
    busy.value = true;
    actionError.value = null;
    try {
      const group = await ports.group.createManualGroup({
        account_id: accountId,
        name,
        send_member_addresses: memberAddresses,
      });
      await loadInitial();
      await openGroup(group.id);
      return true;
    } catch (e) {
      actionError.value = asAppError(e).message;
      return false;
    } finally {
      busy.value = false;
    }
  }

  async function renameGroup(name: string): Promise<boolean> {
    const accountId = nav.accountId.value;
    const group = selected.value;
    if (!accountId || !group || busy.value) return false;
    busy.value = true;
    actionError.value = null;
    try {
      selected.value = await ports.group.renameGroup({
        account_id: accountId,
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
    const accountId = nav.accountId.value;
    const group = selected.value;
    if (!accountId || !group || busy.value) return false;
    busy.value = true;
    actionError.value = null;
    try {
      participants.value = await ports.group.updateSendMembers({
        account_id: accountId,
        group_id: group.id,
        send_member_addresses: addresses,
        is_manual: isManual,
      });
      selected.value = await ports.group.getGroup(accountId, group.id);
      return true;
    } catch (e) {
      actionError.value = asAppError(e).message;
      return false;
    } finally {
      busy.value = false;
    }
  }

  async function mergeInto(targetGroupId: string): Promise<boolean> {
    const accountId = nav.accountId.value;
    const group = selected.value;
    if (!accountId || !group || busy.value) return false;
    busy.value = true;
    actionError.value = null;
    try {
      const target = await ports.group.mergeGroups({
        account_id: accountId,
        source_group_id: group.id,
        target_group_id: targetGroupId,
      });
      await loadInitial();
      await openGroup(target.id);
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
    const accountId = nav.accountId.value;
    if (!accountId || busy.value) return false;
    busy.value = true;
    actionError.value = null;
    try {
      await ports.group.moveThreadToGroup({
        account_id: accountId,
        thread_id: threadId,
        target_group_id: targetGroupId,
      });
      if (selected.value) await openGroup(selected.value.id);
      return true;
    } catch (e) {
      actionError.value = asAppError(e).message;
      await loadInitial();
      return false;
    } finally {
      busy.value = false;
    }
  }

  /** 分页加载全部候选根群组，避免只拿当前页 */
  async function loadAllRootGroups(): Promise<Group[]> {
    const accountId = nav.accountId.value;
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
      () => [nav.accountId.value, nav.sessionVersion.value],
      () => {
        selected.value = null;
        participants.value = [];
        Object.assign(listState, createEmptyPageState<Group>());
        if (nav.accountId.value) void loadInitial();
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
    createGroup,
    renameGroup,
    updateMembers,
    mergeInto,
    moveThread,
    loadAllRootGroups,
  };
}
