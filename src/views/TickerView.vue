<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, type CSSProperties } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { AlertCircle, ExternalLink, RefreshCw, Settings, X } from '@lucide/vue'
import {
  fetchIssues,
  getConfig,
  openExternal,
  openSettingsWindow,
  resizeMainWindow,
  saveMainWindowPosition,
  sendSystemNotification,
  startMainDragging,
} from '../api'
import type { AppConfig, ViewRuntime } from '../types'

const config = ref<AppConfig>({
  jira: { baseUrl: '', refreshInterval: 1, token: '', hasToken: false, clearToken: false },
  views: [],
}) // 应用配置
const activeViewId = ref<string | null>(null) // 当前展开视图
const runtimeByView = reactive<Record<string, ViewRuntime>>({}) // 各视图运行状态
const refreshTimers = new Map<string, number>() // 各视图刷新定时器
const loadError = ref('') // 配置加载错误
let unlistenConfig: UnlistenFn | null = null // 配置事件解绑函数
let dragState: { viewId: string; startX: number; startY: number; moved: boolean } | null = null // 拖动状态
const notificationWelcomeKey = 'fuck-the-bug:notification-welcome:v1' // 原生通知启用提示标识
const projectPalette = [
  { accent: '#63a8dc', surface: '#1a2b37', border: '#365f7b', text: '#a9d4f2' },
  { accent: '#62b985', surface: '#1a2d23', border: '#37684a', text: '#a9ddb9' },
  { accent: '#d39a55', surface: '#33271a', border: '#765630', text: '#edc38c' },
  { accent: '#d47878', surface: '#332020', border: '#754040', text: '#efaaaa' },
  { accent: '#a98ad7', surface: '#2a2235', border: '#5d4a79', text: '#cfb8ed' },
  { accent: '#55b9b4', surface: '#182e2d', border: '#326865', text: '#a0deda' },
] as const // 项目颜色组

const activeView = computed(() => config.value.views.find((view) => view.id === activeViewId.value) ?? null)
const activeRuntime = computed(() => activeView.value ? runtimeByView[activeView.value.id] : null)

/**
 * 创建问题单视图运行状态
 * @returns 初始运行状态
 */
function createRuntime(): ViewRuntime {
  return {
    loading: false,
    initialized: false,
    hasNewIssues: false,
    count: 0,
    issues: [],
    error: '',
    updatedAt: null,
  }
}

/**
 * 计算悬浮窗口折叠宽度
 * @returns 逻辑像素宽度
 */
function getCollapsedWidth(): number {
  if (config.value.views.length === 0) return 250
  return Math.min(config.value.views.length * 88 + 36, 640)
}

/**
 * 清除所有刷新定时器
 */
function clearRefreshTimers(): void {
  refreshTimers.forEach((timer) => window.clearInterval(timer))
  refreshTimers.clear()
}

/**
 * 为全部JQL视图创建独立刷新计划
 */
function scheduleRefreshes(): void {
  clearRefreshTimers()
  config.value.views.forEach((view) => {
    const timer = window.setInterval(() => void refreshView(view.id), config.value.jira.refreshInterval * 60_000)
    refreshTimers.set(view.id, timer)
  })
}

/**
 * 加载配置并重建刷新任务
 */
async function loadConfig(): Promise<void> {
  try {
    const nextConfig = await getConfig()
    config.value = nextConfig
    loadError.value = ''

    Object.keys(runtimeByView).forEach((viewId) => {
      if (!nextConfig.views.some((view) => view.id === viewId)) delete runtimeByView[viewId]
    })

    nextConfig.views.forEach((view) => {
      runtimeByView[view.id] ??= createRuntime()
    })

    if (activeViewId.value && !nextConfig.views.some((view) => view.id === activeViewId.value)) {
      activeViewId.value = null
    }

    scheduleRefreshes()
    await nextTick()
    await resizeForCurrentState()
    await Promise.all(nextConfig.views.map((view) => refreshView(view.id)))
  } catch (error) {
    loadError.value = String(error)
    await resizeMainWindow(320, 88)
  }
}

/**
 * 刷新指定JQL视图
 * @param viewId - 视图标识
 */
