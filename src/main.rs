//! combo-overlay —— 趣味连击悬浮计数器
//!
//! 从 phirLie（`prpr/src/scene/game.rs`）中把 COMBO 分离出来做成独立小工具：
//! - 屏幕顶部一个**完全透明、无边框、置顶、点击穿透**的悬浮窗；
//! - 数字 = 你按下过的键盘键数（全局钩子只计数）；
//! - 每按一键播放一次 `click.ogg`，数字弹跳一下；
//! - 热键：`F9` 清零，`F10` 退出；
//! - 托盘菜单：大小档位 / 按键音效 / 写谱模式 / 清零 / 退出。

//! 注意：控制台子系统 + 启动即隐藏控制台窗口（本机 GUI 子系统收不到 WH_KEYBOARD_LL 事件）。

mod audio;
mod combo;
mod hook;
mod overlay;
mod tray;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use ab_glyph::FontArc;
use windows::Win32::Foundation::{GetLastError, HWND, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Console::GetConsoleWindow;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, GetSystemMetrics, MessageBoxW, PostQuitMessage, ShowWindow, TranslateMessage,
    SHOW_WINDOW_CMD, SM_CXSCREEN, SM_CYSCREEN, SetProcessDPIAware, MB_ICONINFORMATION, MB_OK, MSG,
};

use crate::audio::{SoundKind, SoundSet};
use crate::combo::{Combo, Layout};
use crate::hook::KeyHook;
use crate::overlay::Overlay;

// ---------- 热键 ----------
/// 清零
const VK_F9: u32 = 0x78;
/// 退出
const VK_F10: u32 = 0x79;
/// 写谱模式音效键：Q / R → click，W → drag，E → flick
const VK_Q: u32 = 0x51;
const VK_W: u32 = 0x57;
const VK_E: u32 = 0x45;
const VK_R: u32 = 0x52;
/// 点击音量（0.0 ~ 1.0）
const CLICK_VOLUME: f32 = 0.9;
/// 默认大小档位（中号，数字约为屏高 9%）
const DEFAULT_SCALE: f32 = 0.6;

// ---------- 全局状态 ----------
// 键盘钩子回调与 WM_TIMER 渲染都运行在主线程；Mutex 只作为 static 的载体。
struct AppState {
    combo: Combo,
    /// 当前按住中的键（用于把“长按自动重复”排除在计数外，只算真实按下次数）
    pressed: HashSet<u32>,
    audio: Option<SoundSet>,
    overlay: Overlay,
    font: FontArc,
    layout: Layout,
    /// 屏幕高度（重建布局用）
    sh: i32,
    /// 当前大小档位（1.0 / 0.6 / 0.4）
    size_scale: f32,
    /// 按键音效开关
    sound_on: bool,
    /// 写谱模式：Q/R→click、W→drag、E→flick
    chart_mode: bool,
    /// 需要重绘（按键/清零后置位；动画期间持续重绘）
    dirty: bool,
    /// 静止帧计数（调试心跳用）
    heartbeat: u64,
    debug: bool,
}

static STATE: Mutex<Option<AppState>> = Mutex::new(None);

/// 托盘菜单弹出期间置位：操作菜单的方向键/回车不应计入连击。
static MENU_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn set_menu_active(v: bool) {
    MENU_ACTIVE.store(v, Ordering::Relaxed);
}

fn menu_active() -> bool {
    MENU_ACTIVE.load(Ordering::Relaxed)
}

// ---------- 日志 ----------
fn log_line(msg: &str) {
    let mut p = std::env::current_exe().unwrap_or_default();
    p.set_file_name("combo-overlay.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        use std::io::Write;
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| format!("{}.{:03}", d.as_secs(), d.subsec_millis()))
            .unwrap_or_else(|_| "?".into());
        let _ = writeln!(f, "[{ts}] {msg}");
    }
}

fn fatal(msg: &str) -> ! {
    log_line(&format!("FATAL: {msg}"));
    unsafe {
        let text: Vec<u16> = msg.encode_utf16().chain([0]).collect();
        let _ = MessageBoxW(None, windows::core::PCWSTR(text.as_ptr()), windows::core::w!("combo-overlay"), MB_OK);
    }
    std::process::exit(1);
}

