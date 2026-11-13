<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { useGroups } from './useGroups';
import Button from '../../shared/ui/Button.vue';
import EmptyState from '../../shared/ui/EmptyState.vue';
import ErrorState from '../../shared/ui/ErrorState.vue';
import Dialog from '../../shared/ui/Dialog.vue';
import type { Group } from '../../domain/entities';

const {
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
  loadAllRootGroups,
} = useGroups();

const createOpen = ref(false);
const renameOpen = ref(false);
const membersOpen = ref(false);
const mergeOpen = ref(false);

const createName = ref('');
const createMembers = ref('');
const renameName = ref('');
const membersText = ref('');
const mergeTargets = ref<Group[]>([]);
const mergeTargetId = ref('');

const sendMembers = computed(() =>
  participants.value.filter((p) => p.is_send_member),
);

onMounted(() => {
  if (listState.items.length === 0 && listState.loading === 'idle') {
    void loadInitial();
  }
});

function openCreate() {
  createName.value = '';
  createMembers.value = '';
  createOpen.value = true;
}

async function confirmCreate() {
  const members = createMembers.value
    .split(/[,;\n]/)
    .map((s) => s.trim())
    .filter(Boolean);
  const ok = await createGroup(createName.value, members);
  if (ok) createOpen.value = false;
}

function openRename() {
  renameName.value = selected.value?.name ?? '';
  renameOpen.value = true;
}

async function confirmRename() {
  const ok = await renameGroup(renameName.value);
  if (ok) renameOpen.value = false;
}

function openMembers() {
  membersText.value = sendMembers.value
    .map((p) => p.normalized_address)
    .join('\n');
  membersOpen.value = true;
}

async function confirmMembers() {
  const members = membersText.value
    .split(/[,;\n]/)
    .map((s) => s.trim())
    .filter(Boolean);
  const ok = await updateMembers(members, true);
  if (ok) membersOpen.value = false;
}

async function openMerge() {
  if (!selected.value) return;
  mergeTargets.value = (await loadAllRootGroups()).filter(
    (g) => g.id !== selected.value!.id,
  );
  mergeTargetId.value = mergeTargets.value[0]?.id ?? '';
  mergeOpen.value = true;
}

async function confirmMerge() {
  if (!mergeTargetId.value) return;
  const ok = await mergeInto(mergeTargetId.value);
  if (ok) mergeOpen.value = false;
}
</script>

