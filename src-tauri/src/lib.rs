use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, State, WebviewWindow,
};

const APP_BUNDLE_ID: &str = "com.weiguang.lingdongdao";
const APP_PATH: &str = "/Applications/LingDongDao.app";
const HOMEBREW_TAP: &str = "Gloomysunday28/message-kit";
const HOMEBREW_TAP_URL: &str = "https://github.com/Gloomysunday28/message-kit";
const HOMEBREW_CASK: &str = "lingdongdao";
const WEBUI_MANIFEST_URL: &str =
    "https://github.com/Gloomysunday28/message-kit/releases/download/webui/webui.json";
const WEBUI_HTML_URL: &str =
    "https://github.com/Gloomysunday28/message-kit/releases/download/webui/webui.html";

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    #[link_name = "CGEventSourceSecondsSinceLastEventType"]
    fn cg_event_source_seconds_since_last_event_type(state_id: i32, event_type: u32) -> f64;
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FocusedApp {
    name: String,
    bundle_id: String,
    pid: i32,
    icon_data_url: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppUpdateInfo {
    current_version: String,
    latest_version: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NotchMetrics {
    width: f64,
    height: f64,
    has_notch: bool,
}

#[derive(Serialize, Deserialize, Clone)]
struct WebuiManifest {
    version: String,
    sha256: String,
    #[serde(rename = "minAppVersion", default)]
    min_app_version: String,
    #[serde(rename = "maxAppVersion", default)]
    max_app_version: String,
}

#[derive(Serialize)]
struct WebuiInfo {
    version: String,
    html: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct WebuiProgress {
    version: String,
    phase: String,
    percent: u8,
}

#[derive(Serialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
struct CodexActivity {
    visible: bool,
    active: bool,
    status: String,
    message: String,
    phase: String,
    updated_at: u64,
}

impl Default for CodexActivity {
    fn default() -> Self {
        Self {
            visible: false,
            active: false,
            status: "等待 Codex".to_string(),
            message: String::new(),
            phase: "idle".to_string(),
            updated_at: 0,
        }
    }
}

#[derive(Default)]
struct CodexActivityState(Mutex<CodexActivity>);

#[tauri::command]
fn codex_current(state: State<'_, CodexActivityState>) -> CodexActivity {
    state
        .0
        .lock()
        .map(|activity| activity.clone())
        .unwrap_or_default()
}

#[cfg(target_os = "macos")]
#[tauri::command]
fn system_idle_seconds() -> f64 {
    // Combined session + any input event reports keyboard, pointer and trackpad inactivity.
    unsafe { cg_event_source_seconds_since_last_event_type(0, u32::MAX) }.max(0.0)
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
fn system_idle_seconds() -> f64 {
    0.0
}

#[cfg(target_os = "macos")]
fn native_icon_data_url(
    app_handle: &AppHandle,
    icon: &objc2_app_kit::NSImage,
    identity: &str,
) -> Option<String> {
    let tiff = icon.TIFFRepresentation()?;
    let cache_root = app_handle.path().app_cache_dir().ok()?.join("app-icons-v2");
    fs::create_dir_all(&cache_root).ok()?;
    let cache_key = sha256_hex(identity.as_bytes());
    let png_path = cache_root.join(format!("{cache_key}.png"));
    if !png_path.exists() {
        let tiff_path = cache_root.join(format!("{cache_key}.tiff"));
        fs::write(&tiff_path, tiff.to_vec()).ok()?;
        let converted = Command::new("sips")
            .args(["-s", "format", "png"])
            .arg(&tiff_path)
            .arg("--out")
            .arg(&png_path)
            .output()
            .ok()
            .is_some_and(|output| output.status.success());
        let _ = fs::remove_file(tiff_path);
        if !converted {
            return None;
        }
    }
    fs::read(png_path)
        .ok()
        .map(|bytes| format!("data:image/png;base64,{}", BASE64.encode(bytes)))
}

#[cfg(target_os = "macos")]
#[tauri::command]
fn frontmost_app(
    app_handle: AppHandle,
    previous_bundle_id: Option<String>,
) -> Result<Option<FocusedApp>, String> {
    use objc2_app_kit::NSWorkspace;

    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace
        .frontmostApplication()
        .ok_or_else(|| "暂时没有可识别的前台 App".to_string())?;
    let name = app
        .localizedName()
        .map(|value| value.to_string())
        .unwrap_or_else(|| "未知 App".to_string());
    let bundle_id = app
        .bundleIdentifier()
        .map(|value| value.to_string())
        .unwrap_or_default();
    let pid = app.processIdentifier();

    // 点击灵动岛本身时，保留用户刚才使用的真实 App。
    // 开发版没有 bundle id，所以同时用当前进程 PID 判断。
    if pid == std::process::id() as i32 || bundle_id == APP_BUNDLE_ID {
        return Ok(None);
    }

    let include_icon = previous_bundle_id.as_deref() != Some(bundle_id.as_str());
    let icon_data_url = if include_icon {
        let icon = app.icon().or_else(|| {
            let bundle_path = app.bundleURL()?.path()?;
            Some(workspace.iconForFile(&bundle_path))
        });
        let identity = format!("native-v2:{bundle_id}:{name}:{pid}");
        icon.and_then(|icon| native_icon_data_url(&app_handle, &icon, &identity))
    } else {
        None
    };

    Ok(Some(FocusedApp {
        name,
        bundle_id,
        pid,
        icon_data_url,
    }))
}

#[cfg(target_os = "macos")]
#[tauri::command]
fn codex_host_icon(app_handle: AppHandle) -> Option<String> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::NSString;

    let home_application = app_handle
        .path()
        .home_dir()
        .ok()
        .map(|home| home.join("Applications").join("ChatGPT.app"));
    let candidates = [
        Some(PathBuf::from("/Applications/ChatGPT.app")),
        home_application,
        Some(PathBuf::from("/Applications/Codex.app")),
    ];
    let path = candidates
        .into_iter()
        .flatten()
        .find(|candidate| candidate.exists())?;
    let workspace = NSWorkspace::sharedWorkspace();
    let path_string = NSString::from_str(path.to_str()?);
    let icon = workspace.iconForFile(&path_string);
    native_icon_data_url(&app_handle, &icon, "native-v2:com.openai.codex")
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
fn codex_host_icon() -> Option<String> {
    None
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
fn frontmost_app(_previous_bundle_id: Option<String>) -> Result<Option<FocusedApp>, String> {
    Err("灵动岛只支持 macOS".to_string())
}

#[cfg(not(target_os = "macos"))]
fn attach_window_to_screen_top(window: &WebviewWindow) -> Result<(), String> {
    let Some(monitor) = window
        .current_monitor()
        .map_err(|error| error.to_string())?
        .or_else(|| window.primary_monitor().ok().flatten())
    else {
        return Ok(());
    };
    let scale = monitor.scale_factor();
    let monitor_position = monitor.position();
    let monitor_size = monitor.size();
    let window_size = window
        .outer_size()
        .map_err(|error| error.to_string())?;
    let x = monitor_position.x
        + ((monitor_size.width.saturating_sub(window_size.width)) / 2) as i32;
    let y = monitor_position.y + (5.0 * scale).round() as i32;
    window
        .set_position(tauri::PhysicalPosition::new(x, y))
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
fn attach_window_to_screen_top(window: &WebviewWindow) -> Result<(), String> {
    let frame = {
        let pointer = window.ns_window().map_err(|error| error.to_string())?;
        unsafe {
            let ns_window: &objc2_app_kit::NSWindow = &*pointer.cast();
            ns_window.frame()
        }
    };
    set_native_window_frame(window, frame.size.width, frame.size.height, false)
}

#[cfg(target_os = "macos")]
fn set_native_window_frame(
    window: &WebviewWindow,
    width: f64,
    height: f64,
    animate: bool,
) -> Result<(), String> {
    set_native_window_frame_with_offset(window, width, height, 0.0, animate)
}

#[cfg(target_os = "macos")]
fn set_native_window_frame_with_offset(
    window: &WebviewWindow,
    width: f64,
    height: f64,
    top_offset: f64,
    animate: bool,
) -> Result<(), String> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let pointer = window.ns_window().map_err(|error| error.to_string())?;
    let marker = MainThreadMarker::new().ok_or_else(|| "窗口操作必须在主线程执行".to_string())?;
    unsafe {
        let ns_window: &objc2_app_kit::NSWindow = &*pointer.cast();
        let screen = ns_window
            .screen()
            .or_else(|| NSScreen::mainScreen(marker))
            .ok_or_else(|| "无法识别当前屏幕".to_string())?;
        let screen_frame = screen.frame();
        let x = screen_frame.origin.x
            + (screen_frame.size.width - width) / 2.0;
        let top = screen_frame.origin.y + screen_frame.size.height;
        let target = NSRect::new(
            NSPoint::new(x, top - top_offset - height),
            NSSize::new(width, height),
        );
        ns_window.setFrame_display_animate(target, true, animate);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn read_notch_metrics(window: &WebviewWindow) -> Result<NotchMetrics, String> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;

    let pointer = window.ns_window().map_err(|error| error.to_string())?;
    let marker = MainThreadMarker::new().ok_or_else(|| "窗口操作必须在主线程执行".to_string())?;
    unsafe {
        let ns_window: &objc2_app_kit::NSWindow = &*pointer.cast();
        let screen = ns_window
            .screen()
            .or_else(|| NSScreen::mainScreen(marker))
            .ok_or_else(|| "无法识别当前屏幕".to_string())?;
        let frame = screen.frame();
        let left = screen.auxiliaryTopLeftArea();
        let right = screen.auxiliaryTopRightArea();
        let safe = screen.safeAreaInsets();
        let gap = (right.origin.x - (left.origin.x + left.size.width)).max(0.0);
        let has_notch = safe.top > 0.0 && gap > 0.0;
        Ok(NotchMetrics {
            width: if has_notch { gap + 10.0 } else { 94.0 },
            height: if has_notch {
                safe.top.max(30.0)
            } else {
                (frame.size.height - screen.visibleFrame().size.height).clamp(30.0, 38.0)
            },
            has_notch,
        })
    }
}

#[cfg(target_os = "macos")]
#[tauri::command]
fn notch_metrics(window: WebviewWindow) -> Result<NotchMetrics, String> {
    read_notch_metrics(&window)
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
fn notch_metrics(_window: WebviewWindow) -> Result<NotchMetrics, String> {
    Ok(NotchMetrics {
        width: 94.0,
        height: 34.0,
        has_notch: false,
    })
}

#[tauri::command]
fn set_details_visible(app: AppHandle, visible: bool) -> Result<(), String> {
    let details = app
        .get_webview_window("details")
        .ok_or_else(|| "找不到详情窗口".to_string())?;
    #[cfg(target_os = "macos")]
    {
        let main = app
            .get_webview_window("main")
            .ok_or_else(|| "找不到灵动岛窗口".to_string())?;
        let metrics = read_notch_metrics(&main)?;
        set_native_window_frame_with_offset(
            &details,
            390.0,
            148.0,
            metrics.height + 34.0,
            false,
        )?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        details
            .set_position(tauri::LogicalPosition::new(0.0, 68.0))
            .map_err(|error| error.to_string())?;
    }
    if visible {
        details.show().map_err(|error| error.to_string())
    } else {
        details.hide().map_err(|error| error.to_string())
    }
}

#[tauri::command]
fn hide_island(app: AppHandle) -> Result<(), String> {
    if let Some(details) = app.get_webview_window("details") {
        let _ = details.hide();
    }
    if let Some(main) = app.get_webview_window("main") {
        main.hide().map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn run_brew(args: &[&str]) -> Result<String, String> {
    let candidates = ["/opt/homebrew/bin/brew", "/usr/local/bin/brew", "brew"];
    let mut last_error = "未找到 Homebrew，请先安装 Homebrew".to_string();
    for binary in candidates {
        match Command::new(binary).args(args).output() {
            Ok(output) if output.status.success() => {
                return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
            }
            Ok(output) => {
                let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
                if !message.is_empty() {
                    last_error = message;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => last_error = error.to_string(),
        }
    }
    Err(last_error)
}

fn parse_version(value: &str) -> Vec<u64> {
    value
        .trim()
        .trim_start_matches('v')
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

fn version_at_least(current: &str, required: &str) -> bool {
    let current = parse_version(current);
    let required = parse_version(required);
    for index in 0..current.len().max(required.len()) {
        let left = current.get(index).copied().unwrap_or(0);
        let right = required.get(index).copied().unwrap_or(0);
        if left != right {
            return left > right;
        }
    }
    true
}

fn latest_cask_version(json: &str) -> Result<String, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|error| format!("读取 Homebrew 信息失败：{error}"))?;
    value
        .get("casks")
        .and_then(|value| value.as_array())
        .and_then(|casks| casks.first())
        .and_then(|cask| cask.get("version"))
        .and_then(|version| version.as_str())
        .filter(|version| !version.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| "Homebrew 中还没有可用版本".to_string())
}

#[tauri::command]
async fn check_homebrew_update() -> Result<Option<AppUpdateInfo>, String> {
    run_brew(&["tap", HOMEBREW_TAP, HOMEBREW_TAP_URL])?;
    run_brew(&["update", "--quiet"])?;
    let info = run_brew(&["info", "--cask", "--json=v2", HOMEBREW_CASK])?;
    let latest = latest_cask_version(&info)?;
    let current = env!("CARGO_PKG_VERSION").to_string();
    if version_at_least(&current, &latest) {
        return Ok(None);
    }
    Ok(Some(AppUpdateInfo {
        current_version: current,
        latest_version: latest,
    }))
}

#[tauri::command]
async fn install_homebrew_update(app: AppHandle) -> Result<(), String> {
    run_brew(&["tap", HOMEBREW_TAP, HOMEBREW_TAP_URL])?;
    let installed = run_brew(&["list", "--cask", HOMEBREW_CASK]).is_ok();
    if installed {
        run_brew(&["upgrade", "--cask", HOMEBREW_CASK])?;
    } else {
        run_brew(&["install", "--cask", "--force", HOMEBREW_CASK])?;
    }

    if PathBuf::from(APP_PATH).exists() {
        let _ = Command::new("xattr")
            .args(["-dr", "com.apple.quarantine", APP_PATH])
            .output();
        Command::new("open")
            .args(["-n", APP_PATH])
            .output()
            .map_err(|error| format!("重新打开 App 失败：{error}"))?;
        app.exit(0);
    } else {
        app.restart();
    }
    Ok(())
}

fn webui_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| format!("无法定位应用数据目录：{error}"))?
        .join("webui"))
}

fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn webui_manifest_supported(manifest: &WebuiManifest) -> bool {
    (manifest.min_app_version.is_empty()
        || version_at_least(env!("CARGO_PKG_VERSION"), &manifest.min_app_version))
        && (manifest.max_app_version.is_empty()
            || version_at_least(&manifest.max_app_version, env!("CARGO_PKG_VERSION")))
}

fn webui_asset_url(base: &str) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    format!("{base}?t={timestamp}")
}

fn installed_webui(app: &AppHandle) -> Option<(WebuiManifest, String)> {
    let directory = webui_dir(app).ok()?;
    let manifest: WebuiManifest =
        serde_json::from_str(&fs::read_to_string(directory.join("manifest.json")).ok()?).ok()?;
    if manifest.version.is_empty()
        || manifest.sha256.is_empty()
        || !webui_manifest_supported(&manifest)
    {
        let _ = fs::remove_dir_all(&directory);
        return None;
    }
    let html = fs::read_to_string(directory.join("current.html")).ok()?;
    if sha256_hex(html.as_bytes()) != manifest.sha256.to_lowercase() {
        let _ = fs::remove_dir_all(&directory);
        return None;
    }
    Some((manifest, html))
}

#[tauri::command]
async fn webui_current(app: AppHandle) -> Option<WebuiInfo> {
    if cfg!(debug_assertions) {
        return None;
    }
    let (manifest, html) = installed_webui(&app)?;
    Some(WebuiInfo {
        version: manifest.version,
        html,
    })
}

fn emit_webui_progress(app: &AppHandle, version: &str, phase: &str, percent: u8) {
    let _ = app.emit(
        "lingdongdao:webui-progress",
        WebuiProgress {
            version: version.to_string(),
            phase: phase.to_string(),
            percent: percent.min(100),
        },
    );
}

#[tauri::command]
async fn check_webui_update(app: AppHandle) -> Result<Option<String>, String> {
    if cfg!(debug_assertions) {
        return Ok(None);
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| format!("初始化更新服务失败：{error}"))?;
    let manifest: WebuiManifest = client
        .get(webui_asset_url(WEBUI_MANIFEST_URL))
        .header("Cache-Control", "no-cache")
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|error| format!("获取界面更新失败：{error}"))?
        .json()
        .await
        .map_err(|error| format!("读取界面更新失败：{error}"))?;

    if manifest.version.is_empty()
        || manifest.sha256.is_empty()
        || !webui_manifest_supported(&manifest)
    {
        return Ok(None);
    }
    if installed_webui(&app)
        .is_some_and(|(installed, _)| installed.version == manifest.version)
    {
        return Ok(None);
    }

    emit_webui_progress(&app, &manifest.version, "downloading", 5);
    let html = client
        .get(webui_asset_url(WEBUI_HTML_URL))
        .header("Cache-Control", "no-cache")
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|error| format!("下载界面更新失败：{error}"))?
        .text()
        .await
        .map_err(|error| format!("读取界面更新失败：{error}"))?;

    emit_webui_progress(&app, &manifest.version, "verifying", 90);
    if sha256_hex(html.as_bytes()) != manifest.sha256.to_lowercase() {
        return Err("界面更新校验失败".to_string());
    }

    let directory = webui_dir(&app)?;
    fs::create_dir_all(&directory).map_err(|error| format!("创建更新目录失败：{error}"))?;
    let temporary = directory.join("current.html.tmp");
    fs::write(&temporary, &html).map_err(|error| format!("保存界面更新失败：{error}"))?;
    fs::rename(&temporary, directory.join("current.html"))
        .map_err(|error| format!("应用界面更新失败：{error}"))?;
    fs::write(
        directory.join("manifest.json"),
        serde_json::to_string(&manifest).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("保存更新清单失败：{error}"))?;
    emit_webui_progress(&app, &manifest.version, "ready", 100);
    Ok(Some(manifest.version))
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn newest_codex_session(root: &Path) -> Option<PathBuf> {
    let mut directories = vec![root.to_path_buf()];
    let mut newest: Option<(SystemTime, PathBuf)> = None;

    while let Some(directory) = directories.pop() {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                directories.push(path);
                continue;
            }
            if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(modified) = entry.metadata().and_then(|metadata| metadata.modified()) else {
                continue;
            };
            if newest
                .as_ref()
                .is_none_or(|(current, _)| modified > *current)
            {
                newest = Some((modified, path));
            }
        }
    }

    newest.map(|(_, path)| path)
}

fn codex_session_id(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    if stem.len() < 36 {
        return None;
    }
    Some(stem[stem.len() - 36..].to_string())
}

fn clean_codex_message(message: &str) -> String {
    let compact = message.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut output = compact.chars().take(220).collect::<String>();
    if compact.chars().count() > 220 {
        output.push('…');
    }
    output
}

fn poll_codex_output_deltas(
    connection: &Connection,
    last_log_id: &mut i64,
    session_id: Option<&str>,
    accepting_response: &mut bool,
    stream_item_id: &mut String,
    activity: &mut CodexActivity,
) -> bool {
    let Ok(mut statement) = connection.prepare(
        "SELECT id, feedback_log_body
         FROM logs
         WHERE id > ?1
           AND target = 'codex_api::sse::responses'
           AND feedback_log_body LIKE 'SSE event: {%'
         ORDER BY id
         LIMIT 4000",
    ) else {
        return false;
    };
    let Ok(rows) = statement.query_map([*last_log_id], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    }) else {
        return false;
    };
    let records = rows.filter_map(Result::ok).collect::<Vec<_>>();
    drop(statement);

    let mut changed = false;
    for (id, body) in records {
        *last_log_id = (*last_log_id).max(id);
        let Some(json) = body.strip_prefix("SSE event: ") else {
            continue;
        };
        let Ok(event) = serde_json::from_str::<serde_json::Value>(json) else {
            continue;
        };
        match event.get("type").and_then(|value| value.as_str()) {
            Some("response.created") => {
                let prompt_cache_key = event
                    .pointer("/response/prompt_cache_key")
                    .and_then(|value| value.as_str());
                *accepting_response =
                    session_id.is_some() && prompt_cache_key == session_id;
                if *accepting_response {
                    activity.active = true;
                    activity.status = "正在思考".to_string();
                    activity.phase = "thinking".to_string();
                    changed = true;
                }
            }
            Some("response.output_text.delta") if *accepting_response => {
                let item_id = event
                    .get("item_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                if !item_id.is_empty() && stream_item_id != item_id {
                    stream_item_id.clear();
                    stream_item_id.push_str(item_id);
                    activity.message.clear();
                }
                if let Some(delta) = event.get("delta").and_then(|value| value.as_str()) {
                    activity.message.push_str(delta);
                    let character_count = activity.message.chars().count();
                    if character_count > 1600 {
                        activity.message = activity
                            .message
                            .chars()
                            .skip(character_count - 1600)
                            .collect();
                    }
                    activity.visible = true;
                    activity.active = true;
                    activity.status = "正在回复".to_string();
                    activity.phase = "stream".to_string();
                    changed = true;
                }
            }
            Some("response.completed" | "response.failed" | "response.incomplete")
                if *accepting_response =>
            {
                *accepting_response = false;
            }
            _ => {}
        }
    }

    if changed {
        activity.updated_at = unix_millis();
    }
    changed
}

fn codex_tool_status(name: &str) -> &'static str {
    let name = name.to_ascii_lowercase();
    if name.contains("apply_patch") || name.contains("edit") || name.contains("write") {
        "正在修改"
    } else if name.contains("web") || name.contains("search") || name.contains("browser") {
        "正在查询"
    } else if name.contains("view") || name.contains("read") {
        "正在查看"
    } else if name.contains("exec") || name.contains("command") || name.contains("stdin") {
        "正在运行"
    } else {
        "正在处理"
    }
}

fn apply_codex_record(
    line: &str,
    activity: &mut CodexActivity,
    completed_at: &mut Option<Instant>,
) -> bool {
    let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
        return false;
    };
    let Some(payload) = record.get("payload") else {
        return false;
    };
    let record_type = record.get("type").and_then(|value| value.as_str());
    let payload_type = payload.get("type").and_then(|value| value.as_str());
    let before = activity.clone();

    match (record_type, payload_type) {
        (Some("event_msg"), Some("task_started")) => {
            activity.visible = false;
            activity.active = true;
            activity.status = "正在思考".to_string();
            activity.message.clear();
            activity.phase = "thinking".to_string();
            *completed_at = None;
        }
        (Some("event_msg"), Some("agent_reasoning")) if activity.active => {
            activity.status = "正在思考".to_string();
            activity.phase = "thinking".to_string();
        }
        (Some("event_msg"), Some("agent_message")) => {
            if activity.message.is_empty() || activity.phase != "stream" {
                if let Some(message) = payload.get("message").and_then(|value| value.as_str()) {
                    let message = clean_codex_message(message);
                    if !message.is_empty() {
                        activity.message = message;
                    }
                }
            }
            activity.visible = true;
            if payload.get("phase").and_then(|value| value.as_str()) == Some("final_answer") {
                activity.active = false;
                activity.status = "回复完成".to_string();
                activity.phase = "answer".to_string();
                *completed_at = Some(Instant::now());
            } else {
                activity.active = true;
                activity.status = "正在回复".to_string();
                activity.phase = "message".to_string();
            }
        }
        (Some("event_msg"), Some("task_complete")) => {
            if activity.message.is_empty() {
                if let Some(message) = payload
                    .get("last_agent_message")
                    .and_then(|value| value.as_str())
                {
                    activity.message = clean_codex_message(message);
                }
            }
            activity.visible = !activity.message.is_empty();
            activity.active = false;
            activity.status = "已完成".to_string();
            activity.phase = "done".to_string();
            *completed_at = Some(Instant::now());
        }
        (Some("event_msg"), Some("turn_aborted")) => {
            activity.visible = !activity.message.is_empty();
            activity.active = false;
            activity.status = "已停止".to_string();
            activity.phase = "stopped".to_string();
            *completed_at = Some(Instant::now());
        }
        (Some("response_item"), Some("custom_tool_call" | "function_call"))
            if activity.active =>
        {
            if let Some(name) = payload.get("name").and_then(|value| value.as_str()) {
                activity.status = codex_tool_status(name).to_string();
                activity.phase = "working".to_string();
            }
        }
        (
            Some("response_item"),
            Some("custom_tool_call_output" | "function_call_output"),
        ) if activity.active => {
            activity.status = "正在整理".to_string();
            activity.phase = "thinking".to_string();
        }
        (Some("event_msg"), Some("patch_apply_begin" | "patch_apply_end")) if activity.active => {
            activity.status = "正在修改".to_string();
            activity.phase = "working".to_string();
        }
        _ => {}
    }

    if *activity != before {
        activity.updated_at = unix_millis();
        true
    } else {
        false
    }
}

