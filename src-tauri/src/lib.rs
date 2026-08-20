mod jira;
mod models;
mod storage;
mod tray;
mod windows;

use tauri::{Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::{
    models::{IssueItem, IssueView, IssueViewKind, PublicAppConfig},
    storage::{
        AppState, initialize_state, to_public_config, to_stored_config, update_jira_token,
        write_stored_config,
    },
};

/// 将问题单加入自动创建的暂存视图。
#[tauri::command]
fn stash_issue(issue: IssueItem, state: tauri::State<'_, AppState>) -> Result<IssueView, String> {
    let mut config = state
        .config
        .lock()
        .map_err(|_| "配置锁已损坏".to_string())?;
    let stash_index = match config
        .views
        .iter()
        .position(|view| view.kind == IssueViewKind::Stash)
    {
        Some(index) => index,
        None => {
            config.views.push(IssueView {
                id: uuid::Uuid::new_v4().to_string(),
                name: "暂存".to_string(),
                kind: IssueViewKind::Stash,
                jql: String::new(),
                issues: Vec::new(),
            });
            config.views.len() - 1
        }
    };
    let stash_view = &mut config.views[stash_index]; // 唯一暂存视图
    if !stash_view.issues.iter().any(|item| item.key == issue.key) {
        stash_view.issues.insert(0, issue);
    }
    let result = stash_view.clone(); // 写入后的暂存视图
    write_stored_config(&state.config_path, &config)?;
    Ok(result)
}

/// 从暂存视图移除指定问题单。
#[tauri::command]
fn unstash_issue(
    issue_key: String,
    state: tauri::State<'_, AppState>,
) -> Result<IssueView, String> {
    let mut config = state
        .config
        .lock()
        .map_err(|_| "配置锁已损坏".to_string())?;
    let stash_view = config
        .views
        .iter_mut()
        .find(|view| view.kind == IssueViewKind::Stash)
        .ok_or_else(|| "暂存视图不存在".to_string())?;
    stash_view.issues.retain(|issue| issue.key != issue_key);
    let result = stash_view.clone(); // 写入后的暂存视图
    write_stored_config(&state.config_path, &config)?;
    Ok(result)
}

/// 清空暂存视图中的全部问题单。
#[tauri::command]
fn clear_stashed_issues(state: tauri::State<'_, AppState>) -> Result<IssueView, String> {
    let mut config = state
        .config
        .lock()
        .map_err(|_| "配置锁已损坏".to_string())?;
    let stash_view = config
        .views
        .iter_mut()
        .find(|view| view.kind == IssueViewKind::Stash)
        .ok_or_else(|| "暂存视图不存在".to_string())?;
    stash_view.issues.clear();
    let result = stash_view.clone(); // 写入后的暂存视图
    write_stored_config(&state.config_path, &config)?;
    Ok(result)
}

/// 获取不含Token明文的应用配置。
#[tauri::command]
fn get_config(state: tauri::State<'_, AppState>) -> Result<PublicAppConfig, String> {
    let config = state
        .config
        .lock()
        .map_err(|_| "配置锁已损坏".to_string())?;
    to_public_config(&config)
}

/// 校验并保存应用配置。
#[tauri::command]
fn save_config(
    app: tauri::AppHandle,
    config: PublicAppConfig,
    state: tauri::State<'_, AppState>,
) -> Result<PublicAppConfig, String> {
    let mut current = state
        .config
        .lock()
        .map_err(|_| "配置锁已损坏".to_string())?;
    let stored = to_stored_config(&config, &current)?; // 新持久化配置
    update_jira_token(&config.jira)?;
    write_stored_config(&state.config_path, &stored)?;
    *current = stored;
    let public = to_public_config(&current)?; // 保存后的公开配置
    app.emit("config-updated", ())
        .map_err(|error| format!("无法通知配置更新：{error}"))?;
    Ok(public)
}

/// 由安装后的Tauri应用发送Windows系统通知。
#[tauri::command]
fn send_system_notification(
    app: tauri::AppHandle,
    title: String,
    body: String,
) -> Result<bool, String> {
    if title.trim().is_empty() || title.chars().count() > 80 {
        return Err("通知标题必须为1到80个字符".to_string());
    }
    if body.trim().is_empty() || body.chars().count() > 500 {
        return Err("通知正文必须为1到500个字符".to_string());
    }

    let executable =
        std::env::current_exe().map_err(|error| format!("无法读取应用路径：{error}"))?;
    let executable_dir = executable
        .parent()
        .ok_or_else(|| "无法读取应用目录".to_string())?;
    let build_dir = executable_dir.file_name().and_then(|value| value.to_str()); // Rust构建目录
    let target_dir = executable_dir
        .parent()
        .and_then(|path| path.file_name())
        .and_then(|value| value.to_str()); // Rust目标目录
    if target_dir == Some("target") && matches!(build_dir, Some("debug" | "release")) {
        return Ok(false);
    }

    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|error| format!("Windows通知发送失败：{error}"))?;
    Ok(true)
}

/// 启动Tauri应用。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(
            tauri_plugin_updater::Builder::new()
                .header("User-Agent", "FuckTheBug-Updater")
                .expect("更新请求User-Agent配置失败")
                .header("Cache-Control", "no-cache")
                .expect("更新请求缓存策略配置失败")
                .build(),
        )
        .plugin(
            tauri_plugin_opener::Builder::new()
                .open_js_links_on_click(false)
                .build(),
        )
        .setup(|app| {
            let state = app.state::<AppState>();
            let position = state
                .config
                .lock()
                .map_err(|_| std::io::Error::other("配置锁已损坏"))?
                .window_position
                .clone();
            windows::restore_main_window_position(app.handle(), position.as_ref())
                .map_err(std::io::Error::other)?;
            tray::create_tray(app.handle()).map_err(std::io::Error::other)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Err(error) = window.hide() {
                    eprintln!("隐藏窗口失败：{error}");
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            stash_issue,
            unstash_issue,
            clear_stashed_issues,
            send_system_notification,
            jira::fetch_issues,
            jira::test_jira_connection,
            jira::open_external,
            windows::resize_main_window,
            windows::start_main_dragging,
            windows::save_main_window_position,
            windows::open_settings_window,
            windows::close_settings_window,
        ])
        .build(tauri::generate_context!())
        .expect("Tauri应用构建失败");
    let state = initialize_state(app.handle()).expect("应用状态初始化失败");
    app.manage(state);
    app.run(|_, _| {});
}
