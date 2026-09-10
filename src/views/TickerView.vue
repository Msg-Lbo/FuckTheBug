<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, type CSSProperties } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { Archive, AlertCircle, Check, RefreshCw, Settings, Sparkles, StickyNote, Trash2, X } from '@lucide/vue'
import {
  clearStashedIssues,
  fetchIssues,
  getConfig,
  openAiChatWindow,
  openExternal,
  openSettingsWindow,
  resizeMainWindow,
  saveIssueNote,
  saveMainWindowPosition,
  sendSystemNotification,
  stashIssue as persistStashedIssue,
  startMainDragging,
  unstashIssue as persistUnstashedIssue,
} from '../api'
import type { AppConfig, IssueItem, IssueView, ViewRuntime } from '../types'

const config = ref<AppConfig>({
  jira: { baseUrl: '', refreshInterval: 1, token: '', hasToken: false, clearToken: false },
  ai: { baseUrl: '', model: '', token: '', hasToken: false, clearToken: false, skill: '', defaultSkill: '', outputFormat: '' },
  notes: {},
  views: [],
}) // 应用配置
const activeViewId = ref<string | null>(null) // 当前展开视图
const runtimeByView = reactive<Record<string, ViewRuntime>>({}) // 各视图运行状态
const versionFilterByView = reactive<Record<string, string>>({}) // 各视图版本筛选
const platformFilterByView = reactive<Record<string, string>>({}) // 各视图平台筛选
const refreshTimers = new Map<string, number>() // 各视图刷新定时器
const loadError = ref('') // 配置加载错误
let unlistenConfig: UnlistenFn | null = null // 配置事件解绑函数
let dragState: { viewId: string; startX: number; startY: number; moved: boolean } | null = null // 拖动状态
const contextMenu = reactive({ visible: false, x: 0, y: 0, issueKey: '' }) // 问题单右键菜单
const noteEditor = reactive({ visible: false, issueKey: '', text: '' }) // 备注编辑弹层
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
const activeVersionFilter = computed({
  get: () => activeViewId.value ? versionFilterByView[activeViewId.value] ?? '' : '',
  set: (value: string) => {
    if (activeViewId.value) versionFilterByView[activeViewId.value] = value
  },
}) // 当前视图版本筛选
const activePlatformFilter = computed({
  get: () => activeViewId.value ? platformFilterByView[activeViewId.value] ?? '' : '',
  set: (value: string) => {
    if (activeViewId.value) platformFilterByView[activeViewId.value] = value
  },
}) // 当前视图平台筛选
const activeVersionOptions = computed(() => {
  const versions = activeRuntime.value?.issues.flatMap((issue) => issue.versions ?? []) ?? [] // 当前列表全部版本
  return [...new Set(versions)].sort((left, right) => left.localeCompare(right, 'zh-CN', { numeric: true }))
})
const activePlatformOptions = computed(() => {
  const platforms = activeRuntime.value?.issues.flatMap((issue) => issue.platforms ?? []) ?? [] // 当前列表全部平台
  return ['Android', 'iOS'].filter((platform) => platforms.includes(platform))
})
const activeVisibleIssues = computed(() => {
  if (!activeRuntime.value) return []
  return activeRuntime.value.issues.filter((issue) => {
    const matchesVersion = !activeVersionFilter.value || (issue.versions ?? []).includes(activeVersionFilter.value) // 版本是否匹配
    const matchesPlatform = !activePlatformFilter.value || (issue.platforms ?? []).includes(activePlatformFilter.value) // 平台是否匹配
    return matchesVersion && matchesPlatform
  })
})

/**
 * 切换版本筛选Tag
 * @param version - 版本号
 */
function toggleVersionFilter(version: string): void {
  activeVersionFilter.value = activeVersionFilter.value === version ? '' : version
}

/**
 * 切换平台筛选Tag
 * @param platform - 平台名称
 */
function togglePlatformFilter(platform: string): void {
  activePlatformFilter.value = activePlatformFilter.value === platform ? '' : platform
}

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
 * 将视图配置同步到运行状态
 * @param view - 问题单视图
 */
function syncViewRuntime(view: IssueView): void {
  runtimeByView[view.id] ??= createRuntime()
  if (view.kind !== 'stash') return
  runtimeByView[view.id].issues = view.issues
  runtimeByView[view.id].count = view.issues.length
  runtimeByView[view.id].initialized = true
  runtimeByView[view.id].updatedAt = null
}

/**
 * 清理当前列表中已经不存在的筛选值
 * @param viewId - 视图标识
 */
