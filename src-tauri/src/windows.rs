use tauri::{LogicalSize, Manager, PhysicalPosition, Position, Size, State};

use crate::{
    models::WindowPosition,
    storage::{AppState, write_stored_config},
};

/// 调整主窗口并保证窗口仍位于当前显示器内。
#[tauri::command]
pub fn resize_main_window(app: tauri::AppHandle, width: f64, height: f64) -> Result<(), String> {
    if !(80.0..=900.0).contains(&width) || !(80.0..=800.0).contains(&height) {
        return Err("窗口尺寸超出允许范围".to_string());
    }
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    window
        .set_size(Size::Logical(LogicalSize::new(width, height)))
        .map_err(|error| format!("无法调整窗口尺寸：{error}"))?;

    let monitor = window
        .current_monitor()
        .map_err(|error| format!("无法读取显示器信息：{error}"))?;
    let Some(monitor) = monitor else {
        return Err("主窗口不在任何有效显示器上".to_string());
    };
    let position = window
        .outer_position()
        .map_err(|error| format!("无法读取窗口位置：{error}"))?;
    let size = window
        .outer_size()
        .map_err(|error| format!("无法读取窗口尺寸：{error}"))?;
    let monitor_position = monitor.position(); // 显示器左上角
    let monitor_size = monitor.size(); // 显示器物理尺寸
    let max_x = monitor_position.x + monitor_size.width as i32 - size.width as i32;
    let max_y = monitor_position.y + monitor_size.height as i32 - size.height as i32;
    let x = position
        .x
        .clamp(monitor_position.x, max_x.max(monitor_position.x));
    let y = position
        .y
        .clamp(monitor_position.y, max_y.max(monitor_position.y));

    if x != position.x || y != position.y {
        window
            .set_position(Position::Physical(PhysicalPosition::new(x, y)))
            .map_err(|error| format!("无法修正窗口位置：{error}"))?;
    }
    Ok(())
}

/// 开始系统级主窗口拖动。
#[tauri::command]
pub fn start_main_dragging(app: tauri::AppHandle) -> Result<(), String> {
    app.get_webview_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?
        .start_dragging()
        .map_err(|error| format!("无法拖动窗口：{error}"))
}

/// 保存主窗口物理坐标。
#[tauri::command]
pub fn save_main_window_position(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    let position = window
        .outer_position()
        .map_err(|error| format!("无法读取窗口位置：{error}"))?;
    let mut config = state
        .config
        .lock()
        .map_err(|_| "配置锁已损坏".to_string())?;
    config.window_position = Some(WindowPosition {
        x: position.x,
        y: position.y,
    });
    write_stored_config(&state.config_path, &config)
}

/// 显示并聚焦设置窗口。
#[tauri::command]
pub fn open_settings_window(app: tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("settings")
        .ok_or_else(|| "设置窗口不存在".to_string())?;
    window
        .show()
        .map_err(|error| format!("无法显示设置窗口：{error}"))?;
    window
        .unminimize()
        .map_err(|error| format!("无法恢复设置窗口：{error}"))?;
    window
        .set_focus()
        .map_err(|error| format!("无法聚焦设置窗口：{error}"))
}

/// 隐藏设置窗口。
#[tauri::command]
pub fn close_settings_window(app: tauri::AppHandle) -> Result<(), String> {
    app.get_webview_window("settings")
        .ok_or_else(|| "设置窗口不存在".to_string())?
        .hide()
        .map_err(|error| format!("无法隐藏设置窗口：{error}"))
}

/// 恢复已保存的主窗口位置。
pub fn restore_main_window_position(
    app: &tauri::AppHandle,
    position: Option<&WindowPosition>,
) -> Result<(), String> {
    let Some(position) = position else {
        return Ok(());
    };
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    window
        .set_position(Position::Physical(PhysicalPosition::new(
            position.x, position.y,
        )))
        .map_err(|error| format!("无法恢复窗口位置：{error}"))
}
