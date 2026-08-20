use tauri::State;
use tauri_plugin_opener::OpenerExt;
use url::Url;

use crate::{
    models::{
        IssueItem, IssueResponse, IssueViewKind, JiraConnectionResult, JiraFieldDefinition,
        JiraSearchResponse, JiraUserResponse,
    },
    storage::{AppState, normalize_base_url, read_jira_token},
};

/// 从问题单字段文本中归纳移动平台。
///
/// # 参数
/// * `source` - 版本、组件、标签和标题组成的文本
///
/// # 返回值
/// 标准化的平台名称列表
fn extract_platforms(source: &str) -> Vec<String> {
    let normalized = source.to_lowercase(); // 统一用于匹配的平台文本
    let mut platforms = Vec::new(); // 归一化平台列表
    if normalized.contains("android") || normalized.contains("安卓") {
        platforms.push("Android".to_string());
    }
    if normalized
        .split(|character: char| !character.is_ascii_alphanumeric())
        .any(|segment| segment == "ios")
    {
        platforms.push("iOS".to_string());
    }
    platforms
}

/// 将JIRA描述字段转换为可检索文本。
///
/// # 参数
/// * `value` - JIRA字段JSON值
///
/// # 返回值
/// 字段中包含的全部文本
fn jira_value_to_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(items) => items
            .iter()
            .map(jira_value_to_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        serde_json::Value::Object(fields) => fields
            .values()
            .map(jira_value_to_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// 读取JIRA选择类自定义字段的显示文本。
///
/// # 参数
/// * `value` - 自定义字段JSON值
///
/// # 返回值
/// 字段显示文本
fn jira_option_to_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(items) => items
            .iter()
            .map(jira_option_to_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
        serde_json::Value::Object(fields) => fields
            .get("value")
            .or_else(|| fields.get("name"))
            .map(jira_option_to_text)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// 清理描述中的版本候选文本。
/// @param token - 描述中的单词
/// @returns 版本候选值
fn normalize_version_candidate(token: &str) -> String {
    let Some((start, _)) = token
        .char_indices()
        .find(|(_, character)| character.is_ascii_digit())
    else {
        return String::new();
    };
    let prefix = &token[..start]; // 数字前缀
    let numeric: String = token[start..]
        .chars()
        .take_while(|character| character.is_ascii_digit() || matches!(character, '.' | '-' | '_'))
        .collect();
    if prefix.ends_with('V') || prefix.ends_with('v') {
        format!("V{numeric}")
    } else {
        numeric
    }
}

/// 判断文本是否为多段版本号。
fn is_version_number(value: &str) -> bool {
    let parts = value
        .trim_start_matches(['V', 'v'])
        .split(['.', '-', '_'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>(); // 版本号分段
    (2..=3).contains(&parts.len())
        && parts
            .iter()
            .all(|part| part.chars().all(|character| character.is_ascii_digit()))
}

/// 从描述文本中提取版本号。
///
/// # 参数
/// * `description` - JIRA问题描述
///
/// # 返回值
/// 去重后的版本号列表
fn extract_versions(description: &str) -> Vec<String> {
    let mut versions = Vec::new(); // 描述中的版本号
    let mut expect_version = false; // 下一行是否为版本值
    for line in description.lines() {
        let trimmed = line.trim(); // 当前描述行
        let has_version_label = trimmed.contains("版本"); // 是否包含版本标题
        let version_context = expect_version || has_version_label; // 当前行是否处于版本上下文
        for token in trimmed.split_whitespace() {
            let candidate = normalize_version_candidate(token); // 清理后的候选版本
            let explicit_version =
                token.trim_start().starts_with(['V', 'v']) && is_version_number(&candidate); // 是否为明确的V开头版本号
            if (version_context || explicit_version) && is_version_number(&candidate) {
                versions.push(candidate);
            }
        }
        expect_version = has_version_label || (expect_version && trimmed.is_empty());
    }
    versions.sort();
    versions.dedup();
    versions
}

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
    let fields_url = format!("{}/rest/api/2/field", jira.base_url); // JIRA字段元数据地址
    let field_response = state
        .http_client
        .get(fields_url)
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|error| format!("无法读取JIRA字段：{error}"))?;
    if field_response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("JIRA认证失败，请在设置中更新Token".to_string());
    }
    let field_definitions: Vec<JiraFieldDefinition> = field_response
        .error_for_status()
        .map_err(|error| format!("JIRA字段查询失败：{error}"))?
        .json()
        .await
        .map_err(|error| format!("无法解析JIRA字段：{error}"))?;
    let platform_field_id = field_definitions
        .into_iter()
        .find(|field| field.name.trim() == "操作平台")
        .map(|field| field.id); // 操作平台自定义字段ID
    let url = format!("{}/rest/api/2/search", jira.base_url); // JIRA搜索接口
    let mut start_at = 0; // 当前分页起始位置
    let mut jira_issues = Vec::new(); // 全部分页问题单
    let total = loop {
        let start_at_text = start_at.to_string(); // 分页起始位置参数
        let fields = match &platform_field_id {
            Some(field_id) => {
                format!("summary,description,project,status,priority,issuetype,updated,{field_id}")
            }
            None => "summary,description,project,status,priority,issuetype,updated".to_string(),
        }; // 搜索字段列表
        let response = state
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .query(&[
                ("jql", view.jql.as_str()),
                ("startAt", start_at_text.as_str()),
                ("maxResults", "100"),
                ("fields", fields.as_str()),
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
        let mut search: JiraSearchResponse = response
            .json()
            .await
            .map_err(|error| format!("无法解析JIRA响应：{error}"))?;
        let total = search.total; // JIRA问题单总数
        let page_size = search.issues.len(); // 当前分页实际条数
        if page_size == 0 && jira_issues.len() < total {
            return Err(format!(
                "JIRA分页返回异常：已获取{}条，总数{total}条",
                jira_issues.len()
            ));
        }
        jira_issues.append(&mut search.issues);
        if jira_issues.len() >= total {
            break total;
        }
        start_at += page_size;
    };
    let issues = jira_issues
        .into_iter()
        .map(|issue| {
            let fields = issue.fields; // 当前问题单字段
            let description = fields
                .description
                .as_ref()
                .map(jira_value_to_text)
                .unwrap_or_default(); // 问题描述文本
            let versions = extract_versions(&description); // 描述中的版本列表
            let custom_platform = platform_field_id
                .as_ref()
                .and_then(|field_id| fields.custom_fields.get(field_id))
                .map(jira_option_to_text)
                .unwrap_or_default(); // 操作平台字段文本
            let platform_source = std::iter::once(fields.summary.as_str())
                .chain(std::iter::once(custom_platform.as_str()))
                .collect::<Vec<_>>()
                .join(" "); // 平台识别文本
            let platforms = extract_platforms(&platform_source); // 归一化平台列表
            IssueItem {
                link: format!("{}/browse/{}", jira.base_url, issue.key),
                key: issue.key,
                title: fields.summary,
                project_key: fields.project.key,
                project_name: fields.project.name,
                issue_type: fields.issuetype.map(|field| field.name).unwrap_or_default(),
                status: fields.status.map(|field| field.name).unwrap_or_default(),
                priority: fields.priority.map(|field| field.name).unwrap_or_default(),
                versions,
                platforms,
                updated: fields.updated.unwrap_or_default(),
            }
        })
        .collect();

    Ok(IssueResponse {
        view_id: view.id,
        view_name: view.name,
        count: total,
        issues,
    })
}

#[cfg(test)]
mod tests {
    use super::{extract_platforms, extract_versions};

    #[test]
    fn extracts_and_normalizes_mobile_platforms() {
        assert_eq!(extract_platforms("Android 15 iOS-App"), ["Android", "iOS"]);
        assert_eq!(extract_platforms("安卓客户端"), ["Android"]);
        assert!(extract_platforms("BIOS 设置").is_empty());
    }

    #[test]
    fn extracts_versions_from_description_labels() {
        assert_eq!(extract_versions("1.版本\n\nV1.61.1"), ["V1.61.1"]);
        assert_eq!(
            extract_versions("版本：1.2.3\n版本：2.0.0"),
            ["1.2.3", "2.0.0"]
        );
        assert!(extract_versions("2026-08-11-16-05-14-808").is_empty());
        assert!(extract_versions("版本\n\n2026-08-10-20-23-23-799").is_empty());
    }
}
