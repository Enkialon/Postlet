<script setup lang="ts">
import { computed, inject, onBeforeUnmount, ref, watch } from 'vue';
import type { BodyContentType } from '../../domain/types';
import { portsKey } from '../../app/provide';
import { bindMailLinks, createMailDocument } from './mailDocument';
import Button from '../ui/Button.vue';

const props = withDefaults(defineProps<{
  content: string;
  contentType: BodyContentType;
  title?: string;
}>(), { title: '邮件正文' });

const ports = inject(portsKey)!;
const allowRemoteImages = ref(false);
const linkError = ref('');
const frame = ref<HTMLIFrameElement | null>(null);
const height = ref(160);
let resizeObserver: ResizeObserver | undefined;
let resizeFrame = 0;
watch(() => [props.content, props.contentType], () => {
  allowRemoteImages.value = false;
  linkError.value = '';
  height.value = 160;
}, { flush: 'sync' });
const source = computed(() => createMailDocument(props.content, allowRemoteImages.value));

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
    <template v-if="contentType === 'html'">
      <div class="mb-3 flex items-center gap-2 text-[12px] text-text-secondary">
        <span>{{ allowRemoteImages ? '已允许此邮件加载远程图片' : '远程图片已屏蔽' }}</span>
        <Button variant="ghost" @click="allowRemoteImages = !allowRemoteImages">
          {{ allowRemoteImages ? '屏蔽远程图片' : '加载此邮件的远程图片' }}
        </Button>
      </div>
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
