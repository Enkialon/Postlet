<script setup lang="ts">
import {
  computed,
  inject,
  onMounted,
  onUnmounted,
  ref,
  watch,
} from 'vue';
import type { AccountFilter } from '../../app/navigation';
import { navigationKey } from '../../app/provide';
import { useAccounts } from '../accounts/useAccounts';
import { useConversations } from '../conversations/useConversations';
import { useMessages } from '../conversations/useMessages';
import { useSyncStatus } from '../sync/useSyncStatus';
import Avatar from '../../shared/ui/Avatar.vue';
import ListPaneResizeHandle from '../../shared/ui/ListPaneResizeHandle.vue';
import { useListPaneWidth } from '../../shared/ui/useListPaneWidth';
import WelcomePage from '../accounts/WelcomePage.vue';
import AddAccountDialog from '../accounts/AddAccountDialog.vue';
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
      if (nav.sessionId.value) void messages.loadInitial();
    }
  },
});

const width = ref(
  typeof window !== 'undefined' ? window.innerWidth : 1200,
);
const accountMenuOpen = ref(false);
const accountMenuRef = ref<HTMLElement | null>(null);
const addAccountOpen = ref(false);

function onResize() {
  width.value = window.innerWidth;
}

function onAccountMenuPointer(e: PointerEvent): void {
  if (!accountMenuOpen.value) return;
  const t = e.target as Node | null;
  if (accountMenuRef.value && t && accountMenuRef.value.contains(t)) return;
  accountMenuOpen.value = false;
}

function onAccountMenuKey(e: KeyboardEvent): void {
  if (!accountMenuOpen.value) return;
  if (e.key === 'Escape') {
    e.preventDefault();
    accountMenuOpen.value = false;
  }
}

watch(accountMenuOpen, (open) => {
  if (open) {
    window.addEventListener('pointerdown', onAccountMenuPointer, true);
    window.addEventListener('keydown', onAccountMenuKey);
  } else {
    window.removeEventListener('pointerdown', onAccountMenuPointer, true);
    window.removeEventListener('keydown', onAccountMenuKey);
  }
});

onMounted(() => {
  window.addEventListener('resize', onResize);
  void accounts.bootstrapAccounts();
});
onUnmounted(() => {
  window.removeEventListener('resize', onResize);
  window.removeEventListener('pointerdown', onAccountMenuPointer, true);
  window.removeEventListener('keydown', onAccountMenuKey);
});

const narrow = computed(() => width.value < 760);

const showList = computed(() => {
  if (nav.page.value !== 'conversations') return false;
  if (!narrow.value) return true;
  return nav.narrowShowList.value || !nav.sessionId.value;
});

const showReader = computed(() => {
  if (nav.page.value !== 'conversations') return false;
  if (!narrow.value) return true;
  return !nav.narrowShowList.value && Boolean(nav.sessionId.value);
});

const listStyle = computed(() =>
  narrow.value ? { width: '100%' } : listPaneStyle.value,
);

const accountLabel = computed(() => {
  if (nav.accountFilter.value === 'all') return '全部账号';
  return (
    accounts.filterAccount.value?.display_name ||
    accounts.filterAccount.value?.email ||
    accounts.current.value?.display_name ||
    accounts.current.value?.email ||
    '账户'
  );
});

function isFilterActive(filter: AccountFilter): boolean {
  return nav.accountFilter.value === filter;
}

function toggleAccountMenu(): void {
  if (!accounts.hasAccounts.value) return;
  accountMenuOpen.value = !accountMenuOpen.value;
}

function pickAccountFilter(filter: AccountFilter): void {
  accountMenuOpen.value = false;
  void accounts.setAccountFilter(filter);
}

