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
                updated: "2026-08-20T00:00:00Z".to_string(),
            }],
        };
        let encoded = serde_json::to_string(&view).expect("暂存视图应可序列化");
        let decoded: IssueView = serde_json::from_str(&encoded).expect("暂存视图应可反序列化");

        assert_eq!(decoded.kind, IssueViewKind::Stash);
        assert_eq!(decoded.issues[0].key, "BUG-1");
    }
}
