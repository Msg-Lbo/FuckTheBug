export interface JiraConfig {
  baseUrl: string
  refreshInterval: number
  token: string
  hasToken: boolean
  clearToken: boolean
}

export interface AiConfig {
  baseUrl: string
  model: string
  token: string
  hasToken: boolean
  clearToken: boolean
  skill: string
  defaultSkill: string
  outputFormat: string
}

export interface IssueView {
  id: string
  name: string
  kind: 'jira' | 'stash'
  jql: string
  issues: IssueItem[]
}

export interface AppConfig {
  jira: JiraConfig
  ai: AiConfig
  views: IssueView[]
}

export interface IssueItem {
  title: string
  link: string
  key: string
  projectKey: string
  projectName: string
  issueType: string
  status: string
  priority: string
  versions: string[]
  platforms: string[]
  updated: string
}

export interface IssueResponse {
  viewId: string
  viewName: string
  count: number
  issues: IssueItem[]
}

export interface ViewRuntime {
  loading: boolean
  initialized: boolean
  hasNewIssues: boolean
  count: number
  issues: IssueItem[]
  error: string
  updatedAt: Date | null
}

export interface JiraConnectionResult {
  displayName: string
  username: string
}

export interface IssueImage {
  filename: string
  mimeType: string
  dataUrl: string
}

export interface IssueDetail {
  title: string
  link: string
  key: string
  projectKey: string
  projectName: string
  issueType: string
  status: string
  priority: string
  versions: string[]
  platforms: string[]
  updated: string
  description: string
  environment: string
  labels: string[]
  components: string[]
  reporter: string
  images: IssueImage[]
  failedImages: string[]
}
