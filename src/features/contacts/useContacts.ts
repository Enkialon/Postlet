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

function contactKey(c: Contact): string {
  return `${c.account_id}:${c.id}`;
}

export function useContacts() {
  const ports = inject(portsKey)!;
  const nav = inject(navigationKey)!;

  async function loadInitial(search: string | null): Promise<void> {
    const filter = nav.accountFilter.value;
    const version = ++listState.requestVersion;
    const session = nav.sessionVersion.value;
    listState.loading = 'initial';
    listState.error = null;
    try {
      const page = await ports.contact.listContacts({
        account_id: nav.listAccountId.value,
        cursor: null,
        limit: PAGE_SIZE,
        search,
      });
      if (
        version !== listState.requestVersion ||
        session !== nav.sessionVersion.value ||
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
    const search = searchInput.value.trim() || null;
    listState.loading = 'more';
    try {
      const page = await ports.contact.listContacts({
        account_id: nav.listAccountId.value,
        cursor: listState.nextCursor,
        limit: PAGE_SIZE,
        search,
      });
      if (
        version !== listState.requestVersion ||
        filter !== nav.accountFilter.value
      ) {
        return;
      }
      const byKey = new Map(listState.items.map((c) => [contactKey(c), c]));
      for (const c of page.items) byKey.set(contactKey(c), c);
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

  function setSearch(value: string): void {
    searchInput.value = value;
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      void loadInitial(value.trim() || null);
    }, DEBOUNCE_MS);
  }

  async function selectContact(contact: Contact): Promise<void> {
    selected.value = contact;
    nav.setAccount(contact.account_id);
  }

  async function openConversation(): Promise<void> {
    const contact = selected.value;
    if (!contact || opening.value) return;
    opening.value = true;
    try {
      nav.setAccount(contact.account_id);
      const conversation = await ports.conversation.ensureContactConversation({
        account_id: contact.account_id,
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
      () => [nav.accountFilter.value, nav.sessionVersion.value],
      () => {
        selected.value = null;
        searchInput.value = '';
        Object.assign(listState, createEmptyPageState<Contact>());
        void loadInitial(null);
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
