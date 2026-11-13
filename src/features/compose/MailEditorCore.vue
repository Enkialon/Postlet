<script setup lang="ts">
import { EditorContent, type Editor } from '@tiptap/vue-3';

defineProps<{
  editor: Editor | null;
}>();
</script>

<template>
  <EditorContent
    v-if="editor"
    :editor="editor"
    class="mail-editor-host size-full"
  />
</template>
