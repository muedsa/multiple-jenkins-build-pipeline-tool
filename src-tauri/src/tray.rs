//! 系统托盘：窗口关闭只最小化到托盘，退出程序必须走托盘右键菜单。
//!
//! 这是刻意的交互设计 —— 发布流程可能还在跑，误点关闭不应该直接杀掉程序。
//! 所以：
//! - 点窗口的关闭按钮 → 只是隐藏窗口，程序继续在托盘里跑
//! - 托盘图标左键单击 → 把窗口重新显示出来
//! - 托盘图标右键 → 菜单里选「退出」才真正退出

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WindowEvent};

/// 托盘菜单项 id。
const MENU_SHOW: &str = "tray-show";
const MENU_EXIT: &str = "tray-exit";

/// 主窗口的 label，与 `tauri.conf.json` 里的窗口定义一致。
const MAIN_WINDOW: &str = "main";

/// 把主窗口显示到前台。托盘点击与菜单都用它。
fn show_main_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    // 先 show 再 focus：hide 过的窗口直接 focus 不一定会到前台。
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

/// 创建托盘图标与右键菜单。
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, MENU_SHOW, "显示主窗口", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let exit_item = MenuItem::with_id(app, MENU_EXIT, "退出程序", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &separator, &exit_item])?;

    let mut builder = TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .tooltip("Jenkins 发布管道")
        // 左键单击用来唤出窗口，菜单只在右键弹，避免左键也弹菜单。
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_SHOW => show_main_window(app),
            MENU_EXIT => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // 左键「抬起」时唤出窗口。只看抬起可以避免按下和抬起各触发一次。
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });

    // 用应用图标作为托盘图标；拿不到就退回系统默认图标，
    // 总比没有图标（托盘里看不见）要好。
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

/// 拦截窗口关闭：改成隐藏到托盘，而不是退出程序。
pub fn handle_window_event(window: &tauri::Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        // 阻止默认的关闭（销毁窗口），只把它藏起来。
        api.prevent_close();
        let _ = window.hide();
        eprintln!("[tray] 窗口已最小化到托盘；退出请用托盘右键菜单");
    }
}
