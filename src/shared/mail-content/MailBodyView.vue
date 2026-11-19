<script setup lang="ts">
import { computed, inject, onBeforeUnmount, ref, watch } from 'vue';
import type { BodyContentType, FolderRole } from '../../domain/types';
import { portsKey } from '../../app/provide';
import { bindMailLinks, createMailDocument } from './mailDocument';

const props = withDefaults(defineProps<{
  content: string;
  contentType: BodyContentType;
  /** 当前消息的生效文件夹角色；junk 只显示文本，不创建正文 iframe */
  folderRole?: FolderRole | null;
  title?: string;
}>(), { title: '邮件正文', folderRole: null });

const ports = inject(portsKey)!;
const linkError = ref('');
const frame = ref<HTMLIFrameElement | null>(null);
const height = ref(160);
let resizeObserver: ResizeObserver | undefined;
let resizeFrame = 0;

const junk = computed(() => props.folderRole === 'junk');
// 普通邮件：允许的远程图片直接加载（净化与隔离仍由 mailDocument 保证）
const source = computed(() => createMailDocument(props.content));

watch(
  () => [props.content, props.contentType, props.folderRole],
  () => {
    linkError.value = '';
    height.value = 160;
    // 内容或角色变了：旧 iframe 卸载前先断开测量
    resizeObserver?.disconnect();
    cancelAnimationFrame(resizeFrame);
  },
  { flush: 'sync' },
);

function onFrameLoad(): void {
  resizeObserver?.disconnect();
  cancelAnimationFrame(resizeFrame);
  const element = frame.value;
  if (!element?.contentDocument?.body) return;
  bindMailLinks(element, async (url) => {
    linkError.value = '';
    try { await ports.platform.openExternalUrl(url); }
    catch { linkError.value = '无法打开链接，请重试'; }
  });
  const body = element.contentDocument.body;
  const updateHeight = () => {
    height.value = Math.max(160, Math.min(20000, Math.ceil(body.getBoundingClientRect().height + 4)));
  };
  resizeObserver = new ResizeObserver(() => {
    cancelAnimationFrame(resizeFrame);
    resizeFrame = requestAnimationFrame(updateHeight);
  });
  resizeObserver.observe(body);
  updateHeight();
}
onBeforeUnmount(() => {
  resizeObserver?.disconnect();
  cancelAnimationFrame(resizeFrame);
});
</script>

<template>
  <div class="min-h-40 w-full">
    <!-- 垃圾邮件：纯文本插值，不建 iframe、不激活链接、不请求远程资源 -->
    <div
      v-if="junk"
      class="whitespace-pre-wrap break-words text-[13px] leading-relaxed text-text-secondary"
      v-no-i18n
    >{{ content }}</div>
    <template v-else-if="contentType === 'html'">
      <p v-if="linkError" class="mb-2 text-[12px] text-danger" role="alert">{{ linkError }}</p>
      <!-- Same origin permits parent resize/link handlers; scripts are forbidden
           by both sandbox and the frame CSP. Never add allow-scripts. -->
      <iframe
        ref="frame"
        sandbox="allow-same-origin"
        referrerpolicy="no-referrer"
        :srcdoc="source"
        :title="title"
        class="block w-full border-0 bg-white"
        :style="{ height: `${height}px` }"
        @load="onFrameLoad"
      />
    </template>
    <div v-else class="whitespace-pre-wrap break-words text-[14px] leading-relaxed" v-no-i18n>{{ content }}</div>
  </div>
</template>
