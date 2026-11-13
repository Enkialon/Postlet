<script setup lang="ts">
import { computed, inject, onMounted, onUnmounted, ref, watch } from 'vue';
import type { BodyContentType } from '../../domain/types';
import { portsKey } from '../../app/provide';
import {
  isSafeExternalHttpUrl,
  sanitizeMailHtml,
  wrapSandboxedMailDocument,
} from './sanitizeHtml';

const props = defineProps<{
  content: string;
  contentType: BodyContentType;
}>();

const ports = inject(portsKey)!;
const iframeRef = ref<HTMLIFrameElement | null>(null);

const textContent = computed(() => props.content);
const htmlSrcdoc = computed(() =>
  wrapSandboxedMailDocument(sanitizeMailHtml(props.content)),
);

async function onIframeClick(event: MouseEvent) {
  const target = event.target as HTMLElement | null;
  if (!target) return;
  const anchor = target.closest('a');
  if (!anchor) return;
  event.preventDefault();
  event.stopPropagation();
  const href = anchor.getAttribute('href') || '';
  if (href.toLowerCase().startsWith('mailto:')) return;
  if (!isSafeExternalHttpUrl(href)) return;
  await ports.platform.openExternalUrl(href);
}

function bindIframe() {
  const doc = iframeRef.value?.contentDocument;
  if (!doc) return;
  doc.removeEventListener('click', onIframeClick);
  doc.addEventListener('click', onIframeClick);
}

watch(htmlSrcdoc, () => {
  requestAnimationFrame(bindIframe);
});

onMounted(() => requestAnimationFrame(bindIframe));
onUnmounted(() => {
  iframeRef.value?.contentDocument?.removeEventListener('click', onIframeClick);
});
</script>

<template>
  <div class="mail-body" v-no-i18n>
    <pre v-if="contentType === 'text'" class="mail-body__text">{{
      textContent
    }}</pre>
    <iframe
      v-else
      ref="iframeRef"
      class="mail-body__html"
      title="邮件正文"
      sandbox="allow-same-origin allow-popups"
      :srcdoc="htmlSrcdoc"
    />
  </div>
</template>

<style scoped>
.mail-body__text {
  margin: 0;
  white-space: pre-wrap;
  word-break: break-word;
  font-family: inherit;
  font-size: 0.95rem;
  line-height: 1.5;
}
.mail-body__html {
  width: 100%;
  min-height: 160px;
  border: 0;
  background: transparent;
}
</style>
