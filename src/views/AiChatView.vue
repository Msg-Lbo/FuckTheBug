<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { Check, Copy, ExternalLink, RefreshCw, Sparkles, X } from '@lucide/vue'
import { closeAiChatWindow, generateAiPrompt, getPendingAiIssue, openExternal } from '../api'
import type { IssueDetail } from '../types'

const issueKey = ref('') // 当前问题单 Key
const loading = ref(false) // 生成中
const errorMessage = ref('') // 错误信息
const detail = ref<IssueDetail | null>(null) // 问题单详情
const promptText = ref('') // 模型生成的提示词
const copied = ref(false) // 是否已复制
const threadRef = ref<HTMLElement | null>(null) // 对话滚动容器
let copyTimer = 0 // 复制状态复位定时器
let loadSeq = 0 // 当前加载序号
let activeStreamId = 0 // 当前流式生成标识
let unlistenIssue: UnlistenFn | null = null // 问题单事件解绑函数
let unlistenStart: UnlistenFn | null = null // 流开始事件解绑函数
let unlistenContext: UnlistenFn | null = null // 问题单详情事件解绑函数
let unlistenChunk: UnlistenFn | null = null // 流片段事件解绑函数

/**
 * 将对话滚到最新输出
 */
function scrollThread(): void {
  const thread = threadRef.value
  if (!thread) return
  thread.scrollTop = thread.scrollHeight
}

/**
 * 加载问题单并让模型流式生成提示词
 * @param key - 问题单 Key
 */
async function loadIssue(key: string): Promise<void> {
  const seq = ++loadSeq // 本次加载序号
  issueKey.value = key
  loading.value = true
  errorMessage.value = ''
  detail.value = null
  promptText.value = ''
  copied.value = false

  try {
    await generateAiPrompt(key)
  } catch (error) {
    if (seq !== loadSeq) return
    const message = String(error)
    if (message.includes('生成已被取消')) return
    errorMessage.value = message
  } finally {
    if (seq === loadSeq) loading.value = false
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
  unlistenStart = await listen<{ streamId: number }>('ai-prompt-start', (event) => {
    activeStreamId = event.payload.streamId
    promptText.value = ''
  })
  unlistenContext = await listen<IssueDetail>('ai-prompt-context', (event) => {
    if (event.payload.key !== issueKey.value) return
    detail.value = event.payload
    void nextTick(scrollThread)
  })
  unlistenChunk = await listen<{ streamId: number; text: string }>('ai-prompt-chunk', (event) => {
    if (event.payload.streamId !== activeStreamId) return
    promptText.value += event.payload.text
    void nextTick(scrollThread)
  })
  unlistenIssue = await listen<string>('ai-issue-open', (event) => {
    void loadIssue(event.payload)
  })
  const pending = await getPendingAiIssue()
  if (pending) await loadIssue(pending)
})

onBeforeUnmount(() => {
  unlistenIssue?.()
  unlistenStart?.()
  unlistenContext?.()
  unlistenChunk?.()
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
          <span>{{ detail ? detail.title : '把问题单交给模型，流式生成改代码提示词' }}</span>
        </div>
      </div>
      <button class="icon-button" type="button" title="关闭" aria-label="关闭" @click="closeAiChatWindow">
        <X :size="18" />
      </button>
    </header>

    <section ref="threadRef" class="ai-chat__thread" aria-label="提示词对话">
      <div v-if="!issueKey && !loading" class="panel-state">
        <Sparkles :size="22" />
        <span>点击问题单上的 AI 图标开始生成</span>
      </div>

      <template v-else>
        <article class="chat-msg chat-msg--user">
          <span>你</span>
          <p>根据 {{ issueKey }} 的正文和截图，生成一段给编程 AI 改代码的提示词。</p>
        </article>

        <article class="chat-msg chat-msg--assistant" :class="{ 'chat-msg--error': errorMessage && !promptText }">
          <span>模型</span>
          <div v-if="detail?.images.length" class="chat-images">
            <figure v-for="image in detail.images" :key="image.filename">
              <img :src="image.dataUrl" :alt="image.filename" />
              <figcaption>{{ image.filename }}</figcaption>
            </figure>
          </div>
          <p v-if="detail?.failedImages.length" class="form-message form-message--error">
            以下截图未能读取：{{ detail.failedImages.join('、') }}
          </p>
          <p v-if="loading && !promptText" class="chat-msg__loading">
            <RefreshCw class="spinning" :size="16" />
            {{ detail ? '正在让模型生成提示词' : '正在读取问题单并识别截图' }}
          </p>
          <p v-else-if="errorMessage && !promptText">{{ errorMessage }}</p>
          <pre v-if="promptText" class="chat-prompt" :class="{ 'chat-prompt--streaming': loading }">{{ promptText }}</pre>
          <p v-if="errorMessage && promptText" class="form-message form-message--error">{{ errorMessage }}</p>
          <button v-if="errorMessage && !loading" class="text-button" type="button" @click="loadIssue(issueKey)">重试</button>
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
      <button class="command-button command-button--primary" type="button" :disabled="!promptText || loading" @click="copyPrompt">
        <Check v-if="copied" :size="16" />
        <Copy v-else :size="16" />
        {{ copied ? '已复制' : '复制文本' }}
      </button>
    </footer>
  </main>
</template>
