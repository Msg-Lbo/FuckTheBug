use serde::{Deserialize, Serialize};

/// 持久化配置版本。
pub const CONFIG_VERSION: u8 = 2;

/// JIRA Token在系统凭据库中的固定账户名。
pub const TOKEN_ACCOUNT: &str = "jira-access-token";

/// 系统凭据库服务名。
pub const KEYRING_SERVICE: &str = "com.genata.bug-ticker";

/// 应用持久化配置。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredAppConfig {
    pub version: u8,
    pub jira: StoredJiraConfig,
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

/// 可公开给前端的应用配置。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicAppConfig {
    pub jira: PublicJiraConfig,
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

/// 一个可计数和展示的问题单视图。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueView {
    pub id: String,
    pub name: String,
    pub jql: String,
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
    pub updated: Option<String>,
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
#[derive(Clone, Debug, Serialize)]
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
    pub updated: String,
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
            views: vec![IssueView {
                id: uuid::Uuid::new_v4().to_string(),
                name: "我的问题单".to_string(),
                jql: "assignee = currentUser() AND resolution = Unresolved ORDER BY priority DESC, updated DESC".to_string(),
            }],
            window_position: None,
        }
    }
}
