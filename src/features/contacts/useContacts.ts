import { inject, reactive, ref, watch } from 'vue';
import type { Contact } from '../../domain/entities';
import { portsKey, navigationKey } from '../../app/provide';
import {
  createEmptyPageState,
  type PageState,
} from '../../shared/state/pageState';
import { asAppError } from '../../shared/errors/asAppError';

const PAGE_SIZE = 50;
const DEBOUNCE_MS = 250;

const listState = reactive<PageState<Contact>>(createEmptyPageState());
const searchInput = ref('');
const selected = ref<Contact | null>(null);
const opening = ref(false);
let debounceTimer: ReturnType<typeof setTimeout> | null = null;
let watchBound = false;

export function useContacts() {
  const ports = inject(portsKey)!;
  const nav = inject(navigationKey)!;

  async function loadInitial(search: string | null): Promise<void> {
    const accountId = nav.accountId.value;
    if (!accountId) return;
    const version = ++listState.requestVersion;
    const session = nav.sessionVersion.value;
    listState.loading = 'initial';
    listState.error = null;
    try {
      const page = await ports.contact.listContacts({
        account_id: accountId,
        cursor: null,
        limit: PAGE_SIZE,
        search,
      });
      if (
        version !== listState.requestVersion ||
        session !== nav.sessionVersion.value
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
    const accountId = nav.accountId.value;
    if (!accountId || !listState.hasMore || listState.loading !== 'idle') return;
    const version = listState.requestVersion;
    const search = searchInput.value.trim() || null;
    listState.loading = 'more';
    try {
      const page = await ports.contact.listContacts({
        account_id: accountId,
        cursor: listState.nextCursor,
        limit: PAGE_SIZE,
        search,
      });
      if (version !== listState.requestVersion) return;
      const byId = new Map(listState.items.map((c) => [c.id, c]));
      for (const c of page.items) byId.set(c.id, c);
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

  function setSearch(value: string): void {
    searchInput.value = value;
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      void loadInitial(value.trim() || null);
    }, DEBOUNCE_MS);
  }

  async function selectContact(contact: Contact): Promise<void> {
    selected.value = contact;
  }

  async function openConversation(): Promise<void> {
    const accountId = nav.accountId.value;
    const contact = selected.value;
    if (!accountId || !contact || opening.value) return;
    opening.value = true;
    try {
      const conversation = await ports.conversation.ensureContactConversation({
        account_id: accountId,
        normalized_address: contact.normalized_address,
        display_name: contact.display_name || null,
      });
      nav.goPage('conversations');
      nav.selectConversation(conversation.id);
    } catch (e) {
      listState.error = asAppError(e);
    } finally {
      opening.value = false;
    }
  }

  if (!watchBound) {
    watchBound = true;
    watch(
      () => [nav.accountId.value, nav.sessionVersion.value],
      () => {
        selected.value = null;
        searchInput.value = '';
        Object.assign(listState, createEmptyPageState<Contact>());
        if (nav.accountId.value) void loadInitial(null);
      },
    );
  }

  return {
    listState,
    searchInput,
    selected,
    opening,
    setSearch,
    loadInitial,
    loadMore,
    selectContact,
    openConversation,
  };
}
