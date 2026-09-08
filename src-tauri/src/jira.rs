use tauri::State;
use tauri_plugin_opener::OpenerExt;
use url::Url;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::{
    models::{
        IssueDetail, IssueImage, IssueItem, IssueResponse, IssueViewKind, JiraAttachment,
        JiraConnectionResult, JiraFieldDefinition, JiraIssue, JiraIssueFields, JiraSearchResponse,
        JiraUserResponse,
    },
    storage::{AppState, normalize_base_url, read_jira_token},
};

/// 单次带入提示词的最大图片数。
const MAX_ISSUE_IMAGES: usize = 6;
/// 单张图片最大字节数。
const MAX_IMAGE_BYTES: u64 = 1_500_000;

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

/// 校验JIRA问题单标识。
///
/// # 参数
/// * `issue_key` - 问题单 Key
fn validate_issue_key(issue_key: &str) -> Result<(), String> {
    let valid = issue_key.len() <= 32
        && issue_key.contains('-')
        && issue_key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-');
    if valid {
        Ok(())
    } else {
        Err("问题单标识格式不正确".to_string())
    }
}

/// 从JIRA Wiki描述中提取图片文件名。
///
/// # 参数
/// * `source` - 描述原文
///
/// # 返回值
/// 图片文件名列表，保持出现顺序
fn extract_wiki_image_names(source: &str) -> Vec<String> {
    let mut names = Vec::new(); // 描述中的图片文件名
    let mut rest = source;
    while let Some(start) = rest.find('!') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('!') else {
            break;
        };
        let inner = after[..end].trim(); // 宏内部文本
        rest = &after[end + 1..];
        if inner.is_empty() {
            continue;
        }
        let token = inner.split('|').next().unwrap_or("").trim(); // 去掉缩略图参数
        let filename = if let Some(index) = token.rfind('/') {
            &token[index + 1..]
        } else {
            token
        };
        if filename.contains('.') && !names.iter().any(|item| item == filename) {
            names.push(filename.to_string());
        }
    }
    names
}

/// 去掉Wiki行中的图片宏。
///
/// # 参数
/// * `line` - 原始行
///
/// # 返回值
/// 去掉图片宏后的文本
fn strip_wiki_images(line: &str) -> String {
    let mut output = String::new();
    let mut rest = line;
    while let Some(start) = rest.find('!') {
        output.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('!') {
            Some(end) => rest = &after[end + 1..],
            None => {
                output.push('!');
                rest = after;
            }
        }
    }
    output.push_str(rest);
    output
}

/// 将JIRA Wiki描述转为可读文本。
///
/// # 参数
/// * `source` - Wiki或纯文本描述
///
/// # 返回值
/// 便于交给AI阅读的文本
fn wiki_to_text(source: &str) -> String {
    let mut lines = Vec::new(); // 转换后的行
    let mut in_code = false; // 是否位于代码块
    for line in source.lines() {
        let trimmed = line.trim(); // 当前行
        let lower = trimmed.to_ascii_lowercase(); // 用于识别宏
        if in_code && (lower == "{code}" || lower == "{noformat}") {
            in_code = false;
            lines.push("```".to_string());
            continue;
        }
        if lower.starts_with("{code") || lower.starts_with("{noformat") {
            in_code = true;
            lines.push("```".to_string());
            continue;
        }
        if in_code {
            lines.push(line.to_string());
            continue;
        }
        let stripped = strip_wiki_images(trimmed);
        let stripped = stripped.trim();
        if stripped.is_empty() {
            if lines.last().is_some_and(|item| !item.is_empty()) {
                lines.push(String::new());
            }
            continue;
        }
        let converted = if let Some(rest) = stripped.strip_prefix("h1. ") {
            format!("# {}", rest.trim())
        } else if let Some(rest) = stripped.strip_prefix("h2. ") {
            format!("## {}", rest.trim())
        } else if let Some(rest) = stripped.strip_prefix("h3. ") {
            format!("### {}", rest.trim())
        } else if let Some(rest) = stripped.strip_prefix("h4. ") {
            format!("#### {}", rest.trim())
        } else if let Some(rest) = stripped.strip_prefix("# ") {
            format!("1. {}", rest.trim())
        } else if let Some(rest) = stripped.strip_prefix("* ") {
            format!("- {}", rest.trim())
        } else if let Some(rest) = stripped.strip_prefix("- ") {
            format!("- {}", rest.trim())
        } else {
            stripped.to_string()
        };
        if !converted.is_empty() {
            lines.push(converted);
        }
    }
    lines.join("\n").trim().to_string()
}