async function refreshView(viewId: string): Promise<void> {
  const runtime = runtimeByView[viewId] // 当前运行状态
  if (!runtime || runtime.loading) return

  runtime.loading = true
  runtime.error = ''

  try {
    const result = await fetchIssues(viewId)
    const knownIssueKeys = new Set(runtime.issues.map((issue) => issue.key)) // 刷新前的问题单标识
    const newIssues = result.issues.filter((issue) => !knownIssueKeys.has(issue.key)) // 新增问题单
    const addedCount = Math.max(result.count - runtime.count, newIssues.length) // 新增问题单数量
    const hasAddedIssue = addedCount > 0 // 是否存在新增问题单
    if (runtime.initialized && activeViewId.value !== viewId && hasAddedIssue) {
      runtime.hasNewIssues = true
      void notifyNewIssues(result.viewName, addedCount, newIssues)
    }
    runtime.count = result.count
    runtime.issues = result.issues
    runtime.initialized = true
    runtime.updatedAt = new Date()
  } catch (error) {
    runtime.error = String(error)
    runtime.initialized = true
  } finally {
    runtime.loading = false
  }
}

/**
 * 根据展开状态调整窗口尺寸
 */
async function resizeForCurrentState(): Promise<void> {
  const width = getCollapsedWidth() // 目标窗口宽度
  await resizeMainWindow(activeViewId.value ? Math.max(width, 440) : width, activeViewId.value ? 600 : 88)
}

/**
 * 切换问题单详情面板
 * @param viewId - 视图标识
 */
async function toggleView(viewId: string): Promise<void> {
  const runtime = runtimeByView[viewId] // 当前视图运行状态
  if (runtime) runtime.hasNewIssues = false
  activeViewId.value = activeViewId.value === viewId ? null : viewId
  await nextTick()
  await resizeForCurrentState()
  if (activeViewId.value) await refreshView(activeViewId.value)
}

/**
 * 初始化Windows通知权限和点击监听
 */
async function initializeNotifications(): Promise<void> {
  try {
    if (!window.localStorage.getItem(notificationWelcomeKey)) {
      const sent = await sendSystemNotification('FuckTheBug通知已启用', '发现新问题单时，将在这里提醒你。')
      if (sent) window.localStorage.setItem(notificationWelcomeKey, 'shown')
    }
  } catch (error) {
    console.warn(`Windows通知初始化失败：${String(error)}`)
  }
}

/**
 * 发送新问题单Windows通知
 * @param viewName - 视图名称
 * @param addedCount - 新增数量
 * @param issues - 当前页新增问题单
 */
async function notifyNewIssues(viewName: string, addedCount: number, issues: ViewRuntime['issues']): Promise<void> {
  const issueLines = issues.slice(0, 3).map((issue) => {
    const title = issue.title.length > 46 ? `${issue.title.slice(0, 46)}…` : issue.title // 通知问题单标题
    return `${issue.key} ${title}`
  })
  const body = [`新增 ${addedCount} 条问题单`, ...issueLines].join('\n') // 通知正文
  try {
    await sendSystemNotification(`${viewName} 有新问题单`, body)
  } catch (error) {
    console.warn(`Windows通知发送失败：${String(error)}`)
  }
}

/**
 * 记录按下位置以区分点击和拖动
 * @param event - 指针事件
 * @param viewId - 视图标识
 */
function handlePointerDown(event: PointerEvent, viewId: string): void {
  if (event.button !== 0) return
  dragState = { viewId, startX: event.screenX, startY: event.screenY, moved: false }
  window.addEventListener('pointermove', handlePointerMove)
  window.addEventListener('pointerup', handlePointerUp, { once: true })
}

/**
 * 超过阈值后交由系统拖动窗口
 * @param event - 指针事件
 */
async function handlePointerMove(event: PointerEvent): Promise<void> {
  if (!dragState || dragState.moved) return
  const distance = Math.hypot(event.screenX - dragState.startX, event.screenY - dragState.startY) // 移动距离
  if (distance <= 5) return

  dragState.moved = true
  window.removeEventListener('pointermove', handlePointerMove)
  await startMainDragging()
  await saveMainWindowPosition()
  dragState = null
}

/**
 * 未触发拖动时按普通点击处理
 */
function handlePointerUp(): void {
  window.removeEventListener('pointermove', handlePointerMove)
  if (dragState && !dragState.moved) void toggleView(dragState.viewId)
  dragState = null
}

/**
 * 打开问题单外部页面
 * @param url - 问题单链接
 */
async function handleOpenExternal(url: string): Promise<void> {
  try {
    await openExternal(url)
  } catch (error) {
    if (activeRuntime.value) activeRuntime.value.error = String(error)
  }
}

/**
 * 格式化更新时间
 * @param date - 更新时间
 * @returns 时间文本
 */
function formatUpdatedAt(date: Date | null): string {
  if (!date) return ''
  return new Intl.DateTimeFormat('zh-CN', { hour: '2-digit', minute: '2-digit', second: '2-digit' }).format(date)
}

/**
 * 根据项目键生成稳定的项目配色
 * @param projectKey - JIRA项目键
 * @returns 项目行CSS变量
 */
