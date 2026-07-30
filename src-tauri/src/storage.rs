use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use keyring::Entry;
use tauri::{AppHandle, Manager};
use url::Url;
use uuid::Uuid;

use crate::models::{
    CONFIG_VERSION, IssueView, KEYRING_SERVICE, LegacyAppConfig, PublicAppConfig, PublicJiraConfig,
    StoredAppConfig, TOKEN_ACCOUNT,
};

/// 应用共享状态。
pub struct AppState {
    pub config_path: PathBuf,
    pub config: Mutex<StoredAppConfig>,
    pub in_flight_views: Mutex<std::collections::HashSet<String>>,
    pub http_client: reqwest::Client,
}

/// 初始化配置目录、迁移旧配置并创建共享状态。
pub fn initialize_state(app: &AppHandle) -> Result<AppState, String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("无法确定配置目录：{error}"))?;
    fs::create_dir_all(&config_dir).map_err(|error| format!("无法创建配置目录：{error}"))?;

    let config_path = config_dir.join("config.json"); // 新配置文件路径
    let config = if config_path.exists() {
        read_stored_config(&config_path)?
    } else {
        migrate_legacy_config(app, &config_path)?
    };

    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("FuckTheBug/2.0")
        .build()
        .map_err(|error| format!("无法创建HTTP客户端：{error}"))?;

    Ok(AppState {
        config_path,
        config: Mutex::new(config),
        in_flight_views: Mutex::new(std::collections::HashSet::new()),
        http_client,
    })
}

/// 读取并验证持久化配置。
pub fn read_stored_config(path: &Path) -> Result<StoredAppConfig, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("无法读取配置文件：{error}"))?;
    let config: StoredAppConfig =
        serde_json::from_str(&text).map_err(|error| format!("配置文件格式错误：{error}"))?;

    if config.version != CONFIG_VERSION {
        return Err(format!("不支持的配置版本：{}", config.version));
    }

    validate_stored_config(&config)?;
    Ok(config)
}

/// 将持久化配置写入用户配置目录。
pub fn write_stored_config(path: &Path, config: &StoredAppConfig) -> Result<(), String> {
    let text =
        serde_json::to_string_pretty(config).map_err(|error| format!("无法序列化配置：{error}"))?;
    fs::write(path, text).map_err(|error| format!("无法写入配置文件：{error}"))
}

/// 将持久化配置转换为不含Token的前端配置。
pub fn to_public_config(config: &StoredAppConfig) -> Result<PublicAppConfig, String> {
    Ok(PublicAppConfig {
        jira: PublicJiraConfig {
            base_url: config.jira.base_url.clone(),
            refresh_interval: config.jira.refresh_interval,
            token: String::new(),
            has_token: has_jira_token()?,
            clear_token: false,
        },
        views: config.views.clone(),
    })
}

/// 校验前端提交配置并转换为持久化配置。
pub fn to_stored_config(
    public: &PublicAppConfig,
    current: &StoredAppConfig,
) -> Result<StoredAppConfig, String> {
    let base_url = normalize_base_url(&public.jira.base_url)?; // 规范化JIRA地址
    if !(0.1..=1440.0).contains(&public.jira.refresh_interval) {
        return Err("刷新间隔必须在0.1到1440分钟之间".to_string());
    }
    if public.views.is_empty() {
        return Err("请至少保留一个问题单视图".to_string());
    }

    let mut ids = std::collections::HashSet::new(); // 视图唯一标识集合
    for view in &public.views {
        if view.id.trim().is_empty() || !ids.insert(view.id.clone()) {
            return Err("问题单视图标识无效或重复".to_string());
        }
        if view.name.trim().is_empty() || view.name.chars().count() > 40 {
            return Err("问题单视图名称必须为1到40个字符".to_string());
        }
        if view.jql.trim().is_empty() || view.jql.chars().count() > 2000 {
            return Err("JQL必须为1到2000个字符".to_string());
        }
    }

    Ok(StoredAppConfig {
        version: CONFIG_VERSION,
        jira: crate::models::StoredJiraConfig {
            base_url,
            refresh_interval: public.jira.refresh_interval,
        },
        views: public.views.clone(),
        window_position: current.window_position.clone(),
    })
}

/// 保存或清除JIRA Token。
pub fn update_jira_token(config: &PublicJiraConfig) -> Result<(), String> {
    let entry = token_entry()?; // 系统凭据项
    if config.clear_token {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => return Ok(()),
            Err(error) => return Err(format!("无法清除系统凭据：{error}")),
        }
    }

    if !config.token.is_empty() {
        entry
            .set_password(&config.token)
            .map_err(|error| format!("无法写入系统凭据：{error}"))?;
    }

    Ok(())
}

/// 从系统凭据库读取JIRA Token。
pub fn read_jira_token() -> Result<String, String> {
    token_entry()?.get_password().map_err(|error| match error {
        keyring::Error::NoEntry => "尚未配置JIRA Token".to_string(),
        other => format!("无法读取系统凭据：{other}"),
    })
}

