<script setup lang="ts">
import SettingsView from './views/SettingsView.vue'
import TickerView from './views/TickerView.vue'

const currentView = new URLSearchParams(window.location.search).get('view') // 当前窗口视图
</script>

<template>
  <SettingsView v-if="currentView === 'settings'" />
  <TickerView v-else />
</template>
