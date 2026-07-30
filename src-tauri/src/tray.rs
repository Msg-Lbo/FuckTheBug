use tauri::{
    Manager,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

/// 创建系统托盘和菜单。
pub fn create_tray(app: &tauri::AppHandle) -> Result<(), String> {
    let toggle_item = MenuItem::with_id(app, "toggle", "显示/隐藏", true, None::<&str>)
        .map_err(|error| format!("无法创建托盘菜单：{error}"))?;
    let settings_item = MenuItem::with_id(app, "settings", "设置", true, None::<&str>)
        .map_err(|error| format!("无法创建托盘菜单：{error}"))?;
    let separator =
        PredefinedMenuItem::separator(app).map_err(|error| format!("无法创建托盘菜单：{error}"))?;
    let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)
        .map_err(|error| format!("无法创建托盘菜单：{error}"))?;
    let menu = Menu::with_items(app, &[&toggle_item, &settings_item, &separator, &quit_item])
        .map_err(|error| format!("无法创建托盘菜单：{error}"))?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| "应用图标不存在".to_string())?;

    TrayIconBuilder::new()
        .icon(icon)
        .tooltip("FuckTheBug")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "toggle" => log_error(toggle_main_window(app)),
            "settings" => log_error(show_settings_window(app)),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } => log_error(toggle_main_window(tray.app_handle())),
            TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => log_error(show_settings_window(tray.app_handle())),
            _ => {}
        })
        .build(app)
        .map_err(|error| format!("无法创建系统托盘：{error}"))?;
    Ok(())
}

/// 切换主窗口显示状态。
fn toggle_main_window(app: &tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    let visible = window
        .is_visible()
        .map_err(|error| format!("无法读取窗口状态：{error}"))?;
    if visible {
        window
            .hide()
            .map_err(|error| format!("无法隐藏主窗口：{error}"))
    } else {
        window
            .show()
            .map_err(|error| format!("无法显示主窗口：{error}"))?;
        window
            .set_focus()
            .map_err(|error| format!("无法聚焦主窗口：{error}"))
    }
}

/// 显示设置窗口。
fn show_settings_window(app: &tauri::AppHandle) -> Result<(), String> {
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

/// 输出托盘事件错误。
fn log_error(result: Result<(), String>) {
    if let Err(error) = result {
        eprintln!("托盘操作失败：{error}");
    }
}
