use serde::Serialize;
use serde_json::{Value, json};
use tauri::{Emitter, State};

use crate::{
    jira::load_issue_detail,
    models::IssueDetail,
    storage::{AppState, normalize_ai_url, read_ai_token},
};

const SYSTEM_PROMPT: &str = "你是资深客户端缺陷分析助手。根据用户提供的 JIRA 问题单正文和截图，生成一段可直接交给编程 AI 用于改代码的提示词。\
要求：使用中文；包含问题细节、复现方式、期望与实际结果、平台和版本；结合截图中的界面、文案和操作路径；不要编造问题单中没有的步骤；给出修改要求和验证方式；只输出提示词正文，不要寒暄。";

/// 流式生成的起始事件。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AiPromptStart {
    stream_id: u64,
}

/// 流式生成的文本片段。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AiPromptChunk {
    stream_id: u64,
    text: String,
}

/// 测试AI接口地址、Token和模型。
#[tauri::command]
pub async fn test_ai_connection(
    base_url: String,
    token: String,
    model: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let normalized_base_url = normalize_ai_url(&base_url)?;
    if model.trim().is_empty() || model.chars().count() > 100 {
        return Err("AI模型名称必须为1到100个字符".to_string());
    }
    let active_token = if token.is_empty() {
        read_ai_token()?
    } else {
        token
    };
    let response = state
        .ai_http_client
        .post(format!("{normalized_base_url}/chat/completions"))
        .bearer_auth(active_token)
        .json(&json!({
            "model": model.trim(),
            "stream": false,
            "max_tokens": 8,
            "messages": [{ "role": "user", "content": "回复ok" }]
        }))
        .send()
        .await
        .map_err(|error| format!("无法连接AI接口：{error}"))?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("AI认证失败，请检查Token是否有效".to_string());
    }
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("无法读取AI接口响应：{error}"))?;
    if !status.is_success() {
        return Err(format!("AI接口返回 HTTP {status}：{}", truncate_error(&body)));
    }
    let value: Value = serde_json::from_str(&body)
        .map_err(|error| format!("无法解析AI接口响应：{error}"))?;
    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("模型返回错误");
        return Err(message.to_string());
    }
    let reply = value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .unwrap_or("ok");
    println!("AI连接测试成功：{}", model.trim());
    Ok(format!("{}：{reply}", model.trim()))
}

/// 读取问题单并流式生成改代码提示词。
#[tauri::command]
pub async fn generate_ai_prompt(
    app: tauri::AppHandle,
    issue_key: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let stream_id = {
        let mut current = state
            .ai_stream_id
            .lock()
            .map_err(|_| "请求状态锁已损坏".to_string())?;
        *current += 1;
        *current
    };
    app.emit("ai-prompt-start", AiPromptStart { stream_id })
        .map_err(|error| format!("无法开始AI生成：{error}"))?;

    let issue = load_issue_detail(&issue_key, &state).await?;
    if !is_current_stream(&state, stream_id)? {
        return Err("生成已被取消".to_string());
    }
    app.emit("ai-prompt-context", &issue)
        .map_err(|error| format!("无法发送问题单详情：{error}"))?;

    let (base_url, model) = {
        let config = state
            .config
            .lock()
            .map_err(|_| "配置锁已损坏".to_string())?;
        (config.ai.base_url.clone(), config.ai.model.clone())
    };
    if base_url.is_empty() || model.is_empty() {
        return Err("尚未配置AI接口地址或模型".to_string());
    }
    let token = read_ai_token()?;
    let user_content = build_user_content(&issue);
    let mut response = state
        .ai_http_client
        .post(format!("{base_url}/chat/completions"))
        .bearer_auth(token)
        .json(&json!({
            "model": model,
            "stream": true,
            "messages": [
                { "role": "system", "content": SYSTEM_PROMPT },
                { "role": "user", "content": user_content }
            ]
        }))
        .send()
        .await
        .map_err(|error| format!("无法连接AI接口：{error}"))?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("AI认证失败，请在设置中更新Token".to_string());
    }
    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| String::new());
        return Err(format!("AI接口返回 HTTP {status}：{}", truncate_error(&body)));
    }

    let mut buffer = String::new();
    let mut full_text = String::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("读取模型流失败：{error}"))?
    {
        if !is_current_stream(&state, stream_id)? {
            return Err("生成已被取消".to_string());
        }
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(index) = buffer.find('\n') {
            let line = buffer[..index].trim_end_matches('\r').to_string();
            buffer.drain(..=index);
            if let Some(text) = parse_sse_line(&line)? {
                full_text.push_str(&text);
                app.emit(
                    "ai-prompt-chunk",
                    AiPromptChunk {
                        stream_id,
                        text,
                    },
                )
                .map_err(|error| format!("无法发送模型输出：{error}"))?;
            }
        }
    }
    if !buffer.trim().is_empty()
        && let Some(text) = parse_sse_line(buffer.trim())?
    {
        full_text.push_str(&text);
        app.emit(
            "ai-prompt-chunk",
            AiPromptChunk {
                stream_id,
                text,
            },
        )
        .map_err(|error| format!("无法发送模型输出：{error}"))?;
    }
    if !is_current_stream(&state, stream_id)? {
        return Err("生成已被取消".to_string());
    }
    if full_text.trim().is_empty() {
        return Err("模型没有返回任何内容".to_string());
    }
    app.emit("ai-prompt-done", AiPromptStart { stream_id })
        .map_err(|error| format!("无法结束AI生成：{error}"))?;
    Ok(full_text)
}

