<script setup lang="ts">
import { computed, inject, onMounted, onUnmounted, ref } from 'vue';
import { navigationKey } from '../../app/provide';
import { useAccounts } from '../accounts/useAccounts';
import { useConversations } from '../conversations/useConversations';
import { useMessages } from '../conversations/useMessages';
import { useSyncStatus } from '../sync/useSyncStatus';
import Avatar from '../../shared/ui/Avatar.vue';
import ListPaneResizeHandle from '../../shared/ui/ListPaneResizeHandle.vue';
import { useListPaneWidth } from '../../shared/ui/useListPaneWidth';
import WelcomePage from '../accounts/WelcomePage.vue';
import ConversationListPane from '../conversations/ConversationListPane.vue';
import ReadingPane from '../conversations/ReadingPane.vue';
import ContactsPage from '../contacts/ContactsPage.vue';
import GroupsPage from '../groups/GroupsPage.vue';
import SettingsPage from '../settings/SettingsPage.vue';

const nav = inject(navigationKey)!;
const accounts = useAccounts();
const conversations = useConversations();
const messages = useMessages();
const {
  listStyle: listPaneStyle,
  dragging: listDragging,
  startDrag: startListDrag,
} = useListPaneWidth();

useSyncStatus({
  onForegroundRefresh: () => {
    if (nav.page.value === 'conversations') {
      void conversations.refreshSoft();
      if (nav.conversationId.value) void messages.loadInitial();
    }
  },
});

const width = ref(
  typeof window !== 'undefined' ? window.innerWidth : 1200,
);

function onResize() {
  width.value = window.innerWidth;
}

onMounted(() => {
  window.addEventListener('resize', onResize);
  void accounts.bootstrapAccounts();
});
onUnmounted(() => window.removeEventListener('resize', onResize));

const narrow = computed(() => width.value < 760);

const showList = computed(() => {
  if (nav.page.value !== 'conversations') return false;
  if (!narrow.value) return true;
  return nav.narrowShowList.value || !nav.conversationId.value;
});

const showReader = computed(() => {
  if (nav.page.value !== 'conversations') return false;
  if (!narrow.value) return true;
  return !nav.narrowShowList.value && Boolean(nav.conversationId.value);
});

const listStyle = computed(() =>
  narrow.value ? { width: '100%' } : listPaneStyle.value,
);

const accountLabel = computed(
  () =>
    accounts.current.value?.display_name ||
    accounts.current.value?.email ||
    '账户',
);

function railBtn(active: boolean): string {
  return [
    'mx-auto grid size-10 cursor-pointer place-items-center rounded-md border-0 bg-transparent text-icon-rail transition-colors duration-150',
    'hover:bg-surface-rail-hover hover:text-icon-rail-active',
    active ? 'bg-surface-rail-selected text-icon-rail-active' : '',
  ].join(' ');
}
</script>

