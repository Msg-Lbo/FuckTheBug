<script setup lang="ts">
import AiChatView from './views/AiChatView.vue'
import SettingsView from './views/SettingsView.vue'
import TickerView from './views/TickerView.vue'

const currentView = new URLSearchParams(window.location.search).get('view') // 当前窗口视图
</script>

<template>
  <AiChatView v-if="currentView === 'ai'" />
  <SettingsView v-else-if="currentView === 'settings'" />
  <TickerView v-else />
</template>
