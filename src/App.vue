<script setup lang="ts">
import { onMounted, onUnmounted } from 'vue';
import AppShell from './features/shell/AppShell.vue';

/** 桌面壳禁用浏览器默认右键；有自定义菜单的地方自行 preventDefault + 弹菜单 */
function onContextMenu(e: Event): void {
  e.preventDefault();
}

onMounted(() => {
  window.addEventListener('contextmenu', onContextMenu, true);
});
onUnmounted(() => {
  window.removeEventListener('contextmenu', onContextMenu, true);
});
</script>

<template>
  <AppShell />
</template>
