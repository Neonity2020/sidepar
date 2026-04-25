use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{
    tray::TrayIconBuilder,
    Emitter, Manager,
};
use uuid::Uuid;

// ─── Data Models ────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardItem {
    pub id: String,
    pub content: String,
    pub preview: String,
    pub timestamp: u64,
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptItem {
    pub id: String,
    pub title: String,
    pub content: String,
    #[serde(rename = "categoryId")]
    pub category_id: String,
    #[serde(rename = "createdAt")]
    pub created_at: u64,
    #[serde(rename = "updatedAt")]
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub color: String,
    pub order: u32,
}

// ─── App State ──────────────────────────────────────────

pub struct AppState {
    pub db: Mutex<Connection>,
    pub last_clipboard: Mutex<String>,
    pub last_shown_at: Mutex<u64>,
}

// ─── Database Init ──────────────────────────────────────

fn init_db(app_data_dir: &std::path::Path) -> Connection {
    std::fs::create_dir_all(app_data_dir).expect("Failed to create app data dir");
    let db_path = app_data_dir.join("sidepar.db");
    eprintln!("[SidePar] Database path: {:?}", db_path);

    let conn = Connection::open(&db_path).expect("Failed to open database");

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS clipboard_history (
            id TEXT PRIMARY KEY,
            content TEXT NOT NULL,
            preview TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            pinned INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS categories (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            color TEXT NOT NULL,
            sort_order INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS prompts (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            content TEXT NOT NULL,
            category_id TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_history_timestamp ON clipboard_history(timestamp DESC);
        CREATE INDEX IF NOT EXISTS idx_history_pinned ON clipboard_history(pinned DESC, timestamp DESC);",
    )
    .expect("Failed to create tables");

    // Seed default categories if table is empty
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM categories", [], |row| row.get(0))
        .unwrap_or(0);
    if count == 0 {
        let defaults = [
            ("AI 提示", "#8B5CF6", 0),
            ("代码", "#10B981", 1),
            ("邮件", "#3B82F6", 2),
            ("写作", "#F59E0B", 3),
        ];
        for (name, color, order) in defaults {
            let _ = conn.execute(
                "INSERT INTO categories (id, name, color, sort_order) VALUES (?1, ?2, ?3, ?4)",
                params![Uuid::new_v4().to_string(), name, color, order],
            );
        }
        eprintln!("[SidePar] Seeded default categories");
    }

    conn
}

// ─── Helpers ────────────────────────────────────────────

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn make_preview(content: &str, max_chars: usize) -> String {
    let trimmed = content.trim();
    let char_count = trimmed.chars().count();
    if char_count <= max_chars {
        trimmed.to_string()
    } else {
        let end: usize = trimmed
            .char_indices()
            .nth(max_chars)
            .map(|(i, _)| i)
            .unwrap_or(trimmed.len());
        format!("{}...", &trimmed[..end])
    }
}

// ─── Window Positioning ─────────────────────────────────

fn position_window_right(window: &tauri::WebviewWindow) {
    if let Ok(Some(monitor)) = window.current_monitor() {
        let screen_size = monitor.size();
        let screen_pos = monitor.position();
        let scale = monitor.scale_factor();

        let win_width = 400.0_f64;
        let screen_w = screen_size.width as f64 / scale;
        let screen_h = screen_size.height as f64 / scale;
        let offset_x = screen_pos.x as f64 / scale;
        let offset_y = screen_pos.y as f64 / scale;

        let menu_bar_height = 38.0_f64;
        let dock_margin = 8.0_f64;
        let edge_gap = 6.0_f64;

        let x = offset_x + screen_w - win_width - edge_gap;
        let y = offset_y + menu_bar_height;
        let height = screen_h - menu_bar_height - dock_margin;

        let _ = window.set_size(tauri::LogicalSize::new(win_width, height));
        let _ = window.set_position(tauri::LogicalPosition::new(x, y));
    }
}

fn toggle_panel(window: &tauri::WebviewWindow, app_handle: &tauri::AppHandle) {
    if window.is_visible().unwrap_or(false) {
        let _ = window.emit("panel-hide", ());
        let w = window.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            let _ = w.hide();
        });
    } else {
        if let Some(state) = app_handle.try_state::<AppState>() {
            let mut ts = state.last_shown_at.lock().unwrap();
            *ts = now_ms();
        }
        position_window_right(window);
        let _ = window.show();
        let _ = window.set_focus();
        let _ = window.emit("panel-show", ());
    }
}