/// 将JIRA问题单字段映射为列表项。
///
/// # 参数
/// * `base_url` - JIRA根地址
/// * `key` - 问题单 Key
/// * `fields` - 问题单字段
/// * `platform_field_id` - 操作平台自定义字段ID
///
/// # 返回值
/// 前端问题单数据
fn map_jira_fields(
    base_url: &str,
    key: &str,
    fields: &JiraIssueFields,
    platform_field_id: Option<&str>,
) -> IssueItem {
    let description = fields
        .description
        .as_ref()
        .map(jira_value_to_text)
        .unwrap_or_default(); // 问题描述文本
    let versions = extract_versions(&description); // 描述中的版本列表
    let custom_platform = platform_field_id
        .and_then(|field_id| fields.custom_fields.get(field_id))
        .map(jira_option_to_text)
        .unwrap_or_default(); // 操作平台字段文本
    let platform_source = format!("{} {custom_platform}", fields.summary); // 平台识别文本
    IssueItem {
        link: format!("{base_url}/browse/{key}"),
        key: key.to_string(),
        title: fields.summary.clone(),
        project_key: fields.project.key.clone(),
        project_name: fields.project.name.clone(),
        issue_type: fields
            .issuetype
            .as_ref()
            .map(|field| field.name.clone())
            .unwrap_or_default(),
        status: fields
            .status
            .as_ref()
            .map(|field| field.name.clone())
            .unwrap_or_default(),
        priority: fields
            .priority
            .as_ref()
            .map(|field| field.name.clone())
            .unwrap_or_default(),
        versions,
        platforms: extract_platforms(&platform_source),
        updated: fields.updated.clone().unwrap_or_default(),
    }
}

/// 读取操作平台自定义字段ID。
async fn read_platform_field_id(
    client: &reqwest::Client,
    base_url: &str,
    token: &str,
) -> Result<Option<String>, String> {
    let field_response = client
        .get(format!("{base_url}/rest/api/2/field"))
        .bearer_auth(token)
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
    Ok(field_definitions
        .into_iter()
        .find(|field| field.name.trim() == "操作平台")
        .map(|field| field.id))
}