function getProjectStyle(projectKey: string): CSSProperties {
  let hash = 0 // 项目键哈希
  for (const character of projectKey) hash = (hash * 31 + character.charCodeAt(0)) >>> 0
  const color = projectPalette[hash % projectPalette.length] // 稳定项目颜色
  return {
    '--project-accent': color.accent,
    '--project-surface': color.surface,
    '--project-border': color.border,
    '--project-text': color.text,
  } as CSSProperties
}

onMounted(async () => {
  await initializeNotifications()
  await loadConfig()
  unlistenConfig = await listen('config-updated', () => void loadConfig())
})

onBeforeUnmount(() => {
  clearRefreshTimers()
  unlistenConfig?.()
  window.removeEventListener('pointermove', handlePointerMove)
  window.removeEventListener('pointerup', handlePointerUp)
})
</script>

<template>
  <main class="ticker-shell">
    <section class="ticker-bar" aria-label="问题单统计">
      <div v-if="loadError" class="ticker-message ticker-message--error">
        <AlertCircle :size="18" />
        <span>{{ loadError }}</span>
      </div>

      <template v-else>
        <button
          v-for="view in config.views"
          :key="view.id"
          class="counter"
          :class="{
            'counter--active': activeViewId === view.id,
            'counter--error': runtimeByView[view.id]?.error,
            'counter--new': runtimeByView[view.id]?.hasNewIssues && !runtimeByView[view.id]?.error,
          }"
          :title="runtimeByView[view.id]?.hasNewIssues ? `${view.name}：有新问题单` : view.name"
          type="button"
          @pointerdown="handlePointerDown($event, view.id)"
        >
          <span class="counter__value">
            {{ runtimeByView[view.id]?.error ? '!' : runtimeByView[view.id]?.initialized ? runtimeByView[view.id].count : '--' }}
          </span>
          <span class="counter__name">{{ view.name }}</span>
          <span v-if="runtimeByView[view.id]?.loading" class="counter__progress" />
        </button>

        <div v-if="config.views.length === 0" class="ticker-message">
          <span>尚未配置问题单视图</span>
        </div>
      </template>

      <button class="icon-button ticker-settings" type="button" title="设置" aria-label="打开设置" @click="openSettingsWindow">
        <Settings :size="17" />
      </button>
    </section>

    <section v-if="activeView && activeRuntime" class="bug-panel">
      <header class="bug-panel__header">
        <div class="bug-panel__heading">
          <strong>{{ activeView.name }}</strong>
          <span v-if="activeRuntime.updatedAt">更新于 {{ formatUpdatedAt(activeRuntime.updatedAt) }}</span>
        </div>
        <div class="bug-panel__actions">
          <button class="icon-button" type="button" title="刷新" aria-label="刷新" :disabled="activeRuntime.loading" @click="refreshView(activeView.id)">
            <RefreshCw :size="17" :class="{ spinning: activeRuntime.loading }" />
          </button>
          <button class="icon-button" type="button" title="关闭" aria-label="关闭" @click="toggleView(activeView.id)">
            <X :size="18" />
          </button>
        </div>
      </header>

      <div class="bug-panel__content">
        <div v-if="activeRuntime.error" class="panel-state panel-state--error">
          <AlertCircle :size="22" />
          <span>{{ activeRuntime.error }}</span>
          <button class="text-button" type="button" @click="refreshView(activeView.id)">重试</button>
        </div>

        <div v-else-if="activeRuntime.loading && !activeRuntime.initialized" class="panel-state">
          <RefreshCw class="spinning" :size="22" />
          <span>正在加载</span>
        </div>

        <div v-else-if="activeRuntime.issues.length === 0" class="panel-state panel-state--success">
          <span class="status-dot" />
          <span>当前没有符合条件的问题单</span>
        </div>

        <button
          v-for="issue in activeRuntime.issues"
          v-else
          :key="issue.key"
          class="bug-row"
          :style="getProjectStyle(issue.projectKey)"
          type="button"
          @click="handleOpenExternal(issue.link)"
        >
          <span class="bug-row__main">
            <strong><span class="issue-key">{{ issue.key }}</span>{{ issue.title }}</strong>
            <span class="bug-row__meta">
              <span class="project-tag" :title="issue.projectName">{{ issue.projectKey }}</span>
              <span v-if="issue.issueType">{{ issue.issueType }}</span>
              <span v-if="issue.status">{{ issue.status }}</span>
              <span v-if="issue.priority">{{ issue.priority }}</span>
            </span>
          </span>
          <ExternalLink :size="16" />
        </button>
      </div>
    </section>
  </main>
</template>