// ─── Clipboard History Commands ─────────────────────────

#[tauri::command]
fn get_clipboard_history(state: tauri::State<AppState>) -> Vec<ClipboardItem> {
    let db = state.db.lock().unwrap();
    let mut stmt = db
        .prepare("SELECT id, content, preview, timestamp, pinned FROM clipboard_history ORDER BY pinned DESC, timestamp DESC LIMIT 50")
        .unwrap();
    let items = stmt
        .query_map([], |row| {
            Ok(ClipboardItem {
                id: row.get(0)?,
                content: row.get(1)?,
                preview: row.get(2)?,
                timestamp: row.get(3)?,
                pinned: row.get::<_, i32>(4)? != 0,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    items
}

#[tauri::command]
fn clear_clipboard_history(state: tauri::State<AppState>) {
    let db = state.db.lock().unwrap();
    let _ = db.execute("DELETE FROM clipboard_history", []);
}

#[tauri::command]
fn delete_history_item(id: String, state: tauri::State<AppState>) {
    let db = state.db.lock().unwrap();
    let _ = db.execute("DELETE FROM clipboard_history WHERE id = ?1", params![id]);
}

#[tauri::command]
fn toggle_pin_item(id: String, state: tauri::State<AppState>) {
    let db = state.db.lock().unwrap();
    let _ = db.execute(
        "UPDATE clipboard_history SET pinned = CASE WHEN pinned = 0 THEN 1 ELSE 0 END WHERE id = ?1",
        params![id],
    );
}

#[tauri::command]
fn copy_to_clipboard(text: String, state: tauri::State<AppState>) {
    let mut last = state.last_clipboard.lock().unwrap();
    *last = text;
}

// ─── Prompts Commands ───────────────────────────────────

#[tauri::command]
fn get_prompts(state: tauri::State<AppState>) -> Vec<PromptItem> {
    let db = state.db.lock().unwrap();
    let mut stmt = db
        .prepare("SELECT id, title, content, category_id, created_at, updated_at FROM prompts ORDER BY updated_at DESC")
        .unwrap();
    let items = stmt
        .query_map([], |row| {
            Ok(PromptItem {
                id: row.get(0)?,
                title: row.get(1)?,
                content: row.get(2)?,
                category_id: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    items
}

#[tauri::command]
fn save_prompt(prompt: PromptItem, state: tauri::State<AppState>) {
    let db = state.db.lock().unwrap();
    let _ = db.execute(
        "INSERT OR REPLACE INTO prompts (id, title, content, category_id, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![prompt.id, prompt.title, prompt.content, prompt.category_id, prompt.created_at, prompt.updated_at],
    );
}

#[tauri::command]
fn delete_prompt(id: String, state: tauri::State<AppState>) {
    let db = state.db.lock().unwrap();
    let _ = db.execute("DELETE FROM prompts WHERE id = ?1", params![id]);
}

// ─── Categories Commands ────────────────────────────────

#[tauri::command]
fn get_categories(state: tauri::State<AppState>) -> Vec<Category> {
    let db = state.db.lock().unwrap();
    let mut stmt = db
        .prepare("SELECT id, name, color, sort_order FROM categories ORDER BY sort_order ASC")
        .unwrap();
    let items = stmt
        .query_map([], |row| {
            Ok(Category {
                id: row.get(0)?,
                name: row.get(1)?,
                color: row.get(2)?,
                order: row.get(3)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    items
}

#[tauri::command]
fn save_category(category: Category, state: tauri::State<AppState>) {
    let db = state.db.lock().unwrap();
    let _ = db.execute(
        "INSERT OR REPLACE INTO categories (id, name, color, sort_order) VALUES (?1, ?2, ?3, ?4)",
        params![category.id, category.name, category.color, category.order],
    );
}

#[tauri::command]
fn delete_category(id: String, state: tauri::State<AppState>) {
    let db = state.db.lock().unwrap();
    let _ = db.execute("DELETE FROM categories WHERE id = ?1", params![id]);
}

// ─── App Entry ──────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            get_clipboard_history,
            clear_clipboard_history,
            delete_history_item,
            toggle_pin_item,
            copy_to_clipboard,
            get_prompts,
            save_prompt,
            delete_prompt,
            get_categories,
            save_category,
            delete_category,
        ])
        .setup(|app| {
            // ── Initialize SQLite Database ──
            let app_data_dir = app.path().app_data_dir().expect("Failed to get app data dir");
            let conn = init_db(&app_data_dir);

            // Initialize last_clipboard from current clipboard
            let mut initial_clipboard = String::new();
            use tauri_plugin_clipboard_manager::ClipboardExt;
            if let Ok(text) = app.handle().clipboard().read_text() {
                initial_clipboard = text;
            }

            app.manage(AppState {
                db: Mutex::new(conn),
                last_clipboard: Mutex::new(initial_clipboard),
                last_shown_at: Mutex::new(0),
            });

            // ── macOS: Hide from Dock ──
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let _window = app.get_webview_window("main").unwrap();

            // ── Build Tray Icon ──
            let app_handle = app.handle().clone();
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("SidePar — 剪贴板 & 提示词管理")
                .on_tray_icon_event(move |_tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button_state: tauri::tray::MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let ah = app_handle.clone();
                        std::thread::spawn(move || {
                            std::thread::sleep(std::time::Duration::from_millis(100));
                            if let Some(window) = ah.get_webview_window("main") {
                                toggle_panel(&window, &ah);
                            }
                        });
                    }
                })
                .build(app)?;

            // ── Register Global Shortcut: Cmd+Shift+V ──
            let app_handle2 = app.handle().clone();
            use tauri_plugin_global_shortcut::GlobalShortcutExt;
            app.global_shortcut().on_shortcut(
                "CmdOrCtrl+Shift+V",
                move |_app, _shortcut, event| {
                    if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        if let Some(window) = app_handle2.get_webview_window("main") {
                            toggle_panel(&window, &app_handle2);
                        }
                    }
                },
            )?;

            // ── Clipboard Monitor Thread ──
            let app_handle3 = app.handle().clone();
            std::thread::spawn(move || {
                use tauri_plugin_clipboard_manager::ClipboardExt;
                let mut last_content = {
                    let state = app_handle3.state::<AppState>();
                    let val = state.last_clipboard.lock().unwrap().clone();
                    val
                };
                eprintln!("[SidePar] Clipboard monitor started");
                loop {
                    match app_handle3.clipboard().read_text() {
                        Ok(content) => {
                            if !content.is_empty() && content != last_content {
                                eprintln!("[SidePar] New clipboard: {} chars", content.len());
                                last_content = content.clone();

                                let state = app_handle3.state::<AppState>();
                                {
                                    let mut last = state.last_clipboard.lock().unwrap();
                                    *last = content.clone();
                                }

                                let item = ClipboardItem {
                                    id: Uuid::new_v4().to_string(),
                                    preview: make_preview(&content, 80),
                                    content: content.clone(),
                                    timestamp: now_ms(),
                                    pinned: false,
                                };

                                // Save to SQLite
                                {
                                    let db = state.db.lock().unwrap();
                                    // Remove duplicate content
                                    let _ = db.execute(
                                        "DELETE FROM clipboard_history WHERE content = ?1",
                                        params![content],
                                    );
                                    // Insert new item
                                    let _ = db.execute(
                                        "INSERT INTO clipboard_history (id, content, preview, timestamp, pinned) VALUES (?1, ?2, ?3, ?4, ?5)",
                                        params![item.id, item.content, item.preview, item.timestamp, 0],
                                    );
                                    // Keep max 50 items (delete oldest non-pinned)
                                    let _ = db.execute(
                                        "DELETE FROM clipboard_history WHERE id NOT IN (
                                            SELECT id FROM clipboard_history ORDER BY pinned DESC, timestamp DESC LIMIT 50
                                        )",
                                        [],
                                    );
                                }

                                // Emit to frontend
                                if let Some(window) = app_handle3.get_webview_window("main") {
                                    let _ = window.emit("clipboard-changed", &item);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("[SidePar] Clipboard read error: {:?}", e);
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            });

            // ── Handle window blur (hide panel when clicking outside) ──
            let window2 = app.get_webview_window("main").unwrap();
            let w_clone = window2.clone();
            let app_handle_blur = app.handle().clone();
            window2.on_window_event(move |event| {
                if let tauri::WindowEvent::Focused(false) = event {
                    if let Some(state) = app_handle_blur.try_state::<AppState>() {
                        let ts = state.last_shown_at.lock().unwrap();
                        if now_ms() - *ts < 800 {
                            return;
                        }
                    }
                    let _ = w_clone.emit("panel-hide", ());
                    let wc = w_clone.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(300));
                        let _ = wc.hide();
                    });
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