/// 判断系统凭据库是否已有JIRA Token。
pub fn has_jira_token() -> Result<bool, String> {
    match token_entry()?.get_password() {
        Ok(_) => Ok(true),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(error) => Err(format!("无法读取系统凭据：{error}")),
    }
}

/// 规范化并校验JIRA根地址。
pub fn normalize_base_url(value: &str) -> Result<String, String> {
    let mut url = Url::parse(value.trim()).map_err(|_| "JIRA地址格式不正确".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("JIRA地址仅支持HTTP或HTTPS".to_string());
    }
    if url.host_str().is_none() {
        return Err("JIRA地址缺少主机名".to_string());
    }
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.as_str().trim_end_matches('/').to_string())
}

/// 校验持久化配置。
fn validate_stored_config(config: &StoredAppConfig) -> Result<(), String> {
    normalize_base_url(&config.jira.base_url)?;
    if !(0.1..=1440.0).contains(&config.jira.refresh_interval) {
        return Err("配置中的刷新间隔超出范围".to_string());
    }
    if config.views.is_empty() {
        return Err("配置中没有问题单视图".to_string());
    }
    Ok(())
}

/// 创建系统凭据库条目。
fn token_entry() -> Result<Entry, String> {
    Entry::new(KEYRING_SERVICE, TOKEN_ACCOUNT)
        .map_err(|error| format!("无法访问系统凭据库：{error}"))
}

/// 从旧Electron配置迁移JIRA地址和查询视图。
fn migrate_legacy_config(app: &AppHandle, config_path: &Path) -> Result<StoredAppConfig, String> {
    let Some(legacy_path) = find_legacy_config(app) else {
        let config = StoredAppConfig::default();
        write_stored_config(config_path, &config)?;
        return Ok(config);
    };

    let text =
        fs::read_to_string(&legacy_path).map_err(|error| format!("无法读取旧配置：{error}"))?;
    let legacy: LegacyAppConfig =
        serde_json::from_str(&text).map_err(|error| format!("旧配置格式错误：{error}"))?;
    let mut config = StoredAppConfig::default(); // 迁移后的配置

    if let Some(feed) = legacy.feeds.first() {
        config.jira.base_url = extract_base_url(&feed.url)?;
        config.jira.refresh_interval = feed.refresh_interval.clamp(0.1, 1440.0);
    }

    let views: Vec<IssueView> = legacy
        .feeds
        .iter()
        .filter_map(|feed| {
            extract_filter_id(&feed.url).map(|filter_id| IssueView {
                id: Uuid::new_v4().to_string(),
                name: feed.name.clone(),
                jql: format!("filter = {filter_id}"),
            })
        })
        .collect();
    if !views.is_empty() {
        config.views = views;
    }

    write_stored_config(config_path, &config)?;
    scrub_legacy_passwords(&legacy_path, &legacy)?;
    Ok(config)
}

/// 查找旧Electron配置文件。
fn find_legacy_config(app: &AppHandle) -> Option<PathBuf> {
    let mut candidates = Vec::new(); // 已知旧配置位置
    if let Ok(current_dir) = std::env::current_dir() {
        candidates.push(current_dir.join("config.json"));
    }
    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(resource_dir.join("config.json"));
    }
    if let Ok(executable) = std::env::current_exe()
        && let Some(parent) = executable.parent()
    {
        candidates.push(parent.join("config.json"));
    }
    candidates.into_iter().find(|path| path.is_file())
}

/// 从旧RSS地址提取JIRA根地址。
fn extract_base_url(value: &str) -> Result<String, String> {
    let url = Url::parse(value).map_err(|_| "旧配置中的JIRA地址格式不正确".to_string())?;
    let host = url
        .host_str()
        .ok_or_else(|| "旧配置中的JIRA地址缺少主机名".to_string())?;
    let port = url
        .port()
        .map(|value| format!(":{value}"))
        .unwrap_or_default(); // 可选端口
    Ok(format!("{}://{}{}", url.scheme(), host, port))
}

/// 从旧RSS地址提取保存的筛选器编号。
fn extract_filter_id(value: &str) -> Option<String> {
    let marker = "SearchRequest-"; // 旧链接中的筛选器标记
    let start = value.find(marker)? + marker.len();
    let id: String = value[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    (!id.is_empty()).then_some(id)
}

/// 清除旧配置中的明文密码。
fn scrub_legacy_passwords(path: &Path, legacy: &LegacyAppConfig) -> Result<(), String> {
    let mut sanitized = legacy.clone(); // 脱敏后的旧配置
    sanitized
        .feeds
        .iter_mut()
        .for_each(|feed| feed.password.clear());
    let text = serde_json::to_string_pretty(&sanitized)
        .map_err(|error| format!("无法脱敏旧配置：{error}"))?;
    fs::write(path, text).map_err(|error| format!("无法清除旧配置密码：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_filter_id_from_legacy_url() {
        let url = "https://jira.example.com/sr/jira.issueviews:searchrequest-rss/10519/SearchRequest-10519.xml";
        assert_eq!(extract_filter_id(url).as_deref(), Some("10519"));
    }

    #[test]
    fn rejects_non_http_jira_url() {
        assert!(normalize_base_url("file:///tmp/config").is_err());
    }
}
