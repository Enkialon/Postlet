<script setup lang="ts">
import { computed, inject, nextTick, onMounted, ref, watch } from 'vue';
import { navigationKey } from '../../app/provide';
import type { MessageWithRecipients } from '../../domain/entities';
import Button from '../../shared/ui/Button.vue';
import { useMailEditor } from './useMailEditor';
import { useComposeDraft } from './useComposeDraft';
import MailEditorCore from './MailEditorCore.vue';
import ComposeToolbar from './ComposeToolbar.vue';
import RecipientChipsInput from './RecipientChipsInput.vue';

const props = defineProps<{
  /** 无默认收件人时，快捷发送会先打开完整模式补全 */
  requireRecipientsForSend?: boolean;
}>();

const emit = defineEmits<{
  sent: [item: MessageWithRecipients];
  modeChange: [mode: 'compact' | 'full'];
}>();

const nav = inject(navigationKey)!;

const mode = ref<'compact' | 'full'>('compact');
const fileInputRef = ref<HTMLInputElement | null>(null);
const showCc = ref(false);
const showBcc = ref(false);
const dragOver = ref(false);
const ccFieldRef = ref<InstanceType<typeof RecipientChipsInput> | null>(null);
const bccFieldRef = ref<InstanceType<typeof RecipientChipsInput> | null>(null);

const mailEditor = useMailEditor({ placeholder: '写邮件…' });
const {
  fields,
  sending,
  error,
  loadForConversation,
  scheduleSave,
  beginReply: setReplyTarget,
  clearReplyTarget,
  addAttachments,
  removeAttachment,
  send,
  defaultToRecipients,
} = useComposeDraft();

const editorInstance = computed(() => mailEditor.editor.value);

const replyHint = computed(() =>
  fields.replyToMessageId ? '正在回复邮件' : null,
);

function onRecipientsChange(): void {
  scheduleSave(mailEditor.getHtml());
}

function onEditorUpdate(): void {
  scheduleSave(mailEditor.getHtml());
}

watch(
  editorInstance,
  (ed, _prev, onCleanup) => {
    if (!ed) return;
    ed.on('update', onEditorUpdate);
    const dom = ed.view.dom;
    const onKey = (ev: KeyboardEvent) => {
      if (mode.value === 'compact') {
        if (ev.key === 'Enter' && !ev.shiftKey) {
          ev.preventDefault();
          void doSend();
        }
      } else if (ev.key === 'Enter' && (ev.ctrlKey || ev.metaKey)) {
        ev.preventDefault();
        void doSend();
      }
    };
    dom.addEventListener('keydown', onKey);
    onCleanup(() => {
      ed.off('update', onEditorUpdate);
      dom.removeEventListener('keydown', onKey);
    });
  },
);

function syncCcBccVisibility(): void {
  showCc.value = fields.cc.length > 0;
  showBcc.value = fields.bcc.length > 0;
}

async function reloadDraft(): Promise<void> {
  const html = await loadForConversation();
  mailEditor.ensureEditor();
  mailEditor.setHtml(html);
  syncCcBccVisibility();
  if (mode.value === 'full') {
    mode.value = 'compact';
    emit('modeChange', 'compact');
  }
}

function revealCc(): void {
  showCc.value = true;
  void nextTick(() => ccFieldRef.value?.focus());
}

function revealBcc(): void {
  showBcc.value = true;
  void nextTick(() => bccFieldRef.value?.focus());
}

onMounted(async () => {
  mailEditor.ensureEditor();
  await reloadDraft();
});

watch(
  () => [nav.accountId.value, nav.conversationId.value] as const,
  async () => {
    await reloadDraft();
  },
);

async function expand(): Promise<void> {
  if (mode.value === 'full') return;
  if (fields.to.length === 0) {
    fields.to = await defaultToRecipients();
  }
  syncCcBccVisibility();
  mode.value = 'full';
  emit('modeChange', 'full');
  void nextTick(() => mailEditor.focus());
}

function collapse(): void {
  scheduleSave(mailEditor.getHtml());
  mode.value = 'compact';
  emit('modeChange', 'compact');
  void nextTick(() => mailEditor.focus());
}

function beginReply(item: MessageWithRecipients): void {
  void setReplyTarget(item).then(() => {
    syncCcBccVisibility();
    void nextTick(() => mailEditor.focus());
  });
}

function pickFiles(): void {
  fileInputRef.value?.click();
}