fn publish_codex_activity(app: &AppHandle, activity: &CodexActivity) {
    if let Some(state) = app.try_state::<CodexActivityState>() {
        if let Ok(mut current) = state.0.lock() {
            *current = activity.clone();
        }
    }
    let _ = app.emit("lingdongdao:codex-activity", activity.clone());
}

fn start_codex_watcher(app: AppHandle, sessions_root: PathBuf) {
    thread::spawn(move || {
        const INITIAL_TAIL_BYTES: u64 = 16 * 1024 * 1024;
        let mut watched_path: Option<PathBuf> = None;
        let mut offset = 0_u64;
        let mut partial = String::new();
        let mut activity = CodexActivity::default();
        let mut completed_at: Option<Instant> = None;
        let mut last_scan = Instant::now() - Duration::from_secs(2);
        let logs_path = sessions_root
            .parent()
            .map(|path| path.join("logs_2.sqlite"));
        let mut logs_connection: Option<Connection> = None;
        let mut last_log_id = 0_i64;
        let mut accepting_response = false;
        let mut stream_item_id = String::new();

        loop {
            if logs_connection.is_none() {
                if let Some(path) = logs_path.as_ref() {
                    if let Ok(connection) = Connection::open_with_flags(
                        path,
                        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
                    ) {
                        last_log_id = connection
                            .query_row("SELECT COALESCE(MAX(id), 0) FROM logs", [], |row| {
                                row.get(0)
                            })
                            .unwrap_or(0);
                        logs_connection = Some(connection);
                    }
                }
            }

            if last_scan.elapsed() >= Duration::from_millis(900) {
                let newest = newest_codex_session(&sessions_root);
                if newest != watched_path {
                    watched_path = newest;
                    partial.clear();
                    completed_at = None;
                    accepting_response = false;
                    stream_item_id.clear();
                    activity = CodexActivity::default();
                    if let Some(path) = watched_path.as_ref() {
                        let length = fs::metadata(path).map(|value| value.len()).unwrap_or(0);
                        offset = length.saturating_sub(INITIAL_TAIL_BYTES);
                    } else {
                        offset = 0;
                    }
                }
                last_scan = Instant::now();
            }

            let mut changed = false;
            if let Some(path) = watched_path.as_ref() {
                if let Ok(metadata) = fs::metadata(path) {
                    if metadata.len() < offset {
                        offset = 0;
                        partial.clear();
                        activity = CodexActivity::default();
                    }
                    if metadata.len() > offset {
                        if let Ok(mut file) = fs::File::open(path) {
                            let starting_mid_file = offset > 0 && partial.is_empty();
                            if file.seek(SeekFrom::Start(offset)).is_ok() {
                                let mut chunk = String::new();
                                if file.read_to_string(&mut chunk).is_ok() {
                                    offset = metadata.len();
                                    if starting_mid_file {
                                        if let Some(index) = chunk.find('\n') {
                                            chunk.drain(..=index);
                                        } else {
                                            chunk.clear();
                                        }
                                    }
                                    partial.push_str(&chunk);
                                    if let Some(last_newline) = partial.rfind('\n') {
                                        let complete = partial[..=last_newline].to_string();
                                        partial.drain(..=last_newline);
                                        for line in complete.lines() {
                                            changed |= apply_codex_record(
                                                line,
                                                &mut activity,
                                                &mut completed_at,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if let Some(connection) = logs_connection.as_ref() {
                let session_id = watched_path.as_deref().and_then(codex_session_id);
                changed |= poll_codex_output_deltas(
                    connection,
                    &mut last_log_id,
                    session_id.as_deref(),
                    &mut accepting_response,
                    &mut stream_item_id,
                    &mut activity,
                );
            }

            if completed_at
                .is_some_and(|instant| instant.elapsed() >= Duration::from_millis(1200))
                && activity.visible
            {
                activity.visible = false;
                activity.phase = "idle".to_string();
                activity.updated_at = unix_millis();
                completed_at = None;
                changed = true;
            }

            if changed {
                publish_codex_activity(&app, &activity);
            }
            thread::sleep(Duration::from_millis(220));
        }
    });
}

fn show_island(app: &AppHandle) {
    if let Some(details) = app.get_webview_window("details") {
        let _ = details.hide();
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = attach_window_to_screen_top(&window);
    }
}

#[cfg(target_os = "macos")]
fn configure_macos_window(window: &WebviewWindow) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSWindow, NSWindowCollectionBehavior,
        NSStatusWindowLevel,
    };

    if let Ok(pointer) = window.ns_window() {
        unsafe {
            let ns_window: &NSWindow = &*pointer.cast();
            let mut behavior = ns_window.collectionBehavior();
            behavior |= NSWindowCollectionBehavior::CanJoinAllSpaces;
            behavior |= NSWindowCollectionBehavior::FullScreenAuxiliary;
            behavior |= NSWindowCollectionBehavior::Stationary;
            ns_window.setCollectionBehavior(behavior);
            ns_window.setLevel(NSStatusWindowLevel);
        }
    }
    if let Some(marker) = MainThreadMarker::new() {
        let application = NSApplication::sharedApplication(marker);
        application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            app.manage(CodexActivityState::default());
            if let Ok(home) = app.path().home_dir() {
                start_codex_watcher(app.handle().clone(), home.join(".codex").join("sessions"));
            }

            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_focusable(false);
                #[cfg(target_os = "macos")]
                configure_macos_window(&window);
                #[cfg(target_os = "macos")]
                if let Ok(metrics) = read_notch_metrics(&window) {
                    let _ = set_native_window_frame(
                        &window,
                        metrics.width,
                        metrics.height + 34.0,
                        false,
                    );
                }
                let _ = attach_window_to_screen_top(&window);
            }
            if let Some(details) = app.get_webview_window("details") {
                let _ = details.set_focusable(false);
                #[cfg(target_os = "macos")]
                {
                    configure_macos_window(&details);
                    if let Some(main) = app.get_webview_window("main") {
                        if let Ok(metrics) = read_notch_metrics(&main) {
                            let _ = set_native_window_frame_with_offset(
                                &details,
                                390.0,
                                148.0,
                                metrics.height + 34.0,
                                false,
                            );
                        }
                    }
                }
            }

            let show = MenuItem::with_id(app, "show", "显示灵动岛", true, None::<&str>)?;
            let hide = MenuItem::with_id(app, "hide", "隐藏灵动岛", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &hide, &quit])?;
            TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("灵动岛")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_island(app),
                    "hide" => {
                        if let Some(details) = app.get_webview_window("details") {
                            let _ = details.hide();
                        }
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.hide();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                if let Some(details) = app.get_webview_window("details") {
                    let _ = details.hide();
                }
                if let Some(main) = app.get_webview_window("main") {
                    let _ = main.hide();
                }
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            frontmost_app,
            codex_host_icon,
            notch_metrics,
            set_details_visible,
            hide_island,
            check_homebrew_update,
            install_homebrew_update,
            webui_current,
            check_webui_update,
            codex_current,
            system_idle_seconds,
        ])
        .run(tauri::generate_context!())
        .expect("启动灵动岛失败");
}