/// 判断当前流是否仍是最新请求。
fn is_current_stream(state: &State<'_, AppState>, stream_id: u64) -> Result<bool, String> {
    let current = state
        .ai_stream_id
        .lock()
        .map_err(|_| "请求状态锁已损坏".to_string())?;
    Ok(*current == stream_id)
}

/// 组装发给模型的问题单文本和截图。
fn build_user_content(issue: &IssueDetail) -> Value {
    let mut lines = vec![
        format!("问题单：{}", issue.key),
        format!("标题：{}", issue.title),
        format!("链接：{}", issue.link),
        format!("项目：{}（{}）", issue.project_key, issue.project_name),
        format!("类型：{}", issue.issue_type),
        format!("状态：{}", issue.status),
        format!("优先级：{}", issue.priority),
        format!("版本：{}", issue.versions.join("、")),
        format!("平台：{}", issue.platforms.join("、")),
        format!("报告人：{}", issue.reporter),
        format!("组件：{}", issue.components.join("、")),
        format!("标签：{}", issue.labels.join("、")),
        String::new(),
        "问题描述：".to_string(),
        if issue.description.is_empty() {
            "问题单没有填写描述。".to_string()
        } else {
            issue.description.clone()
        },
    ];
    if !issue.environment.is_empty() {
        lines.push(String::new());
        lines.push("环境：".to_string());
        lines.push(issue.environment.clone());
    }
    if !issue.images.is_empty() {
        let names = issue
            .images
            .iter()
            .map(|image| image.filename.as_str())
            .collect::<Vec<_>>()
            .join("、");
        lines.push(String::new());
        lines.push(format!(
            "以下附带 {} 张问题单截图：{names}。请识别截图中的界面和操作路径。",
            issue.images.len()
        ));
    }
    if !issue.failed_images.is_empty() {
        lines.push(format!(
            "以下截图未能读取：{}",
            issue.failed_images.join("、")
        ));
    }
    let text = lines.join("\n");
    if issue.images.is_empty() {
        return Value::String(text);
    }

    let mut parts = vec![json!({ "type": "text", "text": text })];
    for image in &issue.images {
        parts.push(json!({
            "type": "image_url",
            "image_url": { "url": image.data_url }
        }));
    }
    Value::Array(parts)
}

/// 解析一行SSE数据中的增量文本。
fn parse_sse_line(line: &str) -> Result<Option<String>, String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with(':') {
        return Ok(None);
    }
    let Some(data) = trimmed.strip_prefix("data:") else {
        return Ok(None);
    };
    let data = data.trim();
    if data.is_empty() || data == "[DONE]" {
        return Ok(None);
    }
    let value: Value =
        serde_json::from_str(data).map_err(|error| format!("无法解析模型流：{error}"))?;
    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("模型返回错误");
        return Err(message.to_string());
    }
    match value.pointer("/choices/0/delta/content") {
        Some(Value::String(text)) if !text.is_empty() => Ok(Some(text.clone())),
        Some(Value::Array(parts)) => {
            let text = parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<String>();
            Ok((!text.is_empty()).then_some(text))
        }
        _ => Ok(None),
    }
}

/// 截断接口错误正文。
fn truncate_error(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.chars().count() <= 300 {
        trimmed.to_string()
    } else {
        format!("{}…", trimmed.chars().take(300).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::parse_sse_line;

    #[test]
    fn parses_openai_delta_content() {
        let line = r#"data: {"choices":[{"delta":{"content":"复现"}}]}"#;
        assert_eq!(parse_sse_line(line).unwrap().as_deref(), Some("复现"));
    }

    #[test]
    fn ignores_done_and_comments() {
        assert!(parse_sse_line("data: [DONE]").unwrap().is_none());
        assert!(parse_sse_line(": keep-alive").unwrap().is_none());
        assert!(parse_sse_line("").unwrap().is_none());
    }
}
