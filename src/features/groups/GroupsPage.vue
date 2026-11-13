<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { useVirtualizer } from '@tanstack/vue-virtual';
import { useGroups } from './useGroups';
import Avatar from '../../shared/ui/Avatar.vue';
import Button from '../../shared/ui/Button.vue';
import EmptyState from '../../shared/ui/EmptyState.vue';
import ErrorState from '../../shared/ui/ErrorState.vue';
import Dialog from '../../shared/ui/Dialog.vue';
import OverlayScrollArea from '../../shared/ui/OverlayScrollArea.vue';
import type { Group } from '../../domain/entities';

const ROW_SIZE = 60;

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

const scrollAreaRef = ref<InstanceType<typeof OverlayScrollArea> | null>(null);
const scrollViewport = computed(
  () => scrollAreaRef.value?.viewport ?? null,
);

const virtualizer = useVirtualizer(
  computed(() => ({
    count: listState.items.length,
    getScrollElement: () => scrollViewport.value,
    estimateSize: () => ROW_SIZE,
    overscan: 10,
  })),
);

const virtualItems = computed(() => virtualizer.value.getVirtualItems());
const totalSize = computed(() => virtualizer.value.getTotalSize());

watch(virtualItems, (rows) => {
  const last = rows[rows.length - 1];
  if (!last) return;
  if (
    last.index >= listState.items.length - 8 &&
    listState.hasMore &&
    listState.loading === 'idle'
  ) {
    void loadMore();
  }
});

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
  <section
    class="grid h-full min-h-0 grid-cols-[minmax(260px,320px)_1fr] bg-surface-chat max-[760px]:grid-cols-1"
    aria-label="群组"
  >
    <aside class="flex min-h-0 flex-col border-r border-border-subtle bg-surface-sidebar">
      <header
        class="flex items-center justify-between border-b border-border-subtle px-3.5 py-3.5"
      >
        <h2 class="m-0 text-base font-semibold text-text-primary">群组</h2>
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
      <OverlayScrollArea
        v-else
        ref="scrollAreaRef"
        class="m-0 min-h-0 flex-1"
        type="hover"
      >
        <div class="relative w-full" :style="{ height: `${totalSize}px` }">
          <div
            v-for="row in virtualItems"
            :key="String(row.key)"
            class="absolute top-0 left-0 box-border flex w-full cursor-pointer items-center gap-3 px-3.5 py-3 transition-colors duration-150 hover:bg-surface-sidebar-hover"
            :class="
              selected?.id === listState.items[row.index]?.id
                ? 'bg-surface-sidebar-selected'
                : ''
            "
            tabindex="0"
            :style="{
              height: `${row.size}px`,
              transform: `translateY(${row.start}px)`,
            }"
            @click="
              listState.items[row.index] &&
                openGroup(listState.items[row.index]!.id)
            "
            @keydown.enter="
              listState.items[row.index] &&
                openGroup(listState.items[row.index]!.id)
            "
          >
            <template v-if="listState.items[row.index]">
              <Avatar
                :name="listState.items[row.index]!.name || '群'"
                kind="group"
                :size="40"
              />
              <strong
                class="overflow-hidden text-[14px] font-semibold text-ellipsis whitespace-nowrap text-text-primary"
                v-no-i18n
                >{{ listState.items[row.index]!.name || '未命名群组' }}</strong
              >
            </template>
          </div>
        </div>
      </OverlayScrollArea>
      <p
        v-if="listState.loading === 'more'"
        class="m-0 p-2.5 text-[13px] text-text-secondary"
      >
        加载中…
      </p>
    </aside>

    <div class="overflow-auto bg-surface-chat px-6 py-5">
      <EmptyState v-if="!selected" title="选择群组查看详情" />
      <template v-else>
        <h2 class="mb-1 mt-0 text-base font-semibold text-text-primary" v-no-i18n>
          {{ selected.name || '未命名群组' }}
        </h2>
        <p class="text-[13px] text-text-secondary">
          来源：{{ selected.source === 'manual' ? '手动' : '自动' }}
        </p>
        <div class="my-3 flex flex-wrap gap-2">
          <Button variant="primary" @click="openGroupConversation()">
            打开会话
          </Button>
          <Button @click="openRename">重命名</Button>
          <Button @click="openMembers">发信成员</Button>
          <Button @click="openMerge">合并到…</Button>
        </div>
        <p v-if="actionError" class="text-[13px] text-danger">{{ actionError }}</p>

        <h3 class="text-[14px] font-medium text-text-primary">成员</h3>
        <ul class="my-2 grid list-none gap-0 p-0">
          <li
            v-for="p in participants"
            :key="p.normalized_address"
            class="flex justify-between gap-3 border-b border-border-subtle py-2"
          >
            <span class="text-[14px] text-text-primary" v-no-i18n>{{
              p.display_name || p.normalized_address
            }}</span>
            <span class="flex flex-wrap gap-1.5 text-[12px] text-text-tertiary">
              <span v-if="p.is_observed">往来成员</span>
              <span v-if="p.is_send_member">发信成员配置</span>
            </span>
          </li>
        </ul>
        <p class="text-[13px] text-text-secondary">
          发信成员配置为将来发信保留，V1 不提供发送入口。
        </p>
      </template>
    </div>

    <Dialog
      :open="createOpen"
      title="创建群组"
      :confirming="busy"
      @close="createOpen = false"
      @confirm="confirmCreate"
    >
      <label class="mt-2.5 grid gap-1.5 text-[13px] [&_input]:rounded-md [&_input]:border [&_input]:border-border-subtle [&_input]:bg-surface-input [&_input]:px-2.5 [&_input]:py-2 [&_input]:text-[14px] [&_textarea]:rounded-md [&_textarea]:border [&_textarea]:border-border-subtle [&_textarea]:bg-surface-input [&_textarea]:px-2.5 [&_textarea]:py-2 [&_textarea]:text-[14px] [&_select]:rounded-md [&_select]:border [&_select]:border-border-subtle [&_select]:bg-surface-input [&_select]:px-2.5 [&_select]:py-2 [&_select]:text-[14px]">
        名称
        <input v-model="createName" />
      </label>
      <label class="mt-2.5 grid gap-1.5 text-[13px] [&_input]:rounded-md [&_input]:border [&_input]:border-border-subtle [&_input]:bg-surface-input [&_input]:px-2.5 [&_input]:py-2 [&_input]:text-[14px] [&_textarea]:rounded-md [&_textarea]:border [&_textarea]:border-border-subtle [&_textarea]:bg-surface-input [&_textarea]:px-2.5 [&_textarea]:py-2 [&_textarea]:text-[14px] [&_select]:rounded-md [&_select]:border [&_select]:border-border-subtle [&_select]:bg-surface-input [&_select]:px-2.5 [&_select]:py-2 [&_select]:text-[14px]">
        成员地址（可选，逗号或换行分隔）
        <textarea v-model="createMembers" rows="4" />
      </label>
      <p v-if="actionError" class="text-[13px] text-danger">{{ actionError }}</p>
    </Dialog>

    <Dialog
      :open="renameOpen"
      title="重命名群组"
      :confirming="busy"
      @close="renameOpen = false"
      @confirm="confirmRename"
    >
      <label class="mt-2.5 grid gap-1.5 text-[13px] [&_input]:rounded-md [&_input]:border [&_input]:border-border-subtle [&_input]:bg-surface-input [&_input]:px-2.5 [&_input]:py-2 [&_input]:text-[14px] [&_textarea]:rounded-md [&_textarea]:border [&_textarea]:border-border-subtle [&_textarea]:bg-surface-input [&_textarea]:px-2.5 [&_textarea]:py-2 [&_textarea]:text-[14px] [&_select]:rounded-md [&_select]:border [&_select]:border-border-subtle [&_select]:bg-surface-input [&_select]:px-2.5 [&_select]:py-2 [&_select]:text-[14px]">
        名称
        <input v-model="renameName" />
      </label>
      <p v-if="actionError" class="text-[13px] text-danger">{{ actionError }}</p>
    </Dialog>

    <Dialog
      :open="membersOpen"
      title="更新发信成员"
      :confirming="busy"
      @close="membersOpen = false"
      @confirm="confirmMembers"
    >
      <label class="mt-2.5 grid gap-1.5 text-[13px] [&_input]:rounded-md [&_input]:border [&_input]:border-border-subtle [&_input]:bg-surface-input [&_input]:px-2.5 [&_input]:py-2 [&_input]:text-[14px] [&_textarea]:rounded-md [&_textarea]:border [&_textarea]:border-border-subtle [&_textarea]:bg-surface-input [&_textarea]:px-2.5 [&_textarea]:py-2 [&_textarea]:text-[14px] [&_select]:rounded-md [&_select]:border [&_select]:border-border-subtle [&_select]:bg-surface-input [&_select]:px-2.5 [&_select]:py-2 [&_select]:text-[14px]">
        完整发信成员集合
        <textarea v-model="membersText" rows="6" />
      </label>
      <p v-if="actionError" class="text-[13px] text-danger">{{ actionError }}</p>
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
      <label class="mt-2.5 grid gap-1.5 text-[13px] [&_input]:rounded-md [&_input]:border [&_input]:border-border-subtle [&_input]:bg-surface-input [&_input]:px-2.5 [&_input]:py-2 [&_input]:text-[14px] [&_textarea]:rounded-md [&_textarea]:border [&_textarea]:border-border-subtle [&_textarea]:bg-surface-input [&_textarea]:px-2.5 [&_textarea]:py-2 [&_textarea]:text-[14px] [&_select]:rounded-md [&_select]:border [&_select]:border-border-subtle [&_select]:bg-surface-input [&_select]:px-2.5 [&_select]:py-2 [&_select]:text-[14px]">
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
      <p v-if="actionError" class="text-[13px] text-danger">{{ actionError }}</p>
    </Dialog>
  </section>
</template>