/// 按描述引用顺序下载问题单图片。
async fn download_issue_images(
    client: &reqwest::Client,
    token: &str,
    base_url: &str,
    description: &str,
    attachments: &[JiraAttachment],
) -> (Vec<IssueImage>, Vec<String>) {
    let wiki_names = extract_wiki_image_names(description); // 描述中点名的图片
    let mut ordered: Vec<&JiraAttachment> = Vec::new(); // 下载顺序
    let mut failed_images = Vec::new(); // 下载失败的文件名
    for name in &wiki_names {
        if let Some(attachment) = attachments.iter().find(|item| item.filename == *name) {
            if !ordered.iter().any(|item| item.id == attachment.id) {
                ordered.push(attachment);
            }
        } else {
            failed_images.push(format!("{name}（描述中引用但未找到附件）"));
        }
    }
    for attachment in attachments {
        if ordered.len() >= MAX_ISSUE_IMAGES {
            break;
        }
        if !attachment.mime_type.starts_with("image/") {
            continue;
        }
        if !ordered.iter().any(|item| item.id == attachment.id) {
            ordered.push(attachment);
        }
    }

    let mut images = Vec::new(); // 已下载图片
    for attachment in ordered.into_iter().take(MAX_ISSUE_IMAGES) {
        if !attachment.mime_type.starts_with("image/") {
            continue;
        }
        if attachment.size > MAX_IMAGE_BYTES {
            failed_images.push(format!(
                "{}（超过{}KB）",
                attachment.filename,
                MAX_IMAGE_BYTES / 1024
            ));
            continue;
        }
        let url = if attachment.content.starts_with("http://")
            || attachment.content.starts_with("https://")
        {
            attachment.content.clone()
        } else {
            format!("{base_url}{}", attachment.content)
        };
        let response = match client.get(&url).bearer_auth(token).send().await {
            Ok(response) => response,
            Err(error) => {
                failed_images.push(format!("{}：{error}", attachment.filename));
                continue;
            }
        };
        if !response.status().is_success() {
            failed_images.push(format!("{}：HTTP {}", attachment.filename, response.status()));
            continue;
        }
        let bytes = match response.bytes().await {
            Ok(bytes) => bytes,
            Err(error) => {
                failed_images.push(format!("{}：{error}", attachment.filename));
                continue;
            }
        };
        if bytes.len() as u64 > MAX_IMAGE_BYTES {
            failed_images.push(format!(
                "{}（超过{}KB）",
                attachment.filename,
                MAX_IMAGE_BYTES / 1024
            ));
            continue;
        }
        images.push(IssueImage {
            filename: attachment.filename.clone(),
            mime_type: attachment.mime_type.clone(),
            data_url: format!(
                "data:{};base64,{}",
                attachment.mime_type,
                STANDARD.encode(&bytes)
            ),
        });
    }
    (images, failed_images)
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

/// 获取问题单详情、描述和图片。
#[tauri::command]
pub async fn fetch_issue_detail(
    issue_key: String,
    state: State<'_, AppState>,
) -> Result<IssueDetail, String> {
    load_issue_detail(&issue_key, &state).await
}

/// 读取问题单详情、描述和图片。
pub async fn load_issue_detail(
    issue_key: &str,
    state: &State<'_, AppState>,
) -> Result<IssueDetail, String> {
    validate_issue_key(issue_key)?;
    let base_url = {
        let config = state
            .config
            .lock()
            .map_err(|_| "配置锁已损坏".to_string())?;
        config.jira.base_url.clone()
    };
    let token = read_jira_token()?; // JIRA访问Token
    let platform_field_id =
        read_platform_field_id(&state.http_client, &base_url, &token).await?; // 操作平台自定义字段ID
    let mut field_list = String::from(
        "summary,description,attachment,project,status,priority,issuetype,updated,environment,labels,components,reporter",
    );
    if let Some(field_id) = &platform_field_id {
        field_list.push(',');
        field_list.push_str(field_id);
    }
    let response = state
        .http_client
        .get(format!("{base_url}/rest/api/2/issue/{issue_key}"))
        .bearer_auth(&token)
        .query(&[("fields", field_list.as_str())])
        .send()
        .await
        .map_err(|error| format!("无法连接JIRA：{error}"))?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("JIRA认证失败，请在设置中更新Token".to_string());
    }
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(format!("未找到问题单 {issue_key}"));
    }
    let issue: JiraIssue = response
        .error_for_status()
        .map_err(|error| format!("JIRA查询失败：{error}"))?
        .json()
        .await
        .map_err(|error| format!("无法解析问题单详情：{error}"))?;
    let description_source = issue
        .fields
        .description
        .as_ref()
        .map(jira_value_to_text)
        .unwrap_or_default(); // 描述原文
    let item = map_jira_fields(
        &base_url,
        &issue.key,
        &issue.fields,
        platform_field_id.as_deref(),
    );
    let (images, failed_images) = download_issue_images(
        &state.http_client,
        &token,
        &base_url,
        &description_source,
        &issue.fields.attachment,
    )
    .await;
    let reporter = issue
        .fields
        .reporter
        .as_ref()
        .map(|user| {
            if user.display_name.is_empty() {
                user.name.clone()
            } else {
                user.display_name.clone()
            }
        })
        .unwrap_or_default(); // 报告人
    Ok(IssueDetail {
        title: item.title,
        link: item.link,
        key: item.key,
        project_key: item.project_key,
        project_name: item.project_name,
        issue_type: item.issue_type,
        status: item.status,
        priority: item.priority,
        versions: item.versions,
        platforms: item.platforms,
        updated: item.updated,
        description: wiki_to_text(&description_source),
        environment: issue
            .fields
            .environment
            .as_ref()
            .map(jira_value_to_text)
            .map(|text| wiki_to_text(&text))
            .unwrap_or_default(),
        labels: issue.fields.labels,
        components: issue
            .fields
            .components
            .into_iter()
            .map(|component| component.name)
            .collect(),
        reporter,
        images,
        failed_images,
    })
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
    let platform_field_id =
        read_platform_field_id(&state.http_client, &jira.base_url, &token).await?; // 操作平台自定义字段ID
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
            map_jira_fields(
                &jira.base_url,
                &issue.key,
                &issue.fields,
                platform_field_id.as_deref(),
            )
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
    use super::{extract_platforms, extract_versions, extract_wiki_image_names, wiki_to_text};

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

    #[test]
    fn extracts_wiki_image_names_in_order() {
        let source = "现象\n!home.png|thumbnail!\n步骤\n!https://jira.example.com/secure/attachment/12/replay.jpg!";
        assert_eq!(extract_wiki_image_names(source), ["home.png", "replay.jpg"]);
    }

    #[test]
    fn converts_wiki_description_to_readable_text() {
        let source = "h3. 复现步骤\n# 打开预览页\n* 关闭监听\n!shot.png|thumbnail!\n{code:java}\nfoo()\n{code}";
        let text = wiki_to_text(source);
        assert!(text.contains("### 复现步骤"));
        assert!(text.contains("1. 打开预览页"));
        assert!(text.contains("- 关闭监听"));
        assert!(text.contains("```\nfoo()\n```"));
        assert!(!text.contains("shot.png"));
    }
}
