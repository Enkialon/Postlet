<script setup lang="ts">
import { computed, inject, onMounted, onUnmounted, ref, watch } from 'vue';
import { portsKey } from '../../app/provide';
import type { MailFolderSyncState } from '../../domain/entities';
import type { AccountId, FolderId } from '../../domain/types';
import { asAppError } from '../../shared/errors/asAppError';
import {
  buildFolderTree,
  flattenFolderTree,
} from '../settings/folderTree';
import Button from '../../shared/ui/Button.vue';
import OverlayScrollArea from '../../shared/ui/OverlayScrollArea.vue';

const props = defineProps<{
  open: boolean;
  accountId: AccountId | null;
  accountLabel?: string;
}>();

const emit = defineEmits<{
  close: [];
  done: [];
}>();

const ports = inject(portsKey)!;

const panelRef = ref<HTMLElement | null>(null);
const loading = ref(false);
const saving = ref(false);
const error = ref<string | null>(null);
const folders = ref<MailFolderSyncState[]>([]);
/** 本地勾选（确认前不写库） */
const selected = ref<Set<FolderId>>(new Set());

let previousFocus: HTMLElement | null = null;

const rows = computed(() =>
  flattenFolderTree(buildFolderTree(folders.value)),
);

const selectedCount = computed(() => selected.value.size);

const title = computed(() =>
  props.accountLabel
    ? `选择要同步的文件夹 · ${props.accountLabel}`
    : '选择要同步的文件夹',
);

function isChecked(folderId: FolderId): boolean {
  return selected.value.has(folderId);
}

function toggle(folderId: FolderId, checked: boolean): void {
  const next = new Set(selected.value);
  if (checked) next.add(folderId);
  else next.delete(folderId);
  selected.value = next;
}

function selectAll(): void {
  selected.value = new Set(folders.value.map((f) => f.folder_id));
}

function selectNone(): void {
  selected.value = new Set();
}

async function loadFolders(): Promise<void> {
  const accountId = props.accountId;
  if (!accountId || !props.open) return;
  loading.value = true;
  error.value = null;
  try {
    const list = await ports.sync.refreshMailFolders(accountId);
    folders.value = list;
    selected.value = new Set(
      list.filter((f) => f.sync_enabled).map((f) => f.folder_id),
    );
    if (list.length === 0) {
      error.value = '未发现任何文件夹，请稍后在设置中重试';
    }
  } catch (e) {
    folders.value = [];
    selected.value = new Set();
    error.value = asAppError(e).message;
  } finally {
    loading.value = false;
  }
}

async function confirm(): Promise<void> {
  const accountId = props.accountId;
  if (!accountId || saving.value) return;
  saving.value = true;
  error.value = null;
  try {
    for (const f of folders.value) {
      const want = selected.value.has(f.folder_id);
      if (want === f.sync_enabled) continue;
      await ports.sync.setFolderSyncEnabled({
        account_id: accountId,
        folder_id: f.folder_id,
        sync_enabled: want,
      });
    }
    void ports.sync.triggerMailSync({
      account_id: accountId,
      folder_ids: null,
    });
    emit('done');
    emit('close');
  } catch (e) {
    error.value = asAppError(e).message;
  } finally {
    saving.value = false;
  }
}

function onKey(e: KeyboardEvent): void {
  if (!props.open || saving.value) return;
  if (e.key === 'Escape') {
    e.preventDefault();
    emit('close');
  }
}

watch(
  () => [props.open, props.accountId] as const,
  ([open]) => {
    if (open) {
      previousFocus = document.activeElement as HTMLElement | null;
      requestAnimationFrame(() => panelRef.value?.focus());
      void loadFolders();
    } else if (previousFocus) {
      previousFocus.focus();
      previousFocus = null;
    }
  },
);

onMounted(() => window.addEventListener('keydown', onKey));
onUnmounted(() => window.removeEventListener('keydown', onKey));
</script>

<template>
  <div
    v-if="open"
    class="fixed inset-0 z-50 grid place-items-center"
    role="presentation"
  >
    <div class="absolute inset-0 bg-black/30" @click="!saving && emit('close')" />
    <div
      ref="panelRef"
      class="relative flex max-h-[min(640px,calc(100vh-2rem))] w-[min(520px,calc(100vw-2rem))] flex-col rounded-lg border border-border-subtle bg-surface-elevated px-4 py-4 shadow-panel"
      role="dialog"
      aria-modal="true"
      :aria-label="title"
      tabindex="-1"
    >
      <header class="shrink-0">
        <h2 class="m-0 text-base font-semibold text-text-primary" v-no-i18n>
          {{ title }}
        </h2>
        <p class="mt-1.5 mb-0 text-[13px] leading-normal text-text-secondary">
          已拉取邮箱全部文件夹。勾选后才会同步邮件；可稍后在设置中修改。
        </p>
      </header>

      <div class="mt-3 mb-2 flex shrink-0 flex-wrap items-center gap-2">
        <Button
          variant="ghost"
          :disabled="loading || saving || folders.length === 0"
          @click="selectAll"
        >
          全选
        </Button>
        <Button
          variant="ghost"
          :disabled="loading || saving || selectedCount === 0"
          @click="selectNone"
        >
          全不选
        </Button>
        <span class="text-[12px] text-text-tertiary" v-no-i18n>
          已选 {{ selectedCount }} / {{ folders.length }}
        </span>
      </div>

      <p
        v-if="error"
        class="mt-0 mb-2 shrink-0 text-[13px] text-danger"
        role="alert"
      >
        {{ error }}
      </p>
      <p
        v-else-if="loading"
        class="mt-0 mb-2 shrink-0 text-[13px] text-text-secondary"
      >
        正在从邮箱拉取文件夹树…
      </p>

      <OverlayScrollArea class="min-h-0 flex-1">
        <ul
          v-if="!loading && rows.length"
          class="m-0 list-none divide-y divide-border-subtle overflow-hidden rounded-md border border-border-subtle bg-surface-chat p-0"
        >
          <li
            v-for="f in rows"
            :key="f.folder_id"
            class="px-3 py-2 text-[13px]"
          >
            <label
              class="flex min-w-0 cursor-pointer items-center gap-2 text-text-primary"
              :style="{ paddingLeft: `${f.depth * 16}px` }"
            >
              <input
                type="checkbox"
                :checked="isChecked(f.folder_id)"
                :disabled="saving"
                @change="
                  toggle(
                    f.folder_id,
                    ($event.target as HTMLInputElement).checked,
                  )
                "
              />
              <span class="truncate font-medium" v-no-i18n>{{
                f.display_name || f.folder_id
              }}</span>
            </label>
          </li>
        </ul>
        <p
          v-else-if="!loading && !error"
          class="m-0 rounded-md border border-dashed border-border-subtle px-3 py-6 text-center text-[13px] text-text-secondary"
        >
          没有可勾选的文件夹
        </p>
      </OverlayScrollArea>

      <footer class="mt-4 flex shrink-0 justify-end gap-2">
        <Button variant="ghost" :disabled="saving" @click="emit('close')">
          稍后设置
        </Button>
        <Button
          variant="primary"
          :disabled="saving || loading"
          @click="confirm"
        >
          {{ saving ? '保存中…' : '开始同步' }}
        </Button>
      </footer>
    </div>
  </div>
</template>
