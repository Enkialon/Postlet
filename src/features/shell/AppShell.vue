<script setup lang="ts">
import { computed, inject, onMounted, onUnmounted, ref } from 'vue';
import { navigationKey } from '../../app/provide';
import { usePreferences } from '../../app/preferences';
import { useAccounts } from '../accounts/useAccounts';
import { useConversations } from '../conversations/useConversations';
import { useSyncStatus } from '../sync/useSyncStatus';
import WelcomePage from '../accounts/WelcomePage.vue';
import ConversationListPane from '../conversations/ConversationListPane.vue';
import ReadingPane from '../conversations/ReadingPane.vue';
import ContactsPage from '../contacts/ContactsPage.vue';
import GroupsPage from '../groups/GroupsPage.vue';
import SettingsPage from '../settings/SettingsPage.vue';

const nav = inject(navigationKey)!;
const prefs = usePreferences();
const accounts = useAccounts();
const conversations = useConversations();

useSyncStatus({
  onForegroundRefresh: () => {
    if (nav.page.value === 'conversations') {
      void conversations.refreshSoft();
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

const compactNav = computed(() => width.value < 1000);
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

const navStyle = computed(() => ({
  width: compactNav.value ? '64px' : `${prefs.preferences.panelWidths.nav}px`,
}));

const listStyle = computed(() => ({
  width: narrow.value ? '100%' : `${prefs.preferences.panelWidths.list}px`,
}));
</script>

<template>
  <WelcomePage v-if="nav.page.value === 'welcome'" />

  <div v-else class="shell">
    <nav class="nav" :class="{ 'nav--compact': compactNav }" :style="navStyle">
      <div class="nav__account">
        <label v-if="!compactNav" class="nav__label">账户</label>
        <select
          v-if="accounts.accounts.value.length"
          :value="accounts.current.value?.id"
          :aria-label="compactNav ? '账户切换' : undefined"
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

      <ul class="nav__links">
        <li>
          <button
            type="button"
            :class="{ active: nav.page.value === 'conversations' }"
            :aria-label="'会话'"
            :title="'会话'"
            @click="nav.goPage('conversations')"
          >
            <span v-if="compactNav" aria-hidden="true">会话</span>
            <span v-else>会话</span>
          </button>
        </li>
        <li>
          <button
            type="button"
            :class="{ active: nav.page.value === 'contacts' }"
            aria-label="联系人"
            title="联系人"
            @click="nav.goPage('contacts')"
          >
            联系人
          </button>
        </li>
        <li>
          <button
            type="button"
            :class="{ active: nav.page.value === 'groups' }"
            aria-label="群组"
            title="群组"
            @click="nav.goPage('groups')"
          >
            群组
          </button>
        </li>
        <li>
          <button
            type="button"
            :class="{ active: nav.page.value === 'settings' }"
            aria-label="设置"
            title="设置"
            @click="nav.goPage('settings')"
          >
            设置
          </button>
        </li>
      </ul>

      <p class="nav__sync" :title="accounts.current.value?.mail_sync_status">
        <template v-if="!compactNav">
          {{
            accounts.current.value?.mail_sync_status === 'running'
              ? '正在同步'
              : '同步状态'
          }}
        </template>
        <span v-else aria-label="同步状态">↻</span>
      </p>
    </nav>

    <main class="main">
      <template v-if="nav.page.value === 'conversations'">
        <div
          v-show="showList"
          class="pane pane--list"
          :style="listStyle"
        >
          <ConversationListPane />
        </div>
        <div v-show="showReader" class="pane pane--reader">
          <ReadingPane />
        </div>
      </template>
      <ContactsPage v-else-if="nav.page.value === 'contacts'" />
      <GroupsPage v-else-if="nav.page.value === 'groups'" />
      <SettingsPage v-else-if="nav.page.value === 'settings'" />
    </main>
  </div>
</template>

<style scoped>
.shell {
  height: 100%;
  display: flex;
  min-height: 0;
  background: var(--bg);
}
.nav {
  flex: none;
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  padding: 0.85rem 0.65rem;
  border-right: 1px solid var(--border);
  background: var(--surface);
  min-height: 0;
}
.nav--compact {
  align-items: center;
}
.nav__label {
  display: block;
  font-size: 0.75rem;
  color: var(--muted);
  margin-bottom: 0.25rem;
}
.nav__account select {
  width: 100%;
  font: inherit;
  font-size: 0.85rem;
  padding: 0.35rem;
  border-radius: 6px;
  border: 1px solid var(--border);
  background: var(--surface-2);
  color: var(--text);
}
.nav__links {
  list-style: none;
  margin: 0;
  padding: 0;
  display: grid;
  gap: 0.25rem;
  flex: 1;
}
.nav__links button {
  width: 100%;
  text-align: left;
  border: 0;
  background: transparent;
  padding: 0.55rem 0.65rem;
  border-radius: 8px;
  cursor: pointer;
  color: var(--text);
  font: inherit;
}
.nav--compact .nav__links button {
  text-align: center;
  padding: 0.55rem 0.25rem;
  font-size: 0.75rem;
}
.nav__links button.active,
.nav__links button:hover {
  background: var(--surface-2);
}
.nav__sync {
  margin: 0;
  font-size: 0.75rem;
  color: var(--muted);
  padding: 0.35rem;
}
.main {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
}
.pane--list {
  flex: none;
  border-right: 1px solid var(--border);
  background: var(--surface);
  min-height: 0;
  overflow: hidden;
}
.pane--reader {
  flex: 1;
  min-width: 0;
  min-height: 0;
  background: var(--bg);
}

@media (max-width: 760px) {
  .pane--list,
  .pane--reader {
    width: 100% !important;
    border-right: 0;
  }
}
</style>
