use tauri::State;
use tauri_plugin_opener::OpenerExt;
use url::Url;

use crate::{
    models::{
        IssueItem, IssueResponse, IssueViewKind, JiraConnectionResult, JiraSearchResponse,
        JiraUserResponse,
    },
    storage::{AppState, normalize_base_url, read_jira_token},
};

/// 查询指定JQL视图的问题单。
#[tauri::command]
pub async fn fetch_issues(
    view_id: String,
    state: State<'_, AppState>,
) -> Result<IssueResponse, String> {
    {
        let mut in_flight = state
            .in_flight_views
            .lock()
            .map_err(|_| "请求状态锁已损坏".to_string())?;
        if !in_flight.insert(view_id.clone()) {
            return Err("该视图正在刷新，请稍候".to_string());
        }
    }

    let result = fetch_issues_inner(&view_id, &state).await; // 实际请求结果
    state
        .in_flight_views
        .lock()
        .map_err(|_| "请求状态锁已损坏".to_string())?
        .remove(&view_id);
    match &result {
        Ok(response) => println!(
            "JIRA视图“{}”刷新成功，共{}条问题单",
            response.view_name, response.count
        ),
        Err(error) => eprintln!("JIRA视图刷新失败：{error}"),
    }
    result
}

/// 测试JIRA Token并返回当前用户。
#[tauri::command]
pub async fn test_jira_connection(
    base_url: String,
    token: String,
    state: State<'_, AppState>,
) -> Result<JiraConnectionResult, String> {
    let normalized_base_url = normalize_base_url(&base_url)?; // 规范化地址
    let active_token = if token.is_empty() {
        read_jira_token()?
    } else {
        token
    };
    let url = format!("{normalized_base_url}/rest/api/2/myself"); // 当前用户接口
    let response = state
        .http_client
        .get(url)
        .bearer_auth(active_token)
        .send()
        .await
        .map_err(|error| format!("无法连接JIRA：{error}"))?;

    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("JIRA认证失败，请检查Token是否有效".to_string());
    }
    let response = response
        .error_for_status()
        .map_err(|error| format!("JIRA返回错误：{error}"))?;
    let user: JiraUserResponse = response
        .json()
        .await
        .map_err(|error| format!("无法解析JIRA用户信息：{error}"))?;
    println!("JIRA连接测试成功：{}", user.display_name);

    Ok(JiraConnectionResult {
        display_name: user.display_name,
        username: user.name,
    })
}

/// 打开HTTP或HTTPS问题单链接。
#[tauri::command]
pub fn open_external(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let parsed = Url::parse(&url).map_err(|_| "问题单链接格式不正确".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("仅允许打开HTTP或HTTPS链接".to_string());
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| format!("无法打开外部链接：{error}"))
}

/// 执行JIRA搜索请求。
async fn fetch_issues_inner(
    view_id: &str,
    state: &State<'_, AppState>,
) -> Result<IssueResponse, String> {
    let (jira, view) = {
        let config = state
            .config
            .lock()
            .map_err(|_| "配置锁已损坏".to_string())?;
        let view = config
            .views
            .iter()
            .find(|view| view.id == view_id && view.kind == IssueViewKind::Jira)
            .cloned()
            .ok_or_else(|| "未找到问题单视图".to_string())?;
        (config.jira.clone(), view)
    };
    let token = read_jira_token()?; // JIRA访问Token
    let url = format!("{}/rest/api/2/search", jira.base_url); // JIRA搜索接口
    let response = state
        .http_client
        .get(url)
        .bearer_auth(token)
        .query(&[
            ("jql", view.jql.as_str()),
            ("startAt", "0"),
            ("maxResults", "100"),
            (
                "fields",
                "summary,project,status,priority,issuetype,updated",
            ),
        ])
        .send()
        .await
        .map_err(|error| format!("无法连接JIRA：{error}"))?;

    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("JIRA认证失败，请在设置中更新Token".to_string());
    }
    let response = response
        .error_for_status()
        .map_err(|error| format!("JIRA查询失败：{error}"))?;
    let search: JiraSearchResponse = response
        .json()
        .await
        .map_err(|error| format!("无法解析JIRA响应：{error}"))?;
    let issues = search
        .issues
        .into_iter()
        .map(|issue| {
            let fields = issue.fields; // 当前问题单字段
            IssueItem {
                link: format!("{}/browse/{}", jira.base_url, issue.key),
                key: issue.key,
                title: fields.summary,
                project_key: fields.project.key,
                project_name: fields.project.name,
                issue_type: fields.issuetype.map(|field| field.name).unwrap_or_default(),
                status: fields.status.map(|field| field.name).unwrap_or_default(),
                priority: fields.priority.map(|field| field.name).unwrap_or_default(),
                updated: fields.updated.unwrap_or_default(),
            }
        })
        .collect();

    Ok(IssueResponse {
        view_id: view.id,
        view_name: view.name,
        count: search.total,
        issues,
    })
}
