<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { getVersion } from '@tauri-apps/api/app'
import { relaunch } from '@tauri-apps/plugin-process'
import { check as checkForUpdate } from '@tauri-apps/plugin-updater'
import { CheckCircle2, Download, Eye, EyeOff, KeyRound, Plus, RefreshCw, Save, Trash2, Unplug, X } from '@lucide/vue'
import { closeSettingsWindow, getConfig, saveConfig, testJiraConnection } from '../api'
import type { AppConfig, IssueView } from '../types'

const config = ref<AppConfig>({
  jira: { baseUrl: '', refreshInterval: 1, token: '', hasToken: false, clearToken: false },
  views: [],
}) // 表单配置
const loading = ref(true) // 加载状态
const saving = ref(false) // 保存状态
const testing = ref(false) // 连接测试状态
const tokenVisible = ref(false) // Token可见状态
const message = ref('') // 操作提示
const errorMessage = ref('') // 错误提示
const appVersion = ref('') // 当前应用版本
const availableVersion = ref('') // 可更新版本
const updateMessage = ref('') // 更新提示
const updateError = ref('') // 更新错误
const checkingUpdate = ref(false) // 检查更新状态
const installingUpdate = ref(false) // 安装更新状态
const updateProgress = ref(0) // 更新下载进度
let pendingUpdate: Awaited<ReturnType<typeof checkForUpdate>> = null // 待安装更新

const updateButtonLabel = computed(() => {
  if (installingUpdate.value) return updateProgress.value > 0 ? `正在下载 ${updateProgress.value}%` : '正在准备更新'
  if (checkingUpdate.value) return '正在检查'
  if (availableVersion.value) return `更新到 v${availableVersion.value}`
  return '检查更新'
})

/**
 * 加载设置表单
 */
async function loadConfig(): Promise<void> {
  loading.value = true
  errorMessage.value = ''
  try {
    config.value = await getConfig()
  } catch (error) {
    errorMessage.value = String(error)
  } finally {
    loading.value = false
  }
}

/**
 * 读取当前应用版本
 */
async function loadAppVersion(): Promise<void> {
  try {
    appVersion.value = await getVersion()
  } catch (error) {
    updateError.value = `无法读取应用版本：${String(error)}`
  }
}

/**
 * 检查GitHub Release更新
 */
async function handleCheckUpdate(): Promise<void> {
  if (checkingUpdate.value || installingUpdate.value) return
  checkingUpdate.value = true
  message.value = ''
  errorMessage.value = ''
  updateMessage.value = ''
  updateError.value = ''

  try {
    pendingUpdate = await checkForUpdate()
    if (!pendingUpdate) {
      availableVersion.value = ''
      updateMessage.value = '当前已是最新版本'
      return
    }
    availableVersion.value = pendingUpdate.version
    updateMessage.value = `发现新版本 v${pendingUpdate.version}`
  } catch (error) {
    updateError.value = `检查更新失败：${String(error)}`
  } finally {
    checkingUpdate.value = false
  }
}

/**
 * 下载并安装已发现的签名更新
 */
async function handleInstallUpdate(): Promise<void> {
  if (!pendingUpdate || installingUpdate.value) return
  installingUpdate.value = true
  message.value = ''
  errorMessage.value = ''
  updateProgress.value = 0
  updateMessage.value = `正在下载 v${pendingUpdate.version}`
  updateError.value = ''
  let downloaded = 0 // 已下载字节数
  let contentLength = 0 // 更新包总字节数

  try {
    await pendingUpdate.downloadAndInstall((event) => {
      if (event.event === 'Started') {
        contentLength = event.data.contentLength ?? 0
      } else if (event.event === 'Progress') {
        downloaded += event.data.chunkLength
        if (contentLength > 0) updateProgress.value = Math.min(99, Math.round(downloaded / contentLength * 100))
      } else if (event.event === 'Finished') {
        updateProgress.value = 100
      }
    })
    updateMessage.value = '更新已安装，正在重启'
    await relaunch()
  } catch (error) {
    updateError.value = `安装更新失败：${String(error)}`
    installingUpdate.value = false
  }
}

/**
 * 根据更新状态执行检查或安装
 */
