<script setup lang="ts">
import { onMounted } from 'vue';
import { useContacts } from './useContacts';
import { displayNameOrAddress } from '../../shared/formatting/format';
import Button from '../../shared/ui/Button.vue';
import EmptyState from '../../shared/ui/EmptyState.vue';
import ErrorState from '../../shared/ui/ErrorState.vue';

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

onMounted(() => {
  if (listState.items.length === 0 && listState.loading === 'idle') {
    void loadInitial(null);
  }
});
</script>

<template>
  <section class="contacts" aria-label="联系人">
    <aside class="contacts__list">
      <header class="contacts__search">
        <label>
          按姓名或地址查找
          <input
            :value="searchInput"
            type="search"
            placeholder="姓名 / 地址前缀"
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
        v-else-if="
          listState.loading === 'idle' && listState.items.length === 0
        "
        :title="searchInput.trim() ? '没有匹配的联系人' : '没有联系人'"
      />

      <ul v-else class="items" role="listbox">
        <li
          v-for="c in listState.items"
          :key="c.id"
          class="item"
          :class="{ 'item--active': selected?.id === c.id }"
          role="option"
          :aria-selected="selected?.id === c.id"
          tabindex="0"
          @click="selectContact(c)"
          @keydown.enter="selectContact(c)"
        >
          <strong v-no-i18n>{{
            displayNameOrAddress(c.display_name, c.normalized_address)
          }}</strong>
          <span v-no-i18n>{{ c.normalized_address }}</span>
        </li>
      </ul>

      <div v-if="listState.hasMore" class="more">
        <Button :disabled="listState.loading === 'more'" @click="loadMore">
          加载更多
        </Button>
      </div>
    </aside>

    <div class="contacts__detail">
      <EmptyState v-if="!selected" title="选择联系人查看信息" />
      <template v-else>
        <h2 v-no-i18n>
          {{
            displayNameOrAddress(
              selected.display_name,
              selected.normalized_address,
            )
          }}
        </h2>
        <p v-no-i18n class="addr">{{ selected.normalized_address }}</p>
        <p v-if="!selected.graph_id" class="hint">本地占位联系人</p>
        <Button
          variant="primary"
          :disabled="opening"
          @click="openConversation"
        >
          查看往来
        </Button>
      </template>
    </div>
  </section>
</template>

<style scoped>
.contacts {
  height: 100%;
  display: grid;
  grid-template-columns: minmax(240px, 320px) 1fr;
  min-height: 0;
}
.contacts__list {
  border-right: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.contacts__search {
  padding: 0.75rem;
  border-bottom: 1px solid var(--border);
}
.contacts__search label {
  display: grid;
  gap: 0.35rem;
  font-size: 0.85rem;
  color: var(--muted);
}
.contacts__search input {
  font: inherit;
  padding: 0.45rem 0.55rem;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--surface);
  color: var(--text);
}
.items {
  list-style: none;
  margin: 0;
  padding: 0.35rem;
  overflow: auto;
  flex: 1;
}
.item {
  display: grid;
  gap: 0.15rem;
  padding: 0.65rem;
  border-radius: 8px;
  cursor: pointer;
}
.item span {
  color: var(--muted);
  font-size: 0.8rem;
  overflow: hidden;
  text-overflow: ellipsis;
}
.item--active,
.item:hover {
  background: var(--surface-2);
}
.contacts__detail {
  padding: 1.25rem;
}
.contacts__detail h2 {
  margin: 0 0 0.35rem;
}
.addr {
  margin: 0 0 0.75rem;
  color: var(--muted);
}
.hint {
  color: var(--muted);
  font-size: 0.85rem;
}
.more {
  padding: 0.65rem;
}

@media (max-width: 760px) {
  .contacts {
    grid-template-columns: 1fr;
  }
}
</style>