function onFilesSelected(e: Event): void {
  const input = e.target as HTMLInputElement;
  if (input.files?.length) {
    addAttachments(input.files);
    scheduleSave(mailEditor.getHtml());
  }
  input.value = '';
}

function onDrop(e: DragEvent): void {
  e.preventDefault();
  dragOver.value = false;
  const files = Array.from(e.dataTransfer?.files ?? []);
  if (!files.length) return;
  // 图片进正文（与 Outlook 内嵌一致）；其余仍作附件
  const images = files.filter((f) => f.type.startsWith('image/'));
  const others = files.filter((f) => !f.type.startsWith('image/'));
  if (images.length) {
    const ed = mailEditor.ensureEditor();
    void (async () => {
      for (const file of images) {
        const src = await new Promise<string>((resolve, reject) => {
          const reader = new FileReader();
          reader.onload = () => resolve(String(reader.result ?? ''));
          reader.onerror = () => reject(reader.error);
          reader.readAsDataURL(file);
        });
        if (src) ed.chain().focus().setImage({ src, alt: file.name }).run();
      }
      scheduleSave(mailEditor.getHtml());
    })();
  }
  if (others.length) {
    addAttachments(others);
    scheduleSave(mailEditor.getHtml());
  }
}

async function doSend(): Promise<void> {
  if (
    fields.to.length === 0 &&
    fields.cc.length === 0 &&
    fields.bcc.length === 0
  ) {
    const defaults = await defaultToRecipients();
    if (defaults.length > 0) {
      fields.to = defaults;
    } else if (props.requireRecipientsForSend || mode.value === 'compact') {
      await expand();
      error.value = '请填写收件人';
      return;
    } else {
      error.value = '请填写收件人';
      return;
    }
  }

  const result = await send(mailEditor.getHtml());
  if (!result) return;
  mailEditor.clear();
  syncCcBccVisibility();
  if (mode.value === 'full') collapse();
  emit('sent', result);
  await nextTick();
  mailEditor.focus();
}

defineExpose({
  beginReply,
  expand,
  collapse,
  mode,
  focus: () => mailEditor.focus(),
});
</script>

