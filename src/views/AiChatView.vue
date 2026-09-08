<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { Check, Copy, ExternalLink, RefreshCw, Sparkles, X } from '@lucide/vue'
import { closeAiChatWindow, fetchIssueDetail, getPendingAiIssue, openExternal } from '../api'
import type { IssueDetail } from '../types'

const issueKey = ref('') // 当前问题单 Key
const loading = ref(false) // 生成中
const errorMessage = ref('') // 错误信息
const detail = ref<IssueDetail | null>(null) // 问题单详情
const promptText = ref('') // 生成的提示词
const copied = ref(false) // 是否已复制
let copyTimer = 0 // 复制状态复位定时器
let unlistenIssue: UnlistenFn | null = null // 问题单事件解绑函数

/**
 * 从问题描述中拆出细节、复现、期望和实际结果
 * @param description - 纯文本描述
 */
function parseDescriptionSections(description: string): { detail: string; repro: string; expected: string; actual: string } {
  const result = { detail: '', repro: '', expected: '', actual: '' }
  if (!description.trim()) return result

  const headingMap = [
    { key: 'detail' as const, pattern: /^(问题描述|问题详情|缺陷描述|现象描述|问题细节)$/ },
    { key: 'repro' as const, pattern: /^(复现步骤|复现方式|重现步骤|重现方式|操作步骤|再现步骤)$/ },
    { key: 'expected' as const, pattern: /^(期望结果|预期结果|期望行为|期望效果|期望)$/ },
    { key: 'actual' as const, pattern: /^(实际结果|实际行为|实际效果|实际现象|实际)$/ },
  ]
  const buckets = { detail: [] as string[], repro: [] as string[], expected: [] as string[], actual: [] as string[] }
  let current: keyof typeof buckets = 'detail'
  let matchedHeading = false

  for (const rawLine of description.split('\n')) {
    const heading = rawLine
      .replace(/^#{1,6}\s*/, '')
      .replace(/^\d+[\.、:：]\s*/, '')
      .replace(/^[【\[]/, '')
      .replace(/[】\]]$/, '')
      .replace(/[:：]\s*$/, '')
      .trim()
    const matched = headingMap.find((item) => item.pattern.test(heading))
    if (matched && heading.length <= 12) {
      current = matched.key
      matchedHeading = true
      continue
    }
    buckets[current].push(rawLine)
  }

  if (!matchedHeading) {
    result.detail = description.trim()
    return result
  }

  result.detail = buckets.detail.join('\n').trim()
  result.repro = buckets.repro.join('\n').trim()
  result.expected = buckets.expected.join('\n').trim()
  result.actual = buckets.actual.join('\n').trim()
  return result
}

/**
 * 根据问题单详情生成交给AI改代码的提示词
 * @param issue - 问题单详情
 * @returns 提示词文本
 */
function buildAiPrompt(issue: IssueDetail): string {
  const sections = parseDescriptionSections(issue.description) // 描述分段
  const lines = [
    '请根据下面的 JIRA 问题单修改代码。先定位相关实现，再给出可以直接落地的改动，不要改无关模块。',
    '',
    '## 基本信息',
    `- 问题单：${issue.key}`,
    `- 标题：${issue.title}`,
    `- 链接：${issue.link}`,
    `- 项目：${issue.projectKey}${issue.projectName ? `（${issue.projectName}）` : ''}`,
  ]

  if (issue.issueType) lines.push(`- 类型：${issue.issueType}`)
  if (issue.status) lines.push(`- 状态：${issue.status}`)
  if (issue.priority) lines.push(`- 优先级：${issue.priority}`)
  if (issue.versions.length > 0) lines.push(`- 版本：${issue.versions.join('、')}`)
  if (issue.platforms.length > 0) lines.push(`- 平台：${issue.platforms.join('、')}`)
  if (issue.reporter) lines.push(`- 报告人：${issue.reporter}`)
  if (issue.components.length > 0) lines.push(`- 组件：${issue.components.join('、')}`)
  if (issue.labels.length > 0) lines.push(`- 标签：${issue.labels.join('、')}`)

  lines.push('', '## 问题细节')
  lines.push(sections.detail || '问题单没有填写描述，请以标题和基本信息为准。')

  if (sections.repro) {
    lines.push('', '## 复现方式')
    lines.push(sections.repro)
  }
  if (sections.expected) {
    lines.push('', '## 期望结果')
    lines.push(sections.expected)
  }
  if (sections.actual) {
    lines.push('', '## 实际结果')
    lines.push(sections.actual)
  }
  if (issue.environment) {
    lines.push('', '## 环境')
    lines.push(issue.environment)
  }
  if (issue.images.length > 0) {
    const names = issue.images.map((image) => image.filename).join('、') // 截图文件名
    lines.push('', '## 截图')
    lines.push(`问题单中共 ${issue.images.length} 张截图：${names}。请结合这些截图中的界面、文案和操作路径理解问题。`)
  }

  lines.push('', '## 修改要求')
  lines.push('1. 按复现方式和截图理解缺陷，不要臆造未出现的步骤')
  lines.push('2. 给出涉及的文件、修改原因和具体改法')
  lines.push('3. 说明如何按复现方式验证')
  return lines.join('\n')
}

/**
 * 加载问题单并生成提示词
 * @param key - 问题单 Key
 */
async function loadIssue(key: string): Promise<void> {
  issueKey.value = key
  loading.value = true
  errorMessage.value = ''
  detail.value = null
  promptText.value = ''
  copied.value = false

  try {
    const issue = await fetchIssueDetail(key)
    detail.value = issue
    promptText.value = buildAiPrompt(issue)
  } catch (error) {
    errorMessage.value = String(error)
  } finally {
    loading.value = false
  }
}

/**
 * 复制已生成的提示词
 */
async function copyPrompt(): Promise<void> {
  if (!promptText.value) return
  try {
    await navigator.clipboard.writeText(promptText.value)
    copied.value = true
    window.clearTimeout(copyTimer)
    copyTimer = window.setTimeout(() => {
      copied.value = false
    }, 2000)
  } catch (error) {
    errorMessage.value = `复制失败：${String(error)}`
  }
}

/**
 * 打开当前问题单的JIRA页面
 */
async function openIssueLink(): Promise<void> {
  if (!detail.value) return
  try {
    await openExternal(detail.value.link)
  } catch (error) {
    errorMessage.value = String(error)
  }
}

onMounted(async () => {
  unlistenIssue = await listen<string>('ai-issue-open', (event) => {
    void loadIssue(event.payload)
  })
  const pending = await getPendingAiIssue()
  if (pending) await loadIssue(pending)
})

onBeforeUnmount(() => {
  unlistenIssue?.()
  window.clearTimeout(copyTimer)
})
</script>

<template>
  <main class="ai-chat">
    <header class="ai-chat__header">
      <div class="ai-chat__heading">
        <Sparkles :size="18" />
        <div>
          <strong>{{ issueKey || 'AI 提示词' }}</strong>
          <span>{{ detail ? detail.title : '从问题单生成给 AI 改代码的文本' }}</span>
        </div>
      </div>
      <button class="icon-button" type="button" title="关闭" aria-label="关闭" @click="closeAiChatWindow">
        <X :size="18" />
      </button>
    </header>

    <section class="ai-chat__thread" aria-label="提示词对话">
      <div v-if="!issueKey && !loading" class="panel-state">
        <Sparkles :size="22" />
        <span>点击问题单上的 AI 图标开始生成</span>
      </div>

      <template v-else>
        <article class="chat-msg chat-msg--user">
          <span>你</span>
          <p>根据 {{ issueKey }} 生成一段给 AI 改代码的提示词，带上问题细节、复现方式和截图。</p>
        </article>

        <article v-if="loading" class="chat-msg chat-msg--assistant">
          <span>FuckTheBug</span>
          <p class="chat-msg__loading">
            <RefreshCw class="spinning" :size="16" />
            正在读取问题单并识别截图
          </p>
        </article>

        <article v-else-if="errorMessage && !promptText" class="chat-msg chat-msg--assistant chat-msg--error">
          <span>FuckTheBug</span>
          <p>{{ errorMessage }}</p>
          <button class="text-button" type="button" @click="loadIssue(issueKey)">重试</button>
        </article>

        <article v-else-if="detail" class="chat-msg chat-msg--assistant">
          <span>FuckTheBug</span>
          <p v-if="errorMessage" class="form-message form-message--error">{{ errorMessage }}</p>
          <div v-if="detail.images.length > 0" class="chat-images">
            <figure v-for="image in detail.images" :key="image.filename">
              <img :src="image.dataUrl" :alt="image.filename" />
              <figcaption>{{ image.filename }}</figcaption>
            </figure>
          </div>
          <p v-if="detail.failedImages.length > 0" class="form-message form-message--error">
            以下截图未能读取：{{ detail.failedImages.join('、') }}
          </p>
          <pre class="chat-prompt">{{ promptText }}</pre>
        </article>
      </template>
    </section>

    <footer class="ai-chat__footer">
      <button class="command-button command-button--secondary" type="button" :disabled="!detail" @click="openIssueLink">
        <ExternalLink :size="16" />
        打开问题单
      </button>
      <button class="command-button command-button--secondary" type="button" :disabled="!issueKey || loading" @click="loadIssue(issueKey)">
        <RefreshCw :class="{ spinning: loading }" :size="16" />
        重新生成
      </button>
      <button class="command-button command-button--primary" type="button" :disabled="!promptText" @click="copyPrompt">
        <Check v-if="copied" :size="16" />
        <Copy v-else :size="16" />
        {{ copied ? '已复制' : '复制文本' }}
      </button>
    </footer>
  </main>
</template>