async function handleUpdateAction(): Promise<void> {
  if (pendingUpdate) {
    await handleInstallUpdate()
    return
  }
  await handleCheckUpdate()
}

/**
 * 添加一个JQL视图
 */
function addView(): void {
  config.value.views.push({
    id: crypto.randomUUID(),
    name: '',
    jql: 'assignee = currentUser() AND resolution = Unresolved ORDER BY priority DESC, updated DESC',
  })
}

/**
 * 删除JQL视图
 * @param index - 视图索引
 */
function removeView(index: number): void {
  const view = config.value.views[index] // 待删除视图
  if (!window.confirm(`确定删除“${view.name || `问题单视图 #${index + 1}`}”吗？`)) return
  config.value.views.splice(index, 1)
}

/**
 * 标记清除已保存Token
 */
function clearToken(): void {
  config.value.jira.token = ''
  config.value.jira.hasToken = false
  config.value.jira.clearToken = true
  message.value = '保存后将清除系统凭据中的Token'
}

/**
 * 校验设置表单
 * @returns 错误信息，空字符串表示通过
 */
function validateForm(): string {
  try {
    const url = new URL(config.value.jira.baseUrl)
    if (!['http:', 'https:'].includes(url.protocol)) return 'JIRA地址仅支持HTTP或HTTPS'
  } catch {
    return 'JIRA地址格式不正确'
  }

  if (!config.value.jira.hasToken && !config.value.jira.token && !config.value.jira.clearToken) {
    return '请输入JIRA Token'
  }
  if (config.value.jira.refreshInterval < 0.1 || config.value.jira.refreshInterval > 1440) {
    return '刷新间隔必须在0.1到1440分钟之间'
  }
  if (config.value.views.length === 0) return '请至少添加一个问题单视图'

  for (const [index, view] of config.value.views.entries()) {
    if (!view.name.trim()) return `问题单视图 #${index + 1} 缺少名称`
    if (!view.jql.trim()) return `问题单视图 #${index + 1} 缺少JQL`
  }

  return ''
}

/**
 * 测试JIRA连接
 */
async function handleTestConnection(): Promise<void> {
  if (!config.value.jira.baseUrl.trim()) {
    errorMessage.value = '请输入JIRA地址'
    return
  }
  if (!config.value.jira.hasToken && !config.value.jira.token) {
    errorMessage.value = '请输入JIRA Token'
    return
  }

  testing.value = true
  message.value = ''
  errorMessage.value = ''
  updateMessage.value = ''
  updateError.value = ''
  try {
    const user = await testJiraConnection(config.value.jira.baseUrl, config.value.jira.token)
    message.value = `连接成功：${user.displayName}`
  } catch (error) {
    errorMessage.value = String(error)
  } finally {
    testing.value = false
  }
}

/**
 * 保存设置
 */
async function handleSave(): Promise<void> {
  const validationError = validateForm() // 表单错误
  if (validationError) {
    errorMessage.value = validationError
    return
  }

  saving.value = true
  message.value = ''
  errorMessage.value = ''
  updateMessage.value = ''
  updateError.value = ''

  try {
    config.value = await saveConfig(config.value)
    message.value = '配置已保存'
    window.setTimeout(() => void closeSettingsWindow(), 500)
  } catch (error) {
    errorMessage.value = String(error)
  } finally {
    saving.value = false
  }
}

onMounted(() => void Promise.all([loadConfig(), loadAppVersion()]))
</script>

<template>
  <main class="settings-view">
    <header class="settings-header">
      <div>
        <p class="eyebrow">FUCK THE BUG</p>
        <h1>问题单追踪设置</h1>
      </div>
      <button class="icon-button" type="button" title="关闭" aria-label="关闭设置" @click="closeSettingsWindow">
        <X :size="20" />
      </button>
    </header>

    <div class="settings-toolbar">
      <span>{{ config.views.length }} 个问题单视图</span>
      <button class="command-button command-button--secondary" type="button" @click="addView">
        <Plus :size="17" />
        添加视图
      </button>
    </div>

    <section v-if="loading" class="settings-state">正在读取配置</section>

    <section v-else class="settings-content">
      <div class="connection-panel">
        <header class="connection-panel__header">
          <div>
            <strong>JIRA连接</strong>
            <span :class="{ 'connection-status--ready': config.jira.hasToken }" class="connection-status">
              {{ config.jira.hasToken ? 'Token已安全保存' : 'Token未保存' }}
            </span>
          </div>
          <button class="command-button command-button--secondary" type="button" :disabled="testing" @click="handleTestConnection">
            <Unplug :size="16" />
            {{ testing ? '正在测试' : '测试连接' }}
          </button>
        </header>

        <div class="form-grid form-grid--connection">
          <label class="field field--wide">
            <span>JIRA地址</span>
            <input v-model.trim="config.jira.baseUrl" type="url" placeholder="https://jira.example.com" />
          </label>

          <label class="field">
            <span>访问Token</span>
            <span class="password-input">
              <input
                v-model="config.jira.token"
                :type="tokenVisible ? 'text' : 'password'"
                :placeholder="config.jira.hasToken ? '已安全保存，留空保持不变' : '输入Personal Access Token'"
                autocomplete="new-password"
                @input="config.jira.clearToken = false"
              />
              <button type="button" :title="tokenVisible ? '隐藏Token' : '显示Token'" :aria-label="tokenVisible ? '隐藏Token' : '显示Token'" @click="tokenVisible = !tokenVisible">
                <EyeOff v-if="tokenVisible" :size="16" />
                <Eye v-else :size="16" />
              </button>
            </span>
            <button v-if="config.jira.hasToken" class="clear-secret" type="button" @click="clearToken">
              <KeyRound :size="14" />
              清除已保存Token
            </button>
          </label>

          <label class="field">
            <span>刷新间隔（分钟）</span>
            <input v-model.number="config.jira.refreshInterval" min="0.1" max="1440" step="0.1" type="number" />
          </label>
        </div>
      </div>

      <div class="feed-list">
        <article v-for="(view, index) in config.views" :key="view.id" class="feed-editor">
          <header class="feed-editor__header">
            <div>
              <span class="feed-editor__index">{{ String(index + 1).padStart(2, '0') }}</span>
              <strong>{{ view.name || '未命名问题单视图' }}</strong>
            </div>
            <button class="icon-button icon-button--danger" type="button" title="删除" aria-label="删除问题单视图" @click="removeView(index)">
              <Trash2 :size="17" />
            </button>
          </header>

          <div class="form-grid form-grid--view">
            <label class="field">
              <span>名称</span>
              <input v-model.trim="view.name" maxlength="40" type="text" placeholder="例如：我的未解决问题单" />
            </label>

            <label class="field field--wide">
              <span>JQL</span>
              <textarea v-model.trim="view.jql" maxlength="2000" rows="3" spellcheck="false" />
            </label>
          </div>
        </article>

        <div v-if="config.views.length === 0" class="settings-state settings-state--compact">
          <span>还没有问题单视图</span>
          <button class="text-button" type="button" @click="addView">添加第一个</button>
        </div>
      </div>
    </section>

    <footer class="settings-footer">
      <div class="settings-footer__status">
        <span class="app-version">FuckTheBug v{{ appVersion || '--' }}</span>
        <p v-if="errorMessage" class="form-message form-message--error">{{ errorMessage }}</p>
        <p v-else-if="updateError" class="form-message form-message--error">{{ updateError }}</p>
        <p v-else-if="message" class="form-message form-message--success">
          <CheckCircle2 :size="15" />
          {{ message }}
        </p>
        <p v-else-if="updateMessage" class="form-message form-message--success">
          <CheckCircle2 :size="15" />
          {{ updateMessage }}
        </p>
      </div>
      <div class="settings-footer__actions">
        <button class="command-button command-button--secondary update-button" type="button" :disabled="checkingUpdate || installingUpdate" @click="handleUpdateAction">
          <Download v-if="availableVersion" :size="16" />
          <RefreshCw v-else :class="{ spinning: checkingUpdate }" :size="16" />
          {{ updateButtonLabel }}
        </button>
        <button class="command-button command-button--secondary" type="button" @click="closeSettingsWindow">取消</button>
        <button class="command-button command-button--primary" type="button" :disabled="saving" @click="handleSave">
          <Save :size="17" />
          {{ saving ? '正在保存' : '保存配置' }}
        </button>
      </div>
    </footer>
  </main>
</template>