function openAddAccount(): void {
  accountMenuOpen.value = false;
  addAccountOpen.value = true;
}

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
      <div ref="accountMenuRef" class="relative mb-3">
        <button
          type="button"
          class="grid cursor-pointer place-items-center rounded-full border-0 bg-transparent p-0 outline-none focus-visible:ring-2 focus-visible:ring-brand/40"
          :aria-expanded="accountMenuOpen"
          aria-haspopup="menu"
          aria-label="切换账号"
          :title="accountLabel"
          @click="toggleAccountMenu"
        >
          <Avatar :name="accountLabel" kind="self" :size="36" />
        </button>
        <div
          v-if="accountMenuOpen"
          class="absolute top-0 left-[calc(100%+10px)] z-50 w-[14.5rem] overflow-hidden rounded-lg border border-border-subtle bg-surface-elevated py-1 shadow-panel"
          role="menu"
          aria-label="账号列表"
        >
          <button
            type="button"
            role="menuitemradio"
            class="flex w-full cursor-pointer items-center gap-2.5 border-0 bg-transparent px-3 py-2 text-left transition-colors duration-150 hover:bg-surface-sidebar-hover"
            :class="
              isFilterActive('all')
                ? 'bg-surface-sidebar-hover'
                : ''
            "
            :aria-checked="isFilterActive('all')"
            @click="pickAccountFilter('all')"
          >
            <Avatar name="全部账号" kind="self" :size="28" />
            <span class="min-w-0 flex-1">
              <span
                class="block text-[13px] font-medium text-text-primary"
                >全部账号</span
              >
              <span class="block text-[11px] text-text-tertiary"
                >所有邮箱混在一起</span
              >
            </span>
            <span
              v-if="isFilterActive('all')"
              class="text-[12px] text-brand"
              aria-hidden="true"
              >✓</span
            >
          </button>
          <div
            class="my-1 border-0 border-t border-border-subtle"
            role="separator"
          />
          <button
            v-for="a in accounts.accounts.value"
            :key="a.id"
            type="button"
            role="menuitemradio"
            class="flex w-full cursor-pointer items-center gap-2.5 border-0 bg-transparent px-3 py-2 text-left transition-colors duration-150 hover:bg-surface-sidebar-hover"
            :class="
              isFilterActive(a.id) ? 'bg-surface-sidebar-hover' : ''
            "
            :aria-checked="isFilterActive(a.id)"
            @click="pickAccountFilter(a.id)"
          >
            <Avatar
              :name="a.display_name || a.email"
              kind="self"
              :size="28"
            />
            <span class="min-w-0 flex-1 overflow-hidden">
              <span
                class="block overflow-hidden text-[13px] font-medium text-ellipsis whitespace-nowrap text-text-primary"
                v-no-i18n
                >{{ a.display_name || a.email }}</span
              >
              <span
                v-if="a.display_name"
                class="block overflow-hidden text-[11px] text-ellipsis whitespace-nowrap text-text-tertiary"
                v-no-i18n
                >{{ a.email }}</span
              >
            </span>
            <span
              v-if="isFilterActive(a.id)"
              class="text-[12px] text-brand"
              aria-hidden="true"
              >✓</span
            >
          </button>
          <div
            class="my-1 border-0 border-t border-border-subtle"
            role="separator"
          />
          <button
            type="button"
            role="menuitem"
            class="flex w-full cursor-pointer items-center gap-2.5 border-0 bg-transparent px-3 py-2 text-left transition-colors duration-150 hover:bg-surface-sidebar-hover"
            @click="openAddAccount"
          >
            <span
              class="grid size-7 shrink-0 place-items-center rounded-full border border-dashed border-border-subtle text-text-secondary"
              aria-hidden="true"
            >
              <svg class="size-3.5" viewBox="0 0 16 16">
                <path
                  fill="currentColor"
                  d="M8 2.5a.75.75 0 0 1 .75.75v4h4a.75.75 0 0 1 0 1.5h-4v4a.75.75 0 0 1-1.5 0v-4h-4a.75.75 0 0 1 0-1.5h4v-4A.75.75 0 0 1 8 2.5Z"
                />
              </svg>
            </span>
            <span class="min-w-0 flex-1 text-[13px] font-medium text-text-primary"
              >添加账号</span
            >
          </button>
        </div>
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
          aria-label="邮件同步"
          title="邮件同步"
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

    <AddAccountDialog
      :open="addAccountOpen"
      @close="addAccountOpen = false"
    />
  </div>
</template>