<template>
  <WelcomePage v-if="nav.page.value === 'welcome'" />

  <div v-else class="flex h-full min-h-0 bg-surface-chat">
    <nav
      class="flex w-14 shrink-0 flex-col items-center gap-1 bg-surface-rail px-1.5 py-3 text-icon-rail min-h-0"
      aria-label="主导航"
    >
      <div class="mb-3 grid justify-items-center gap-2">
        <Avatar :name="accountLabel" kind="self" :size="36" />
        <select
          v-if="accounts.accounts.value.length > 1"
          class="w-11 rounded-sm border-0 bg-surface-rail-hover px-0.5 py-0.5 text-[10px] text-icon-rail"
          :value="accounts.current.value?.id"
          aria-label="账户切换"
          :title="accounts.current.value?.email"
          @change="
            accounts.selectAccount(
              ($event.target as HTMLSelectElement).value,
            )
          "
        >
          <option
            v-for="a in accounts.accounts.value"
            :key="a.id"
            :value="a.id"
            v-no-i18n
          >
            {{ a.display_name || a.email }}
          </option>
        </select>
      </div>

      <ul class="m-0 grid w-full list-none gap-0.5 p-0" aria-label="业务导航">
        <li>
          <button
            type="button"
            :class="railBtn(nav.page.value === 'conversations')"
            aria-label="会话"
            title="会话"
            @click="nav.goPage('conversations')"
          >
            <svg class="block size-5" viewBox="0 0 24 24" aria-hidden="true">
              <path
                fill="currentColor"
                d="M4 4h16a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H9l-4.2 3.15A1 1 0 0 1 3 19.4V6a2 2 0 0 1 1-2Zm1.5 3.25v7.9l1.85-1.4H19.5V7.25Z"
              />
            </svg>
          </button>
        </li>
        <li>
          <button
            type="button"
            :class="railBtn(nav.page.value === 'contacts')"
            aria-label="联系人"
            title="联系人"
            @click="nav.goPage('contacts')"
          >
            <svg class="block size-5" viewBox="0 0 24 24" aria-hidden="true">
              <path
                fill="currentColor"
                d="M12 12a4.25 4.25 0 1 0-4.25-4.25A4.25 4.25 0 0 0 12 12Zm0 1.75c-3.5 0-7 1.7-7 4.25V19.5h14V18c0-2.55-3.5-4.25-7-4.25Z"
              />
            </svg>
          </button>
        </li>
        <li>
          <button
            type="button"
            :class="railBtn(nav.page.value === 'groups')"
            aria-label="群组"
            title="群组"
            @click="nav.goPage('groups')"
          >
            <svg class="block size-5" viewBox="0 0 24 24" aria-hidden="true">
              <path
                fill="currentColor"
                d="M8.25 11a3.25 3.25 0 1 0-3.25-3.25A3.25 3.25 0 0 0 8.25 11Zm7.5 0a3.25 3.25 0 1 0-3.25-3.25A3.25 3.25 0 0 0 15.75 11ZM8.25 12.75c-2.9 0-5.75 1.4-5.75 3.5V18h6.2c.1-.75.45-1.45 1-2a5.4 5.4 0 0 1 1.55-1 7.8 7.8 0 0 0-3-.25Zm7.5 0c-.55 0-1.08.05-1.58.15a5.3 5.3 0 0 1 1.83 1.1c.55.55.9 1.25 1 2h6.25v-1.75c0-2.1-2.85-3.5-5.75-3.5Z"
              />
            </svg>
          </button>
        </li>
      </ul>

      <div class="min-h-2 flex-1" aria-hidden="true" />

      <div class="grid w-full gap-0.5 border-t border-white/10 pt-2">
        <button
          type="button"
          :class="railBtn(nav.page.value === 'settings')"
          aria-label="设置"
          title="设置"
          @click="nav.goPage('settings')"
        >
          <svg class="block size-5" viewBox="0 0 24 24" aria-hidden="true">
            <path
              fill="currentColor"
              d="M10.1 2.6h3.8l.35 2.1a6.9 6.9 0 0 1 1.7.95l2-.9 1.9 3.3-1.65 1.35a6.7 6.7 0 0 1 0 1.9l1.65 1.35-1.9 3.3-2-.9a6.9 6.9 0 0 1-1.7.95l-.35 2.1h-3.8l-.35-2.1a6.9 6.9 0 0 1-1.7-.95l-2 .9-1.9-3.3 1.65-1.35a6.7 6.7 0 0 1 0-1.9L4.15 8.05l1.9-3.3 2 .9a6.9 6.9 0 0 1 1.7-.95ZM12 9.25A2.75 2.75 0 1 0 14.75 12 2.75 2.75 0 0 0 12 9.25Z"
            />
          </svg>
        </button>
      </div>
    </nav>

    <main class="flex min-h-0 min-w-0 flex-1 bg-surface-chat">
      <template v-if="nav.page.value === 'conversations'">
        <div
          v-show="showList"
          class="min-h-0 shrink-0 overflow-hidden border-r border-border-subtle bg-surface-sidebar max-[760px]:w-full! max-[760px]:border-r-0"
          :style="listStyle"
        >
          <ConversationListPane />
        </div>
        <ListPaneResizeHandle
          v-if="showList && showReader && !narrow"
          :dragging="listDragging"
          @start="startListDrag"
        />
        <div
          v-show="showReader"
          class="min-h-0 min-w-0 flex-1 bg-surface-chat max-[760px]:w-full!"
        >
          <ReadingPane />
        </div>
      </template>
      <ContactsPage v-else-if="nav.page.value === 'contacts'" />
      <GroupsPage v-else-if="nav.page.value === 'groups'" />
      <SettingsPage v-else-if="nav.page.value === 'settings'" />
    </main>
  </div>
</template>