function normalizeViewFilters(viewId: string): void {
  const issues = runtimeByView[viewId]?.issues ?? [] // 当前视图完整列表
  const versions = new Set(issues.flatMap((issue) => issue.versions ?? [])) // 可用版本
  const platforms = new Set(issues.flatMap((issue) => issue.platforms ?? [])) // 可用平台
  if (versionFilterByView[viewId] && !versions.has(versionFilterByView[viewId])) versionFilterByView[viewId] = ''
  if (platformFilterByView[viewId] && !platforms.has(platformFilterByView[viewId])) platformFilterByView[viewId] = ''
}

/**
 * 更新本地暂存视图配置和运行状态
 * @param stashView - 暂存视图
 */
function applyStashView(stashView: IssueView): void {
  const index = config.value.views.findIndex((view) => view.id === stashView.id) // 暂存视图索引
  if (index === -1) config.value.views.push(stashView)
  else config.value.views[index] = stashView
  syncViewRuntime(stashView)
}

/**
 * 获取当前已暂存的问题单标识
 * @returns 暂存问题单 Key 集合
 */
function getStashedIssueKeys(): Set<string> {
  const stashView = config.value.views.find((view) => view.kind === 'stash') // 暂存视图
  return new Set(stashView?.issues.map((issue) => issue.key) ?? [])
}

/**
 * 从所有JIRA视图运行状态中移除已暂存问题单
 * @param issueKey - 问题单 Key
 */
function removeIssueFromJiraRuntimes(issueKey: string): void {
  config.value.views.filter((view) => view.kind === 'jira').forEach((view) => {
    const runtime = runtimeByView[view.id]
    if (!runtime) return
    runtime.issues = runtime.issues.filter((issue) => issue.key !== issueKey)
    runtime.count = runtime.issues.length
    normalizeViewFilters(view.id)
  })
}

/**
 * 刷新全部JIRA视图
 */
