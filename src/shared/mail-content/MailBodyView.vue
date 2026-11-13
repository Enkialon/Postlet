<script setup lang="ts">
import { computed, inject, type Component } from 'vue';
import { Letter as LetterImpl } from 'vue-letter';
import type { BodyContentType } from '../../domain/types';
import { portsKey } from '../../app/provide';

/** vue-letter 的 d.ts 把可选 props 标成了 required，运行时仍有默认值 */
const Letter = LetterImpl as Component;

const props = withDefaults(
  defineProps<{
    content: string;
    contentType: BodyContentType;
    title?: string;
  }>(),
  {
    title: '邮件正文',
  },
);

const ports = inject(portsKey)!;

const html = computed(() =>
  props.contentType === 'html' ? props.content : '',
);
const text = computed(() =>
  props.contentType === 'text' ? props.content : '',
);

async function onClickCapture(event: MouseEvent): Promise<void> {
  const target = event.target as HTMLElement | null;
  const anchor = target?.closest?.('a');
  if (!anchor) return;
  const href = anchor.getAttribute('href') || '';
  if (!href || href.toLowerCase().startsWith('mailto:')) return;
  if (!/^https?:\/\//i.test(href)) {
    event.preventDefault();
    return;
  }
  event.preventDefault();
  event.stopPropagation();
  await ports.platform.openExternalUrl(href);
}
</script>

<template>
  <div class="min-h-40 w-full" v-no-i18n @click.capture="onClickCapture">
    <Letter
      class-name="mail-letter__inner"
      :html="html"
      :text="text"
      :use-iframe="false"
      :iframe-title="title"
      :allowed-schemas="['http', 'https', 'mailto']"
    />
  </div>
</template>
