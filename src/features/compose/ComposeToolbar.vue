<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import type { Editor } from '@tiptap/vue-3';
import Button from '../../shared/ui/Button.vue';

const props = defineProps<{
  editor: Editor | null;
}>();

/** 驱动工具栏随选区刷新 */
const tick = ref(0);

watch(
  () => props.editor,
  (ed, _prev, onCleanup) => {
    if (!ed) return;
    const bump = () => {
      tick.value += 1;
    };
    ed.on('selectionUpdate', bump);
    ed.on('transaction', bump);
    onCleanup(() => {
      ed.off('selectionUpdate', bump);
      ed.off('transaction', bump);
    });
  },
  { immediate: true },
);

onBeforeUnmount(() => {
  /* watch cleanup handles editor listeners */
});

const headingValue = computed(() => {
  void tick.value;
  const ed = props.editor;
  if (!ed) return 'paragraph';
  if (ed.isActive('heading', { level: 1 })) return '1';
  if (ed.isActive('heading', { level: 2 })) return '2';
  if (ed.isActive('heading', { level: 3 })) return '3';
  return 'paragraph';
});

function run(fn: (editor: Editor) => void): void {
  if (!props.editor) return;
  fn(props.editor);
}

function onHeadingChange(e: Event): void {
  const value = (e.target as HTMLSelectElement).value;
  const ed = props.editor;
  if (!ed) return;
  if (value === 'paragraph') {
    ed.chain().focus().setParagraph().run();
    return;
  }
  const level = Number(value) as 1 | 2 | 3;
  ed.chain().focus().toggleHeading({ level }).run();
}

function setLink(): void {
  if (!props.editor) return;
  const prev = props.editor.getAttributes('link').href as string | undefined;
  const url = window.prompt('链接地址', prev ?? 'https://');
  if (url === null) return;
  if (url.trim() === '') {
    props.editor.chain().focus().extendMarkRange('link').unsetLink().run();
    return;
  }
  props.editor
    .chain()
    .focus()
    .extendMarkRange('link')
    .setLink({ href: url.trim() })
    .run();
}
</script>

<template>
  <div
    class="flex flex-wrap items-center gap-0.5 border-b border-border-subtle px-2 py-1.5"
    role="toolbar"
    aria-label="格式"
  >
    <label class="sr-only" for="compose-heading">标题级别</label>
    <select
      id="compose-heading"
      class="h-8 rounded-md border border-border-subtle bg-surface-input px-2 text-[12px] text-text-primary outline-none focus:border-brand disabled:opacity-45"
      :value="headingValue"
      :disabled="!editor"
      aria-label="标题级别"
      @change="onHeadingChange"
    >
      <option value="paragraph">正文</option>
      <option value="1">标题 1</option>
      <option value="2">标题 2</option>
      <option value="3">标题 3</option>
    </select>

    <span class="mx-1 h-4 w-px bg-border-subtle" aria-hidden="true" />

    <Button
      variant="ghost"
      aria-label="加粗"
      :disabled="!editor"
      @click="run((e) => e.chain().focus().toggleBold().run())"
      >B</Button
    >
    <Button
      variant="ghost"
      aria-label="斜体"
      :disabled="!editor"
      @click="run((e) => e.chain().focus().toggleItalic().run())"
      ><em>I</em></Button
    >
    <Button
      variant="ghost"
      aria-label="下划线"
      :disabled="!editor"
      @click="run((e) => e.chain().focus().toggleUnderline().run())"
      ><span class="underline">U</span></Button
    >
    <Button
      variant="ghost"
      aria-label="删除线"
      :disabled="!editor"
      @click="run((e) => e.chain().focus().toggleStrike().run())"
      ><span class="line-through">S</span></Button
    >
    <span class="mx-1 h-4 w-px bg-border-subtle" aria-hidden="true" />
    <Button
      variant="ghost"
      aria-label="无序列表"
      :disabled="!editor"
      @click="run((e) => e.chain().focus().toggleBulletList().run())"
      >• 列表</Button
    >
    <Button
      variant="ghost"
      aria-label="有序列表"
      :disabled="!editor"
      @click="run((e) => e.chain().focus().toggleOrderedList().run())"
      >1. 列表</Button
    >
    <Button
      variant="ghost"
      aria-label="引用"
      :disabled="!editor"
      @click="run((e) => e.chain().focus().toggleBlockquote().run())"
      >引用</Button
    >
    <Button variant="ghost" aria-label="链接" :disabled="!editor" @click="setLink"
      >链接</Button
    >
    <span class="mx-1 h-4 w-px bg-border-subtle" aria-hidden="true" />
    <Button
      variant="ghost"
      aria-label="撤销"
      :disabled="!editor"
      @click="run((e) => e.chain().focus().undo().run())"
      >撤销</Button
    >
    <Button
      variant="ghost"
      aria-label="重做"
      :disabled="!editor"
      @click="run((e) => e.chain().focus().redo().run())"
      >重做</Button
    >
    <Button
      variant="ghost"
      aria-label="清除格式"
      :disabled="!editor"
      @click="
        run((e) => e.chain().focus().unsetAllMarks().clearNodes().run())
      "
      >清除</Button
    >
  </div>
</template>
