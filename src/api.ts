import { invoke } from '@tauri-apps/api/core'
import type { AppConfig, IssueResponse, JiraConnectionResult } from './types'

/**
 * 获取应用配置
 * @returns 当前可公开给前端的配置
 */
export function getConfig(): Promise<AppConfig> {
  return invoke<AppConfig>('get_config')
}

/**
 * 保存并校验应用配置
 * @param config - 待保存配置
 * @returns 保存后的公开配置
 */
export function saveConfig(config: AppConfig): Promise<AppConfig> {
  return invoke<AppConfig>('save_config', { config })
}

/**
 * 由安装后的Tauri应用发送Windows系统通知
 * @param title - 通知标题
 * @param body - 通知正文
 * @returns 是否实际发送，开发构建返回false
 */
export function sendSystemNotification(title: string, body: string): Promise<boolean> {
  return invoke<boolean>('send_system_notification', { title, body })
}

/**
 * 获取指定JQL视图的问题单
 * @param viewId - 视图标识
 * @returns 问题单数据
 */
export function fetchIssues(viewId: string): Promise<IssueResponse> {
  return invoke<IssueResponse>('fetch_issues', { viewId })
}

/**
 * 测试JIRA地址和Token
 * @param baseUrl - JIRA根地址
 * @param token - 新Token，空字符串表示使用已保存Token
 * @returns 当前JIRA用户
 */
export function testJiraConnection(baseUrl: string, token: string): Promise<JiraConnectionResult> {
  return invoke<JiraConnectionResult>('test_jira_connection', { baseUrl, token })
}

/**
 * 打开经过协议校验的问题单链接
 * @param url - 问题单链接
 */
export function openExternal(url: string): Promise<void> {
  return invoke('open_external', { url })
}

/**
 * 调整主窗口尺寸并限制在当前屏幕内
 * @param width - 逻辑像素宽度
 * @param height - 逻辑像素高度
 */
export function resizeMainWindow(width: number, height: number): Promise<void> {
  return invoke('resize_main_window', { width, height })
}

/**
 * 开始拖动主窗口
 */
export function startMainDragging(): Promise<void> {
  return invoke('start_main_dragging')
}

/**
 * 保存主窗口当前位置
 */
export function saveMainWindowPosition(): Promise<void> {
  return invoke('save_main_window_position')
}

/**
 * 显示设置窗口
 */
export function openSettingsWindow(): Promise<void> {
  return invoke('open_settings_window')
}

/**
 * 隐藏设置窗口
 */
export function closeSettingsWindow(): Promise<void> {
  return invoke('close_settings_window')
}
