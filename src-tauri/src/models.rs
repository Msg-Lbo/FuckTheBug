use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 持久化配置版本。
pub const CONFIG_VERSION: u8 = 2;

/// JIRA Token在系统凭据库中的固定账户名。
pub const TOKEN_ACCOUNT: &str = "jira-access-token";

/// AI Token在系统凭据库中的固定账户名。
pub const AI_TOKEN_ACCOUNT: &str = "ai-access-token";

/// 系统凭据库服务名。
pub const KEYRING_SERVICE: &str = "com.genata.bug-ticker";

/// 内置AI Skill。
pub const DEFAULT_AI_SKILL: &str = "你是资深客户端缺陷分析助手。根据用户提供的 JIRA 问题单正文和截图，整理成一段可直接交给编程 AI 落地改代码的提示词。\n\n\
工作方式：\n\
- 使用简体中文\n\
- 只使用问题单和截图里出现的信息\n\
- 结合截图中的界面结构、文案、控件和操作路径\n\
- 复现步骤写成可执行的顺序操作，一步一个动作\n\
- 修改要求写清要改的行为，不要空泛说「修复该问题」\n\
- 验证方式必须能按复现步骤核对";

/// 固定的提示词输出格式。
pub const AI_OUTPUT_FORMAT: &str = "只输出提示词正文，不要寒暄，不要用代码围栏包裹全文。必须按下面标题和顺序输出，标题一字不改；某项没有依据就写「问题单未提供」，禁止编造。\n\n\
## 任务\n\
## 问题单\n\
- Key：\n\
- 标题：\n\
- 链接：\n\
- 项目：\n\
- 类型：\n\
- 状态：\n\
- 优先级：\n\
- 版本：\n\
- 平台：\n\
## 问题细节\n\
## 复现步骤\n\
## 期望结果\n\
## 实际结果\n\
## 截图要点\n\
## 修改要求\n\
## 验证方式";

/// AI Skill最大字符数。
pub const AI_SKILL_MAX_CHARS: usize = 4000;

/// 应用持久化配置。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredAppConfig {
    pub version: u8,
    pub jira: StoredJiraConfig,
    #[serde(default)]
    pub ai: StoredAiConfig,
    pub views: Vec<IssueView>,
    pub window_position: Option<WindowPosition>,
}

/// 不包含Token的JIRA持久化配置。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredJiraConfig {
    pub base_url: String,
    pub refresh_interval: f64,
}

/// 不包含Token的AI持久化配置。
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredAiConfig {
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub skill: String,
}

/// 可公开给前端的应用配置。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicAppConfig {
    pub jira: PublicJiraConfig,
    pub ai: PublicAiConfig,
    pub views: Vec<IssueView>,
}

/// 可公开给前端的JIRA配置。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicJiraConfig {
    pub base_url: String,
    pub refresh_interval: f64,
    pub token: String,
    pub has_token: bool,
    pub clear_token: bool,
}

/// 可公开给前端的AI配置。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicAiConfig {
    pub base_url: String,
    pub model: String,
    pub token: String,
    pub has_token: bool,
    pub clear_token: bool,
    pub skill: String,
    pub default_skill: String,
    pub output_format: String,
}

/// 一个可计数和展示的问题单视图。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueView {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: IssueViewKind,
    #[serde(default)]
    pub jql: String,
    #[serde(default)]
    pub issues: Vec<IssueItem>,
}

/// 问题单视图类型。
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum IssueViewKind {
    #[default]
    Jira,
    Stash,
}

/// 主窗口物理坐标。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowPosition {
    pub x: i32,
    pub y: i32,
}

/// 旧Electron配置。
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LegacyAppConfig {
    #[serde(default)]
    pub feeds: Vec<LegacyFeedConfig>,
}

/// 旧Electron RSS源配置。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyFeedConfig {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_refresh_interval")]
    pub refresh_interval: f64,
}

/// JIRA搜索接口响应。
#[derive(Debug, Deserialize)]
pub struct JiraSearchResponse {
    pub total: usize,
    #[serde(default)]
    pub issues: Vec<JiraIssue>,
}

/// JIRA问题单。
#[derive(Debug, Deserialize)]
pub struct JiraIssue {
    pub key: String,
    pub fields: JiraIssueFields,
}

/// JIRA问题单字段。
#[derive(Debug, Deserialize)]
pub struct JiraIssueFields {
    pub summary: String,
    pub project: JiraProjectField,
    pub status: Option<JiraNamedField>,
    pub priority: Option<JiraNamedField>,
    pub issuetype: Option<JiraNamedField>,
    #[serde(default)]
    pub description: Option<Value>,
    pub updated: Option<String>,
    #[serde(default)]
    pub attachment: Vec<JiraAttachment>,
    #[serde(default)]
    pub environment: Option<Value>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub components: Vec<JiraNamedField>,
    pub reporter: Option<JiraUserField>,
    #[serde(flatten)]
    pub custom_fields: HashMap<String, Value>,
}

/// JIRA附件。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraAttachment {
    pub id: String,
    pub filename: String,
    pub mime_type: String,
    pub content: String,
    #[serde(default)]
    pub size: u64,
}

/// JIRA用户字段。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraUserField {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub display_name: String,
}