// ---------- 素材路径解析 ----------
fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn resolve_asset(args: &[String], name: &str, arg_name: &str) -> Option<PathBuf> {
    if let Some(p) = arg_value(args, arg_name) {
        return Some(PathBuf::from(p));
    }
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    for base in [exe_dir.join("assets"), exe_dir] {
        let p = base.join(name);
        if p.exists() {
            return Some(p);
        }
    }
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        let p = PathBuf::from(manifest).join("assets").join(name);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// 按键对应的音效：写谱模式下 Q/R→click、W→drag、E→flick，其余键默认 click。
fn sound_for_key(chart_mode: bool, vk: u32) -> SoundKind {
    if !chart_mode {
        return SoundKind::Click;
    }
    match vk {
        VK_Q | VK_R => SoundKind::Click,
        VK_W => SoundKind::Drag,
        VK_E => SoundKind::Flick,
        _ => SoundKind::Click,
    }
}

// ---------- 按键事件（由键盘钩子回调触发） ----------
fn handle_key(vk: u32, down: bool) {
    let mut g = STATE.lock().unwrap();
    let Some(s) = g.as_mut() else { return };

    if down {
        // 托盘菜单交互期间：方向键/回车等只用于操作菜单，不计入连击
        if menu_active() {
            return;
        }
        // 热键：F9 清零 / F10 退出（它们本身不计数、不播声）
        if vk == VK_F9 {
            s.combo.reset();
            s.dirty = true;
            if s.debug {
                log_line("RESET -> 0");
            }
            return;
        }
        if vk == VK_F10 {
            unsafe {
                PostQuitMessage(0);
            }
            return;
        }
        // 只统计“新按下”（按住自动重复不算），按下过的键总数 +1
        if s.pressed.insert(vk) {
            s.combo.register_press();
            s.dirty = true;
            let kind = sound_for_key(s.chart_mode, vk);
            if s.sound_on {
                if let Some(a) = &s.audio {
                    a.play(kind);
                }
            }
            if s.debug {
                log_line(&format!("KEYDOWN vk={vk} kind={kind:?} count={}", s.combo.value));
            }
        }
    } else {
        s.pressed.remove(&vk);
    }
}

// ---------- 渲染（WM_TIMER） ----------
fn on_timer() {
    let mut g = STATE.lock().unwrap();
    let Some(s) = g.as_mut() else { return };
    let now = Instant::now();
    let animating = s.combo.pop_scale(now) > 1.001;
    if !s.dirty && !animating {
        // 心跳：每 125 个静止帧（约 2 秒）记一次，证明消息循环存活
        s.heartbeat += 1;
        if s.debug && s.heartbeat % 125 == 0 {
            log_line("heartbeat");
        }
        return; // 完全静止：不重复提交
    }
    s.dirty = false;

    let w = s.overlay.width as u32;
    let h = s.overlay.height as u32;
    let mut buf = vec![0u8; (w * h * 4) as usize];
    combo::render(&mut buf, w, h, &s.font, &s.combo, now, &s.layout);
    s.overlay.present(&buf);
}

fn on_display_change() {
    let sw = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let sh = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    let need = {
        let mut g = STATE.lock().unwrap();
        if let Some(s) = g.as_mut() {
            s.sh = sh;
            s.layout = Layout::for_screen_scaled(sh, s.size_scale);
            let hh = s.layout.window_height();
            if let Err(e) = s.overlay.resize(sw, hh) {
                log_line(&format!("resize failed: {e}"));
            }
            s.dirty = true;
            true
        } else {
            false
        }
    };
    if need {
        on_timer();
    }
}

// ---------- 托盘接口（由 tray.rs 调用） ----------

/// 悬浮窗句柄（菜单 owner 用）。
pub fn overlay_hwnd() -> HWND {
    STATE.lock().unwrap().as_ref().map(|s| s.overlay.hwnd).unwrap_or_default()
}

/// 当前大小档位。
pub fn current_size_scale() -> f32 {
    STATE.lock().unwrap().as_ref().map(|s| s.size_scale).unwrap_or(DEFAULT_SCALE)
}

/// 当前音效开关。
pub fn current_sound_on() -> bool {
    STATE.lock().unwrap().as_ref().map(|s| s.sound_on).unwrap_or(true)
}

/// 当前写谱模式开关。
pub fn current_chart_mode() -> bool {
    STATE.lock().unwrap().as_ref().map(|s| s.chart_mode).unwrap_or(false)
}

/// 托盘菜单命令执行。
pub fn handle_tray_cmd(cmd: usize) {
    match cmd {
        crate::tray::CMD_SIZE_BIG => apply_size(1.0),
        crate::tray::CMD_SIZE_MID => apply_size(0.6),
        crate::tray::CMD_SIZE_SMALL => apply_size(0.4),
        crate::tray::CMD_SOUND => {
            let mut g = STATE.lock().unwrap();
            if let Some(s) = g.as_mut() {
                s.sound_on = !s.sound_on;
            }
            if debug_enabled() {
                log_line("sound toggled");
            }
        }
        crate::tray::CMD_CHART_MODE => {
            let mut g = STATE.lock().unwrap();
            if let Some(s) = g.as_mut() {
                s.chart_mode = !s.chart_mode;
            }
            if debug_enabled() {
                log_line("chart mode toggled");
            }
        }
        crate::tray::CMD_RESET => {
            let mut g = STATE.lock().unwrap();
            if let Some(s) = g.as_mut() {
                s.combo.reset();
                s.dirty = true;
            }
        }
        crate::tray::CMD_EXIT => unsafe {
            PostQuitMessage(0);
        },
        _ => {}
    }
}

/// 切换大小档位：重建布局 → 调整窗口 → 立即重绘。
fn apply_size(scale: f32) {
    let need = {
        let mut g = STATE.lock().unwrap();
        if let Some(s) = g.as_mut() {
            s.size_scale = scale;
            s.layout = Layout::for_screen_scaled(s.sh, scale);
            let hh = s.layout.window_height();
            if let Err(e) = s.overlay.resize(s.overlay.width, hh) {
                log_line(&format!("resize 失败: {e}"));
            }
            s.dirty = true;
            true
        } else {
            false
        }
    };
    if need {
        on_timer();
    }
    log_line(&format!("size -> {scale}"));
}

fn debug_enabled() -> bool {
    STATE.lock().unwrap().as_ref().map(|s| s.debug).unwrap_or(false)
}

// ---------- 入口 ----------
fn main() {
    // panic 也写入日志，便于定位崩溃点
    std::panic::set_hook(Box::new(|info| {
        log_line(&format!("PANIC: {info}"));
    }));
    // 隐藏控制台窗口（保留控制台挂载，LL 钩子才能收到事件）
    unsafe {
        let _ = ShowWindow(GetConsoleWindow(), SHOW_WINDOW_CMD(0)); // SW_HIDE
    }
    let args: Vec<String> = std::env::args().collect();
    let debug = args.iter().any(|a| a == "--debug")
        || std::env::var("COMBO_OVERLAY_DEBUG").map(|v| v == "1").unwrap_or(false);
    let chart_start = args.iter().any(|a| a == "--chart")
        || std::env::var("COMBO_OVERLAY_CHART").map(|v| v == "1").unwrap_or(false);

    // 单实例：已有实例在跑则提示并退出（避免重复计数/音效叠加，也避免“双击无反应”的困惑）
    unsafe {
        let mutex = CreateMutexW(None, true, windows::core::w!("Local\\combo-overlay-singleton"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = MessageBoxW(
                None,
                windows::core::w!("combo-overlay 已在运行（右下角托盘有白色圆点图标）。"),
                windows::core::w!("combo-overlay"),
                MB_OK | MB_ICONINFORMATION,
            );
            return;
        }
        let _ = mutex; // 保持句柄存活（进程结束自动释放）
    }

    log_line(&format!("starting, debug={debug}, chart={chart_start}"));

    unsafe {
        let _ = SetProcessDPIAware(); // 使用物理像素，避免高 DPI 缩放错位
    }

    // 字体：必需
    let font_path = resolve_asset(&args, "font.ttf", "--font")
        .unwrap_or_else(|| fatal("找不到字体 font.ttf，请放在 exe 旁的 assets/ 目录或用 --font <路径> 指定"));
    let font_bytes = std::fs::read(&font_path).unwrap_or_else(|_| fatal(&format!("读取字体失败: {}", font_path.display())));
    let font = FontArc::try_from_vec(font_bytes).unwrap_or_else(|e| fatal(&format!("字体解析失败: {e}")));

    // 音效：click / drag / flick（失败则静默降级为“只计数不发声”）
    let click_path = resolve_asset(&args, "click.ogg", "--click");
    let drag_path = resolve_asset(&args, "drag.ogg", "--drag");
    let flick_path = resolve_asset(&args, "flick.ogg", "--flick");
    let audio = match (click_path, drag_path, flick_path) {
        (Some(c), Some(d), Some(f)) => match SoundSet::load(&c, &d, &f, CLICK_VOLUME) {
            Ok(a) => Some(a),
            Err(e) => {
                log_line(&format!("audio disabled: {e}"));
                None
            }
        },
        _ => {
            log_line("音效文件缺失，仅计数不发声（--click/--drag/--flick 可指定路径）");
            None
        }
    };

    let sw = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let sh = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    let layout = Layout::for_screen_scaled(sh, DEFAULT_SCALE);
    let win_h = layout.window_height();
    log_line(&format!("screen={sw}x{sh} overlay_height={win_h}"));

    let overlay = Overlay::create(sw, win_h).unwrap_or_else(|e| fatal(&format!("创建悬浮窗失败: {e}")));
    overlay.show();
    overlay.start_timer();

    let hook = KeyHook::install().unwrap_or_else(|e| fatal(&format!("安装键盘钩子失败: {e}")));

    *STATE.lock().unwrap() = Some(AppState {
        combo: Combo::new(),
        pressed: HashSet::new(),
        audio,
        overlay,
        font,
        layout,
        sh,
        size_scale: DEFAULT_SCALE,
        sound_on: true,
        chart_mode: chart_start,
        dirty: true,
        heartbeat: 0,
        debug,
    });

    // 托盘图标（失败不致命：仅提示日志，悬浮窗照常工作）
    let tray = match crate::tray::Tray::create(overlay_hwnd()) {
        Ok(t) => Some(t),
        Err(e) => {
            log_line(&format!("托盘创建失败: {e}"));
            None
        }
    };

    // 立即画第一帧
    on_timer();
    log_line("ready. F9 = 清零, F10 = 退出");

    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            let _ = DispatchMessageW(&msg);
        }
    }

    drop(hook);
    drop(tray);
    log_line("exited");
}
