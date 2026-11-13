<script setup lang="ts">
import { computed, inject, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import { navigationKey, portsKey } from '../../app/provide';
import type { ComposeRecipientRef } from '../../domain/dto';
import type { Contact } from '../../domain/entities';

const props = defineProps<{
  modelValue: ComposeRecipientRef[];
  placeholder?: string;
}>();

const emit = defineEmits<{
  'update:modelValue': [value: ComposeRecipientRef[]];
  change: [];
}>();

const ports = inject(portsKey)!;
const nav = inject(navigationKey)!;

const input = ref('');
const inputEl = ref<HTMLInputElement | null>(null);
const open = ref(false);
const highlight = ref(0);
const suggestions = ref<Contact[]>([]);
const searching = ref(false);

let searchTimer: ReturnType<typeof setTimeout> | null = null;

const selectedAddresses = computed(
  () => new Set(props.modelValue.map((r) => r.address.toLowerCase())),
);

const filteredSuggestions = computed(() =>
  suggestions.value.filter(
    (c) => !selectedAddresses.value.has(c.normalized_address.toLowerCase()),
  ),
);

function normalizeEmail(raw: string): string {
  return raw.trim().toLowerCase();
}

function parseOne(raw: string): ComposeRecipientRef | null {
  const text = raw.trim().replace(/[;；,，]+$/g, '').trim();
  if (!text) return null;

  // "Name" <email@x.com> 或 Name <email@x.com>
  const angled = text.match(/^(?:"?([^"<]*)"?\s*)?<([^>]+)>$/);
  if (angled) {
    const address = normalizeEmail(angled[2] ?? '');
    if (!address.includes('@')) return null;
    const name = (angled[1] ?? '').trim() || address;
    return { address, display_name: name };
  }

  const address = normalizeEmail(text);
  if (!address.includes('@')) return null;
  return { address, display_name: address };
}

function chipLabel(r: ComposeRecipientRef): string {
  if (r.display_name && r.display_name !== r.address) {
    return `"${r.display_name}" <${r.address}>`;
  }
  return r.address;
}

function commitValue(list: ComposeRecipientRef[]): void {
  emit('update:modelValue', list);
  emit('change');
}

function addRecipient(ref: ComposeRecipientRef): void {
  const addr = normalizeEmail(ref.address);
  if (!addr.includes('@')) return;
  if (selectedAddresses.value.has(addr)) {
    input.value = '';
    return;
  }
  commitValue([
    ...props.modelValue,
    {
      address: addr,
      display_name: ref.display_name.trim() || addr,
    },
  ]);
  input.value = '';
  open.value = false;
  suggestions.value = [];
  void nextTick(() => inputEl.value?.focus());
}

function removeAt(index: number): void {
  const next = props.modelValue.filter((_, i) => i !== index);
  commitValue(next);
  void nextTick(() => inputEl.value?.focus());
}

function tryCommitInput(): boolean {
  const parsed = parseOne(input.value);
  if (!parsed) return false;
  addRecipient(parsed);
  return true;
}

async function runSearch(q: string): Promise<void> {
  const accountId = nav.accountId.value;
  if (!accountId) {
    suggestions.value = [];
    return;
  }
  searching.value = true;
  try {
    const page = await ports.contact.listContacts({
      account_id: accountId,
      cursor: null,
      limit: 12,
      search: q.trim() || null,
    });
    suggestions.value = page.items;
    open.value = true;
    highlight.value = 0;
  } catch {
    suggestions.value = [];
  } finally {
    searching.value = false;
  }
}

function scheduleSearch(): void {
  if (searchTimer) clearTimeout(searchTimer);
  searchTimer = setTimeout(() => {
    void runSearch(input.value);
  }, 180);
}

function onFocus(): void {
  void runSearch(input.value);
}

function onBlur(): void {
  // 延迟关闭，便于点击下拉项
  setTimeout(() => {
    open.value = false;
    if (input.value.trim()) tryCommitInput();
  }, 150);
}

function onKeydown(e: KeyboardEvent): void {
  if (e.key === 'Enter' || e.key === ';' || e.key === ',' || e.key === '；' || e.key === '，') {
    if (open.value && filteredSuggestions.value[highlight.value] && e.key === 'Enter') {
      e.preventDefault();
      const c = filteredSuggestions.value[highlight.value]!;
      addRecipient({
        address: c.normalized_address,
        display_name: c.display_name || c.normalized_address,
      });
      return;
    }
    if (input.value.trim()) {
      e.preventDefault();
      tryCommitInput();
    }
    return;
  }

  if (e.key === 'Backspace' && !input.value && props.modelValue.length) {
    e.preventDefault();
    removeAt(props.modelValue.length - 1);
    return;
  }

  if (e.key === 'ArrowDown' && open.value && filteredSuggestions.value.length) {
    e.preventDefault();
    highlight.value = (highlight.value + 1) % filteredSuggestions.value.length;
    return;
  }
  if (e.key === 'ArrowUp' && open.value && filteredSuggestions.value.length) {
    e.preventDefault();
    highlight.value =
      (highlight.value - 1 + filteredSuggestions.value.length) %
      filteredSuggestions.value.length;
    return;
  }
  if (e.key === 'Escape') {
    open.value = false;
  }
}

function pickSuggestion(c: Contact): void {
  addRecipient({
    address: c.normalized_address,
    display_name: c.display_name || c.normalized_address,
  });
}

watch(input, () => scheduleSearch());

onBeforeUnmount(() => {
  if (searchTimer) clearTimeout(searchTimer);
});

defineExpose({
  focus: () => inputEl.value?.focus(),
});
</script>

<template>
  <div class="relative min-w-0 flex-1">
    <div
      class="flex min-h-[34px] flex-wrap items-center gap-1.5 rounded-md border border-border-subtle bg-surface-input px-2 py-1 focus-within:border-brand"
      @click="inputEl?.focus()"
    >
      <span
        v-for="(r, i) in modelValue"
        :key="`${r.address}-${i}`"
        class="inline-flex max-w-full items-center gap-1 rounded-sm bg-surface-muted px-1.5 py-0.5 text-[12px] text-text-primary"
      >
        <span class="truncate" v-no-i18n>{{ chipLabel(r) }}</span>
        <button
          type="button"
          class="shrink-0 border-0 bg-transparent p-0 text-text-tertiary hover:text-danger"
          :aria-label="`移除 ${r.address}`"
          @click.stop="removeAt(i)"
        >
          ×
        </button>
      </span>
      <input
        ref="inputEl"
        v-model="input"
        class="min-w-[8rem] flex-1 border-0 bg-transparent py-0.5 text-[13px] text-text-primary outline-none"
        :placeholder="modelValue.length ? '' : placeholder"
        @focus="onFocus"
        @blur="onBlur"
        @keydown="onKeydown"
      />
      <slot name="trailing" />
    </div>

    <ul
      v-if="open && filteredSuggestions.length"
      class="absolute top-[calc(100%+4px)] left-0 z-20 m-0 max-h-56 w-full list-none overflow-auto rounded-md border border-border-subtle bg-surface-elevated p-1 shadow-panel"
      role="listbox"
    >
      <li
        v-for="(c, idx) in filteredSuggestions"
        :key="c.id"
        class="cursor-pointer rounded-sm px-2.5 py-1.5 text-[13px]"
        :class="
          idx === highlight
            ? 'bg-surface-sidebar-selected'
            : 'hover:bg-surface-muted'
        "
        role="option"
        @mousedown.prevent="pickSuggestion(c)"
      >
        <div class="truncate font-medium text-text-primary" v-no-i18n>
          {{ c.display_name || c.normalized_address }}
        </div>
        <div class="truncate text-[12px] text-text-tertiary" v-no-i18n>
          {{ c.normalized_address }}
        </div>
      </li>
    </ul>
  </div>
</template>