/// JIRA字段定义。
#[derive(Debug, Deserialize)]
pub struct JiraFieldDefinition {
    pub id: String,
    pub name: String,
}

/// JIRA项目字段。
#[derive(Debug, Deserialize)]
pub struct JiraProjectField {
    pub key: String,
    pub name: String,
}

/// JIRA通用名称字段。
#[derive(Debug, Deserialize)]
pub struct JiraNamedField {
    pub name: String,
}

/// 前端问题单数据。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueItem {
    pub title: String,
    pub link: String,
    pub key: String,
    pub project_key: String,
    pub project_name: String,
    pub issue_type: String,
    pub status: String,
    pub priority: String,
    #[serde(default)]
    pub versions: Vec<String>,
    #[serde(default)]
    pub platforms: Vec<String>,
    pub updated: String,
}

/// 问题单中的图片。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueImage {
    pub filename: String,
    pub mime_type: String,
    pub data_url: String,
}

/// 问题单完整详情，供AI提示词使用。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueDetail {
    pub title: String,
    pub link: String,
    pub key: String,
    pub project_key: String,
    pub project_name: String,
    pub issue_type: String,
    pub status: String,
    pub priority: String,
    pub versions: Vec<String>,
    pub platforms: Vec<String>,
    pub updated: String,
    pub description: String,
    pub environment: String,
    pub labels: Vec<String>,
    pub components: Vec<String>,
    pub reporter: String,
    pub images: Vec<IssueImage>,
    pub failed_images: Vec<String>,
}

/// 前端问题单搜索结果。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueResponse {
    pub view_id: String,
    pub view_name: String,
    pub count: usize,
    pub issues: Vec<IssueItem>,
}

/// JIRA连接测试结果。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraConnectionResult {
    pub display_name: String,
    pub username: String,
}

/// JIRA当前用户接口响应。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraUserResponse {
    pub display_name: String,
    #[serde(default)]
    pub name: String,
}

/// 返回默认刷新间隔。
fn default_refresh_interval() -> f64 {
    5.0
}

impl Default for StoredAppConfig {
    /// 创建面向个人问题单的默认配置。
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            jira: StoredJiraConfig {
                base_url: "https://jira.genata.net.cn".to_string(),
                refresh_interval: 1.0,
            },
            ai: StoredAiConfig::default(),
            views: vec![IssueView {
                id: uuid::Uuid::new_v4().to_string(),
                name: "我的问题单".to_string(),
                kind: IssueViewKind::Jira,
                jql: "assignee = currentUser() AND resolution = Unresolved ORDER BY priority DESC, updated DESC".to_string(),
                issues: Vec::new(),
            }],
            window_position: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_issue_view_defaults_to_jira() {
        let view: IssueView = serde_json::from_str(
            r#"{"id":"view-1","name":"我的问题单","jql":"assignee = currentUser()"}"#,
        )
        .expect("旧视图配置应可读取");

        assert_eq!(view.kind, IssueViewKind::Jira);
        assert!(view.issues.is_empty());
    }

    #[test]
    fn stash_view_round_trip_preserves_issues() {
        let view = IssueView {
            id: "stash-1".to_string(),
            name: "暂存".to_string(),
            kind: IssueViewKind::Stash,
            jql: String::new(),
            issues: vec![IssueItem {
                title: "修复登录异常".to_string(),
                link: "https://jira.example.com/browse/BUG-1".to_string(),
                key: "BUG-1".to_string(),
                project_key: "BUG".to_string(),
                project_name: "缺陷".to_string(),
                issue_type: "Bug".to_string(),
                status: "待处理".to_string(),
                priority: "High".to_string(),
                versions: vec!["2.1.6".to_string()],
                platforms: vec!["Android".to_string()],
                updated: "2026-08-20T00:00:00Z".to_string(),
            }],
        };
        let encoded = serde_json::to_string(&view).expect("暂存视图应可序列化");
        let decoded: IssueView = serde_json::from_str(&encoded).expect("暂存视图应可反序列化");

        assert_eq!(decoded.kind, IssueViewKind::Stash);
        assert_eq!(decoded.issues[0].key, "BUG-1");
        assert_eq!(decoded.issues[0].versions, ["2.1.6"]);
        assert_eq!(decoded.issues[0].platforms, ["Android"]);
    }

    #[test]
    fn legacy_stashed_issue_defaults_new_filter_fields() {
        let issue: IssueItem = serde_json::from_str(
            r#"{"title":"旧问题","link":"https://jira.example.com/browse/BUG-2","key":"BUG-2","projectKey":"BUG","projectName":"缺陷","issueType":"Bug","status":"待处理","priority":"High","updated":""}"#,
        )
        .expect("旧暂存问题单应可读取");

        assert!(issue.versions.is_empty());
        assert!(issue.platforms.is_empty());
    }

    #[test]
    fn stored_config_defaults_missing_ai() {
        let config: StoredAppConfig = serde_json::from_str(
            r#"{"version":2,"jira":{"baseUrl":"https://jira.example.com","refreshInterval":1},"views":[{"id":"1","name":"我的问题单","jql":"assignee = currentUser()"}]}"#,
        )
        .expect("旧配置缺少AI字段时应可读取");

        assert!(config.ai.base_url.is_empty());
        assert!(config.ai.model.is_empty());
        assert!(config.ai.skill.is_empty());
    }
}
