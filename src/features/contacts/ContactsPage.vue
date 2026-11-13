<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { useVirtualizer } from '@tanstack/vue-virtual';
import { useContacts } from './useContacts';
import { displayNameOrAddress } from '../../shared/formatting/format';
import Avatar from '../../shared/ui/Avatar.vue';
import Button from '../../shared/ui/Button.vue';
import EmptyState from '../../shared/ui/EmptyState.vue';
import ErrorState from '../../shared/ui/ErrorState.vue';
import OverlayScrollArea from '../../shared/ui/OverlayScrollArea.vue';
import ListPaneResizeHandle from '../../shared/ui/ListPaneResizeHandle.vue';
import { useListPaneWidth } from '../../shared/ui/useListPaneWidth';

const ROW_SIZE = 64;
const {
  listStyle,
  dragging: listDragging,
  startDrag: startListDrag,
} = useListPaneWidth();

const {
  listState,
  searchInput,
  selected,
  opening,
  setSearch,
  loadInitial,
  loadMore,
  selectContact,
  openConversation,
} = useContacts();

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

onMounted(() => {
  if (listState.items.length === 0 && listState.loading === 'idle') {
    void loadInitial(null);
  }
});
</script>
<template>
  <section
    class="flex h-full min-h-0 bg-surface-chat max-[760px]:flex-col"
    aria-label="联系人"
  >
    <aside
      class="flex min-h-0 shrink-0 flex-col border-r border-border-subtle bg-surface-sidebar max-[760px]:w-full! max-[760px]:border-r-0"
      :style="listStyle"
    >
      <header
        class="flex h-[52px] shrink-0 items-center border-b border-border-subtle px-3"
      >
        <label
          class="flex h-7 min-w-0 flex-1 items-center rounded-md border border-border-subtle bg-surface-elevated px-2 focus-within:border-brand/35"
        >
          <span class="sr-only">搜索联系人</span>
          <input
            class="min-w-0 flex-1 border-0 bg-transparent text-[12px] leading-none text-text-primary outline-none placeholder:text-text-tertiary"
            :value="searchInput"
            type="search"
            placeholder="搜索"
            autocomplete="off"
            @input="setSearch(($event.target as HTMLInputElement).value)"
          />
        </label>
      </header>

      <ErrorState
        v-if="listState.error && listState.items.length === 0"
        :error="listState.error"
        @retry="() => loadInitial(searchInput.trim() || null)"
      />

      <EmptyState
        v-else-if="listState.loading === 'idle' && listState.items.length === 0"
        :title="searchInput.trim() ? '没有匹配的联系人' : '没有联系人'"
      />

      <OverlayScrollArea
        v-else
        ref="scrollAreaRef"
        class="m-0 min-h-0 flex-1"
        type="hover"
        role="listbox"
      >
        <div class="relative w-full" :style="{ height: `${totalSize}px` }">
          <div
            v-for="row in virtualItems"
            :key="String(row.key)"
            class="absolute top-0 left-0 box-border flex w-full cursor-pointer items-center gap-3 px-3.5 py-3 transition-colors duration-150 hover:bg-surface-sidebar-hover"
            role="option"
            :class="
              selected?.id === listState.items[row.index]?.id
                ? 'bg-surface-sidebar-selected'
                : ''
            "
            :aria-selected="selected?.id === listState.items[row.index]?.id"
            tabindex="0"
            :style="{
              height: `${row.size}px`,
              transform: `translateY(${row.start}px)`,
            }"
            @click="
              listState.items[row.index] &&
                selectContact(listState.items[row.index]!)
            "
            @keydown.enter="
              listState.items[row.index] &&
                selectContact(listState.items[row.index]!)
            "
          >
            <template v-if="listState.items[row.index]">
              <Avatar
                :name="
                  displayNameOrAddress(
                    listState.items[row.index]!.display_name,
                    listState.items[row.index]!.normalized_address,
                  )
                "
                :size="40"
              />
              <div class="grid min-w-0 gap-0.5">
                <strong
                  class="overflow-hidden text-[14px] font-semibold text-ellipsis whitespace-nowrap text-text-primary"
                  v-no-i18n
                  >{{
                    displayNameOrAddress(
                      listState.items[row.index]!.display_name,
                      listState.items[row.index]!.normalized_address,
                    )
                  }}</strong
                >
                <span
                  class="overflow-hidden text-[12px] text-ellipsis whitespace-nowrap text-text-secondary"
                  v-no-i18n
                  >{{ listState.items[row.index]!.normalized_address }}</span
                >
              </div>
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

    <ListPaneResizeHandle
      class="max-[760px]:hidden"
      :dragging="listDragging"
      @start="startListDrag"
    />

    <div class="min-h-0 min-w-0 flex-1 bg-surface-chat p-6">
      <EmptyState v-if="!selected" title="选择联系人查看信息" />
      <template v-else>
        <div class="mb-5 flex items-center gap-4">
          <Avatar
            :name="
              displayNameOrAddress(
                selected.display_name,
                selected.normalized_address,
              )
            "
            :size="64"
          />
          <div>
            <h2 class="mb-1 mt-0 text-base font-semibold text-text-primary" v-no-i18n>
              {{
                displayNameOrAddress(
                  selected.display_name,
                  selected.normalized_address,
                )
              }}
            </h2>
            <p class="m-0 text-[13px] text-text-secondary" v-no-i18n>
              {{ selected.normalized_address }}
            </p>
            <p
              v-if="!selected.graph_id"
              class="mt-1 mb-0 text-[13px] text-text-tertiary"
            >
              本地占位联系人
            </p>
          </div>
        </div>
        <Button variant="primary" :disabled="opening" @click="openConversation">
          发消息
        </Button>
      </template>
    </div>
  </section>
</template>