async function refreshJiraViews(): Promise<void> {
  await Promise.all(config.value.views.filter((view) => view.kind === 'jira').map((view) => refreshView(view.id)))
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
  config.value.views.filter((view) => view.kind === 'jira').forEach((view) => {
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
      if (nextConfig.views.some((view) => view.id === viewId)) return
      delete runtimeByView[viewId]
      delete versionFilterByView[viewId]
      delete platformFilterByView[viewId]
    })

    nextConfig.views.forEach(syncViewRuntime)

    if (activeViewId.value && !nextConfig.views.some((view) => view.id === activeViewId.value)) {
      activeViewId.value = null
    }

    scheduleRefreshes()
    await nextTick()
    await resizeForCurrentState()
    await Promise.all(nextConfig.views.filter((view) => view.kind === 'jira').map((view) => refreshView(view.id)))
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
  const view = config.value.views.find((item) => item.id === viewId) // 当前视图配置
  if (!runtime || !view || view.kind !== 'jira' || runtime.loading) return

  runtime.loading = true
  runtime.error = ''

  try {
    const result = await fetchIssues(viewId)
    const stashedKeys = getStashedIssueKeys() // 需要从JIRA视图隐藏的本地问题单
    const visibleIssues = result.issues.filter((issue) => !stashedKeys.has(issue.key)) // 过滤已暂存问题单
    const knownIssueKeys = new Set(runtime.issues.map((issue) => issue.key)) // 刷新前的问题单标识
    const newIssues = visibleIssues.filter((issue) => !knownIssueKeys.has(issue.key)) // 新增问题单
    const addedCount = Math.max(visibleIssues.length - runtime.count, newIssues.length) // 新增问题单数量
    const hasAddedIssue = addedCount > 0 // 是否存在新增问题单
    if (runtime.initialized && activeViewId.value !== viewId && hasAddedIssue) {
      runtime.hasNewIssues = true
      void notifyNewIssues(result.viewName, addedCount, newIssues)
    }
    runtime.count = visibleIssues.length
    runtime.issues = visibleIssues
    normalizeViewFilters(viewId)
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
  if (activeView.value?.kind === 'jira') await refreshView(activeView.value.id)
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
 * 打开AI提示词窗口
 * @param issueKey - 问题单 Key
 */
async function handleOpenAi(issueKey: string): Promise<void> {
  try {
    await openAiChatWindow(issueKey)
  } catch (error) {
    if (activeRuntime.value) activeRuntime.value.error = String(error)
  }
}

/**
 * 将问题单加入持久化暂存视图
 * @param issueKey - 问题单 Key
 */
async function stashIssue(issueKey: string): Promise<void> {
  if (!activeRuntime.value) return
  const issue = activeRuntime.value.issues.find((item) => item.key === issueKey) // 待暂存问题单
  if (!issue) return
  try {
    applyStashView(await persistStashedIssue(issue))
    removeIssueFromJiraRuntimes(issueKey)
    if (activeViewId.value && runtimeByView[activeViewId.value]) {
      runtimeByView[activeViewId.value].hasNewIssues = false
    }
    await nextTick()
    await resizeForCurrentState()
  } catch (error) {
    activeRuntime.value.error = String(error)
  }
}

/**
 * 在鼠标位置弹出问题单右键菜单
 * @param event - 鼠标事件
 * @param issue - 问题单
 */
function handleIssueContextMenu(event: MouseEvent, issue: IssueItem): void {
  const menuWidth = 178 // 菜单宽度
  const menuHeight = 116 // 菜单高度
  contextMenu.x = Math.min(event.clientX, window.innerWidth - menuWidth - 8)
  contextMenu.y = Math.min(event.clientY, window.innerHeight - menuHeight - 8)
  contextMenu.issueKey = issue.key
  contextMenu.visible = true
}

/**
 * 关闭问题单右键菜单
 */
function closeContextMenu(): void {
  contextMenu.visible = false
}

/**
 * 处理全局按键，Esc 关闭右键菜单与备注弹层
 * @param event - 键盘事件
 */
function handleGlobalKeydown(event: KeyboardEvent): void {
  if (event.key !== 'Escape') return
  closeContextMenu()
  noteEditor.visible = false
}

/**
 * 打开备注编辑弹层
 */
function openNoteEditor(): void {
  noteEditor.issueKey = contextMenu.issueKey
  noteEditor.text = config.value.notes[contextMenu.issueKey] ?? ''
  noteEditor.visible = true
  closeContextMenu()
}

/**
 * 保存备注并同步到本地配置
 */
async function saveNote(): Promise<void> {
  try {
    config.value.notes = await saveIssueNote(noteEditor.issueKey, noteEditor.text)
    noteEditor.visible = false
  } catch (error) {
    if (activeRuntime.value) activeRuntime.value.error = String(error)
  }
}

/**
 * 执行右键菜单的暂存或移出暂存
 */
function handleContextStash(): void {
  const issueKey = contextMenu.issueKey // 菜单指向的问题单
  closeContextMenu()
  if (activeView.value?.kind === 'stash') void unstashIssue(issueKey)
  else void stashIssue(issueKey)
}

/**
 * 执行右键菜单的AI提示词生成
 */
function handleContextAi(): void {
  const issueKey = contextMenu.issueKey // 菜单指向的问题单
  closeContextMenu()
  void handleOpenAi(issueKey)
}

/**
 * 取消暂存问题单
 * @param issueKey - 问题单 Key
 */
async function unstashIssue(issueKey: string): Promise<void> {
  try {
    applyStashView(await persistUnstashedIssue(issueKey))
    await refreshJiraViews()
  } catch (error) {
    if (activeRuntime.value) activeRuntime.value.error = String(error)
  }
}

/**
 * 清空所有暂存问题单
 */
async function clearAllStashed(): Promise<void> {
  try {
    applyStashView(await clearStashedIssues())
    await refreshJiraViews()
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
  window.addEventListener('pointerdown', closeContextMenu)
  window.addEventListener('keydown', handleGlobalKeydown)
})

onBeforeUnmount(() => {
  clearRefreshTimers()
  unlistenConfig?.()
  window.removeEventListener('pointermove', handlePointerMove)
  window.removeEventListener('pointerup', handlePointerUp)
  window.removeEventListener('pointerdown', closeContextMenu)
  window.removeEventListener('keydown', handleGlobalKeydown)
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
          <button v-if="activeView.kind === 'jira'" class="icon-button" type="button" title="刷新" aria-label="刷新" :disabled="activeRuntime.loading" @click="refreshView(activeView.id)">
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

        <template v-else>
          <div v-if="activeView.kind === 'jira' && activeRuntime.issues.length > 0" class="filter-toolbar">
            <div class="filter-group" aria-label="按版本筛选">
              <span class="filter-group__label">版本</span>
              <button
                class="filter-tag"
                :class="{ 'filter-tag--active': !activeVersionFilter }"
                type="button"
                @click="activeVersionFilter = ''"
              >全部</button>
              <button
                v-for="version in activeVersionOptions"
                :key="version"
                class="filter-tag"
                :class="{ 'filter-tag--active': activeVersionFilter === version }"
                type="button"
                @click="toggleVersionFilter(version)"
              >{{ version }}</button>
            </div>
            <div class="filter-group" aria-label="按平台筛选">
              <span class="filter-group__label">平台</span>
              <button
                class="filter-tag"
                :class="{ 'filter-tag--active': !activePlatformFilter }"
                type="button"
                @click="activePlatformFilter = ''"
              >全部</button>
              <button
                v-for="platform in activePlatformOptions"
                :key="platform"
                class="filter-tag"
                :class="{ 'filter-tag--active': activePlatformFilter === platform }"
                type="button"
                @click="togglePlatformFilter(platform)"
              >{{ platform }}</button>
            </div>
            <span>{{ activeVisibleIssues.length }} / {{ activeRuntime.issues.length }}</span>
          </div>

          <div v-if="activeView.kind === 'stash' && activeRuntime.issues.length > 0" class="stash-toolbar">
            <span>本地暂存 {{ activeRuntime.issues.length }} 条</span>
            <button class="text-button" type="button" @click="clearAllStashed">清空暂存</button>
          </div>

          <div v-if="activeVisibleIssues.length === 0" class="panel-state panel-state--success">
            <span class="status-dot" />
            <span>{{ activeView.kind === 'stash' ? '暂存视图为空' : activeRuntime.issues.length > 0 ? '没有符合筛选条件的问题单' : '当前没有符合条件的问题单' }}</span>
          </div>

          <button
            v-for="issue in activeVisibleIssues"
            :key="issue.key"
            class="bug-row"
            :style="getProjectStyle(issue.projectKey)"
            type="button"
            @click="handleOpenExternal(issue.link)"
            @contextmenu.prevent="handleIssueContextMenu($event, issue)"
          >
            <span class="bug-row__main">
              <strong><span class="issue-key">{{ issue.key }}</span>{{ issue.title }}</strong>
              <span class="bug-row__meta">
                <span v-if="config.notes[issue.key]" class="note-tag" :title="config.notes[issue.key]">
                  <StickyNote :size="11" />备注
                </span>
                <span class="project-tag" :title="issue.projectName">{{ issue.projectKey }}</span>
                <span v-if="issue.issueType">{{ issue.issueType }}</span>
                <span v-if="issue.status">{{ issue.status }}</span>
                <span v-if="issue.priority">{{ issue.priority }}</span>
                <span v-for="version in issue.versions ?? []" :key="`version-${version}`">{{ version }}</span>
                <span v-for="platform in issue.platforms ?? []" :key="`platform-${platform}`">{{ platform }}</span>
              </span>
            </span>
            <span v-if="activeView.kind === 'jira'" class="stash-action" title="暂存问题单" role="button" tabindex="0" @click.stop="stashIssue(issue.key)" @keydown.enter.stop="stashIssue(issue.key)">
              <Archive :size="15" />
            </span>
            <span v-else class="unstash-btn" role="button" tabindex="0" @click.stop="unstashIssue(issue.key)" @keydown.enter.stop="unstashIssue(issue.key)">移出暂存</span>
            <span class="ai-action" title="生成AI提示词" role="button" tabindex="0" @click.stop="handleOpenAi(issue.key)" @keydown.enter.stop="handleOpenAi(issue.key)">
              <Sparkles :size="15" />
            </span>
          </button>
        </template>
      </div>
    </section>

    <div
      v-if="contextMenu.visible"
      class="context-menu"
      :style="{ left: `${contextMenu.x}px`, top: `${contextMenu.y}px` }"
      @pointerdown.stop
    >
      <button class="context-menu__item" type="button" @click="openNoteEditor">
        <StickyNote :size="15" />备注
      </button>
      <button class="context-menu__item" type="button" @click="handleContextStash">
        <Archive v-if="activeView?.kind !== 'stash'" :size="15" />
        <Trash2 v-else :size="15" />
        {{ activeView?.kind === 'stash' ? '移出暂存' : '暂存问题单' }}
      </button>
      <button class="context-menu__item" type="button" @click="handleContextAi">
        <Sparkles :size="15" />生成AI提示词
      </button>
    </div>

    <div v-if="noteEditor.visible" class="note-overlay" @pointerdown.self="noteEditor.visible = false">
      <section class="note-dialog">
        <header class="note-dialog__header">
          <strong>问题单备注</strong>
          <button class="icon-button" type="button" title="关闭" @click="noteEditor.visible = false">
            <X :size="16" />
          </button>
        </header>
        <span class="note-dialog__key">{{ noteEditor.issueKey }}</span>
        <textarea
          v-model="noteEditor.text"
          class="note-dialog__input"
          rows="6"
          maxlength="2000"
          placeholder="记录补充信息、复现细节或处理进度，生成的AI提示词会带上这段备注"
        />
        <footer class="note-dialog__footer">
          <button class="text-button" type="button" @click="noteEditor.visible = false">取消</button>
          <button class="command-button command-button--primary" type="button" @click="saveNote">
            <Check :size="15" />保存备注
          </button>
        </footer>
      </section>
    </div>
  </main>
</template>