<template>
  <div
    class="flex flex-col bg-surface-elevated"
    :class="
      mode === 'full'
        ? 'absolute inset-0 z-[5]'
        : 'shrink-0 border-t border-border-subtle'
    "
    :aria-label="mode === 'full' ? '完整写信' : '快捷写信'"
    @dragover.prevent="dragOver = true"
    @dragleave="dragOver = false"
    @drop="onDrop"
  >
    <template v-if="mode === 'full'">
      <div class="shrink-0 space-y-2 border-b border-border-subtle px-4 py-3">
        <div class="grid grid-cols-[3rem_1fr] items-start gap-2 text-[13px]">
          <span class="pt-2 text-text-secondary">收件人</span>
          <RecipientChipsInput
            v-model="fields.to"
            placeholder="输入联系人，回车添加"
            @change="onRecipientsChange"
          >
            <template #trailing>
              <div
                v-if="!showCc || !showBcc"
                class="flex shrink-0 items-center gap-2 self-center"
              >
                <button
                  v-if="!showCc"
                  type="button"
                  class="border-0 bg-transparent p-0 text-[12px] font-medium text-text-secondary hover:text-brand"
                  @click="revealCc"
                >
                  抄送
                </button>
                <button
                  v-if="!showBcc"
                  type="button"
                  class="border-0 bg-transparent p-0 text-[12px] font-medium text-text-secondary hover:text-brand"
                  @click="revealBcc"
                >
                  密送
                </button>
              </div>
            </template>
          </RecipientChipsInput>
        </div>

        <div
          v-if="showCc"
          class="grid grid-cols-[3rem_1fr] items-start gap-2 text-[13px]"
        >
          <span class="pt-2 text-text-secondary">抄送</span>
          <RecipientChipsInput
            ref="ccFieldRef"
            v-model="fields.cc"
            placeholder="输入联系人，回车添加"
            @change="onRecipientsChange"
          />
        </div>

        <div
          v-if="showBcc"
          class="grid grid-cols-[3rem_1fr] items-start gap-2 text-[13px]"
        >
          <span class="pt-2 text-text-secondary">密送</span>
          <RecipientChipsInput
            ref="bccFieldRef"
            v-model="fields.bcc"
            placeholder="输入联系人，回车添加"
            @change="onRecipientsChange"
          />
        </div>

        <label class="grid grid-cols-[3rem_1fr] items-center gap-2 text-[13px]">
          <span class="text-text-secondary">主题</span>
          <input
            v-model="fields.subject"
            class="rounded-md border border-border-subtle bg-surface-input px-2.5 py-1.5 text-[13px] text-text-primary outline-none focus:border-brand"
            @input="scheduleSave(mailEditor.getHtml())"
          />
        </label>
        <p v-if="replyHint" class="m-0 text-[12px] text-text-tertiary">
          {{ replyHint }}
          <button
            type="button"
            class="ml-2 border-0 bg-transparent p-0 text-brand"
            @click="clearReplyTarget()"
          >
            取消回复
          </button>
        </p>
      </div>

      <ComposeToolbar :editor="editorInstance" />
    </template>

    <div
      class="relative min-h-0"
      :class="
        mode === 'full'
          ? 'flex-1 overflow-auto px-4 py-3'
          : 'max-h-[400px] min-h-[104px] overflow-auto px-3 pt-2'
      "
      :data-drag-over="dragOver ? '1' : undefined"
    >
      <MailEditorCore :editor="editorInstance" />
    </div>

    <ul
      v-if="fields.attachments.length"
      class="m-0 flex list-none flex-wrap gap-1.5 px-3 pb-1"
    >
      <li
        v-for="att in fields.attachments"
        :key="att.id"
        class="flex items-center gap-1 rounded-pill bg-surface-muted px-2 py-0.5 text-[12px] text-text-secondary"
      >
        <span class="max-w-[10rem] truncate" v-no-i18n>{{ att.name }}</span>
        <button
          type="button"
          class="border-0 bg-transparent p-0 text-text-tertiary hover:text-danger"
          :aria-label="`移除 ${att.name}`"
          @click="
            removeAttachment(att.id);
            scheduleSave(mailEditor.getHtml());
          "
        >
          ×
        </button>
      </li>
    </ul>

    <p v-if="error" class="m-0 px-3 pb-1 text-[12px] text-danger" role="alert">
      {{ error }}
    </p>

    <div
      class="flex shrink-0 items-center gap-2 px-3 py-2"
      :class="mode === 'compact' ? '' : 'border-t border-border-subtle'"
    >
      <input
        ref="fileInputRef"
        type="file"
        class="hidden"
        multiple
        @change="onFilesSelected"
      />
      <Button
        variant="ghost"
        class="!px-2"
        aria-label="添加附件"
        @click="pickFiles"
      >
        <svg class="block size-[18px]" viewBox="0 0 24 24" aria-hidden="true">
          <path
            fill="currentColor"
            d="M16.5 6.75v7.5a4.5 4.5 0 1 1-9 0V5.25a3 3 0 0 1 6 0v8.25a1.5 1.5 0 1 1-3 0V6.75H9v6.75a3 3 0 1 0 6 0V5.25a4.5 4.5 0 1 0-9 0v9a6 6 0 0 0 12 0v-7.5h-1.5Z"
          />
        </svg>
      </Button>

      <span
        v-if="mode === 'compact' && replyHint"
        class="min-w-0 flex-1 truncate text-[12px] text-text-tertiary"
      >
        {{ replyHint }}
      </span>
      <span v-else class="flex-1" />

      <Button
        v-if="mode === 'compact'"
        variant="ghost"
        class="!px-2"
        aria-label="最大化"
        @click="expand"
      >
        <svg class="block size-[18px]" viewBox="0 0 24 24" aria-hidden="true">
          <path
            fill="currentColor"
            d="M3 3h7v2H5v5H3V3zm11 0h7v7h-2V5h-5V3zM3 14h2v5h5v2H3v-7zm16 0h2v7h-7v-2h5v-5z"
          />
        </svg>
      </Button>
      <Button
        v-else
        variant="ghost"
        class="!px-2"
        aria-label="收起"
        @click="collapse"
      >
        <svg class="block size-[18px]" viewBox="0 0 24 24" aria-hidden="true">
          <path
            fill="currentColor"
            d="M9 3H7v4H3v2h6V3zm8 0h-2v6h6V7h-4V3zM9 15H3v2h4v4h2v-6zm6 0v6h2v-4h4v-2h-6z"
          />
        </svg>
      </Button>

      <Button variant="primary" :disabled="sending" @click="doSend"
        >发送</Button
      >
    </div>
  </div>
</template>