<template>
  <section class="groups" aria-label="群组">
    <aside class="groups__list">
      <header class="groups__toolbar">
        <h2>群组</h2>
        <Button variant="primary" @click="openCreate">创建</Button>
      </header>

      <ErrorState
        v-if="listState.error && listState.items.length === 0"
        :error="listState.error"
        @retry="loadInitial"
      />
      <EmptyState
        v-else-if="listState.loading === 'idle' && listState.items.length === 0"
        title="没有群组"
      />
      <ul v-else class="items">
        <li
          v-for="g in listState.items"
          :key="g.id"
          class="item"
          :class="{ 'item--active': selected?.id === g.id }"
          tabindex="0"
          @click="openGroup(g.id)"
          @keydown.enter="openGroup(g.id)"
        >
          <strong v-no-i18n>{{ g.name || '未命名群组' }}</strong>
        </li>
      </ul>
      <div v-if="listState.hasMore" class="more">
        <Button @click="loadMore">加载更多</Button>
      </div>
    </aside>

    <div class="groups__detail">
      <EmptyState v-if="!selected" title="选择群组查看详情" />
      <template v-else>
        <h2 v-no-i18n>{{ selected.name || '未命名群组' }}</h2>
        <p class="meta">
          来源：{{ selected.source === 'manual' ? '手动' : '自动' }}
        </p>
        <div class="actions">
          <Button variant="primary" @click="openGroupConversation()">
            打开会话
          </Button>
          <Button @click="openRename">重命名</Button>
          <Button @click="openMembers">发信成员</Button>
          <Button @click="openMerge">合并到…</Button>
        </div>
        <p v-if="actionError" class="err">{{ actionError }}</p>

        <h3>成员</h3>
        <ul class="participants">
          <li v-for="p in participants" :key="p.normalized_address">
            <span v-no-i18n>{{
              p.display_name || p.normalized_address
            }}</span>
            <span class="tags">
              <span v-if="p.is_observed">往来成员</span>
              <span v-if="p.is_send_member">发信成员配置</span>
            </span>
          </li>
        </ul>
        <p class="hint">发信成员配置为将来发信保留，V1 不提供发送入口。</p>
      </template>
    </div>

    <Dialog
      :open="createOpen"
      title="创建群组"
      :confirming="busy"
      @close="createOpen = false"
      @confirm="confirmCreate"
    >
      <label class="field">
        名称
        <input v-model="createName" />
      </label>
      <label class="field">
        成员地址（可选，逗号或换行分隔）
        <textarea v-model="createMembers" rows="4" />
      </label>
      <p v-if="actionError" class="err">{{ actionError }}</p>
    </Dialog>

    <Dialog
      :open="renameOpen"
      title="重命名群组"
      :confirming="busy"
      @close="renameOpen = false"
      @confirm="confirmRename"
    >
      <label class="field">
        名称
        <input v-model="renameName" />
      </label>
      <p v-if="actionError" class="err">{{ actionError }}</p>
    </Dialog>

    <Dialog
      :open="membersOpen"
      title="更新发信成员"
      :confirming="busy"
      @close="membersOpen = false"
      @confirm="confirmMembers"
    >
      <label class="field">
        完整发信成员集合
        <textarea v-model="membersText" rows="6" />
      </label>
      <p v-if="actionError" class="err">{{ actionError }}</p>
    </Dialog>

    <Dialog
      :open="mergeOpen"
      title="合并群组"
      danger
      :confirming="busy"
      @close="mergeOpen = false"
      @confirm="confirmMerge"
    >
      <p>
        将源群组「{{ selected?.name }}」并入目标群组。此操作不可在前端撤销。
      </p>
      <label class="field">
        目标根群组
        <select v-model="mergeTargetId">
          <option
            v-for="g in mergeTargets"
            :key="g.id"
            :value="g.id"
            v-no-i18n
          >
            {{ g.name }}
          </option>
        </select>
      </label>
      <p v-if="actionError" class="err">{{ actionError }}</p>
    </Dialog>
  </section>
</template>

<style scoped>
.groups {
  height: 100%;
  display: grid;
  grid-template-columns: minmax(240px, 320px) 1fr;
  min-height: 0;
}
.groups__list {
  border-right: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.groups__toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0.75rem;
  border-bottom: 1px solid var(--border);
}
.groups__toolbar h2 {
  margin: 0;
  font-size: 1rem;
}
.items {
  list-style: none;
  margin: 0;
  padding: 0.35rem;
  overflow: auto;
  flex: 1;
}
.item {
  padding: 0.65rem;
  border-radius: 8px;
  cursor: pointer;
}
.item--active,
.item:hover {
  background: var(--surface-2);
}
.groups__detail {
  padding: 1.25rem;
  overflow: auto;
}
.groups__detail h2 {
  margin: 0 0 0.35rem;
}
.meta,
.hint {
  color: var(--muted);
  font-size: 0.85rem;
}
.actions {
  display: flex;
  flex-wrap: wrap;
  gap: 0.4rem;
  margin: 0.75rem 0;
}
.participants {
  list-style: none;
  margin: 0.5rem 0;
  padding: 0;
  display: grid;
  gap: 0.45rem;
}
.participants li {
  display: flex;
  justify-content: space-between;
  gap: 0.75rem;
  padding: 0.45rem 0;
  border-bottom: 1px solid var(--border);
}
.tags {
  display: flex;
  gap: 0.35rem;
  flex-wrap: wrap;
  font-size: 0.75rem;
  color: var(--muted);
}
.field {
  display: grid;
  gap: 0.35rem;
  margin-top: 0.65rem;
}
.field input,
.field textarea,
.field select {
  font: inherit;
  padding: 0.4rem 0.5rem;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--surface);
  color: var(--text);
}
.err {
  color: var(--danger);
  font-size: 0.85rem;
}
.more {
  padding: 0.65rem;
}

@media (max-width: 760px) {
  .groups {
    grid-template-columns: 1fr;
  }
}
</style>
