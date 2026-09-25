//! 系统托盘：通知区图标 + 右键设置菜单。
//!
//! 菜单提供：大小档位（大 15% / 中 9% / 小 6%）、按键音效开关、清零、退出。
//! 菜单命令由 `crate::handle_tray_cmd` 执行；状态读取走 `crate::current_*` 函数。

use std::ffi::c_void;
use std::mem::size_of;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DeleteDC, DeleteObject, DIB_RGB_COLORS,
    GetDC, RGBQUAD,
};
use windows::Win32::UI::Shell::{
    NOTIFYICONDATAW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
    Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, DestroyMenu, GetCursorPos, PostMessageW, SetForegroundWindow,
    TrackPopupMenu, HICON, ICONINFO, MENU_ITEM_FLAGS, MF_CHECKED, MF_SEPARATOR, MF_STRING, TPM_NONOTIFY, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, WM_APP, WM_NULL, WM_RBUTTONUP,
};

/// 托盘回调消息号（WM_APP + 1），发到悬浮窗 wndproc。
pub const WM_TRAYICON: u32 = WM_APP + 1;

// 菜单命令 ID（TrackPopupMenu 的 TPM_RETURNCMD 返回值）
pub const CMD_SIZE_BIG: usize = 1001;
pub const CMD_SIZE_MID: usize = 1002;
pub const CMD_SIZE_SMALL: usize = 1003;
pub const CMD_SOUND: usize = 1004;
pub const CMD_RESET: usize = 1005;
pub const CMD_EXIT: usize = 1006;

/// 托盘：创建时加入通知区，Drop 时移除。
pub struct Tray {
    hwnd: HWND,
}

impl Tray {
    pub fn create(hwnd: HWND) -> Result<Self, String> {
        unsafe {
            let icon = make_icon()?;
            let mut nid = NOTIFYICONDATAW::default();
            nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = hwnd;
            nid.uID = 1;
            nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
            nid.uCallbackMessage = WM_TRAYICON;
            nid.hIcon = icon;
            let tip: Vec<u16> = "combo-overlay".encode_utf16().chain([0]).collect();
            for (i, c) in tip.iter().take(127).enumerate() {
                nid.szTip[i] = *c;
            }
            if !Shell_NotifyIconW(NIM_ADD, &nid).as_bool() {
                return Err("Shell_NotifyIconW(NIM_ADD) 失败".into());
            }
            Ok(Tray { hwnd })
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            let mut nid = NOTIFYICONDATAW::default();
            nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = self.hwnd;
            nid.uID = 1;
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        }
    }
}

/// 处理托盘回调消息：右键按下 → 弹出设置菜单。
pub fn on_tray_message(wparam: WPARAM, lparam: LPARAM) {
    if wparam.0 as u32 == 1 && (lparam.0 as u32 & 0xFFFF) == WM_RBUTTONUP {
        show_menu();
    }
}

fn show_menu() {
    unsafe {
        let menu = match CreatePopupMenu() {
            Ok(m) => m,
            Err(_) => return,
        };
        // 菜单弹出期间屏蔽键盘计数（方向键/回车用于操作菜单）
        crate::set_menu_active(true);
        let scale = crate::current_size_scale();
        let big = (scale - 1.0).abs() < 0.01;
        let mid = (scale - 0.6).abs() < 0.01;
        let small = (scale - 0.4).abs() < 0.01;
        let sound_on = crate::current_sound_on();

        let item = |checked: bool| if checked { MF_CHECKED } else { MENU_ITEM_FLAGS(0) };

        let _ = AppendMenuW(menu, MF_STRING | item(big), CMD_SIZE_BIG, w!("大号（15%）"));
        let _ = AppendMenuW(menu, MF_STRING | item(mid), CMD_SIZE_MID, w!("中号（9%）"));
        let _ = AppendMenuW(menu, MF_STRING | item(small), CMD_SIZE_SMALL, w!("小号（6%）"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING | item(sound_on), CMD_SOUND, w!("按键音效"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, CMD_RESET, w!("清零 (F9)"));
        let _ = AppendMenuW(menu, MF_STRING, CMD_EXIT, w!("退出 (F10)"));

        let hwnd = crate::overlay_hwnd();
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd); // 让菜单点击后能正常关闭
        let cmd = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY,
            pt.x,
            pt.y,
            0,
            hwnd,
            None,
        );
        let _ = DestroyMenu(menu);
        // 惯例：补一个空消息，菜单关闭后窗口重新获得键盘焦点权限
        let _ = PostMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0));
        crate::set_menu_active(false);
        if cmd.0 != 0 {
            crate::handle_tray_cmd(cmd.0 as usize);
        }
    }
}

/// 程序化画一个 32x32 的白色圆点图标（透明背景，带抗锯齿边缘）。
fn make_icon() -> Result<HICON, String> {
    unsafe {
        const S: i32 = 32;
        let dc = GetDC(None);
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER::default(),
            bmiColors: [RGBQUAD::default(); 1],
        };
        bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = S;
        bmi.bmiHeader.biHeight = -S; // 自上而下
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;
        let mut bits: *mut c_void = std::ptr::null_mut();
        let color = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
            .map_err(|e| format!("CreateDIBSection 图标失败: {e}"))?;
        let px = std::slice::from_raw_parts_mut(bits as *mut u8, (S * S * 4) as usize);
        let (cx, cy, r) = (16.0f32, 16.0f32, 11.0f32);
        for y in 0..S {
            for x in 0..S {
                let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
                let i = ((y * S + x) * 4) as usize;
                if d <= r {
                    // 边缘 1.5px 抗锯齿
                    let a = if d > r - 1.5 { 255.0 * (r - d) / 1.5 } else { 255.0 };
                    px[i] = 255;
                    px[i + 1] = 255;
                    px[i + 2] = 255;
                    px[i + 3] = a as u8;
                } else {
                    px[i + 3] = 0;
                }
            }
        }
        let mask = CreateBitmap(S, S, 1, 1, None);
        if mask.is_invalid() {
            return Err("CreateBitmap 失败".into());
        }
        let ii = ICONINFO {
            fIcon: true.into(),
            xHotspot: 8,
            yHotspot: 8,
            hbmMask: mask,
            hbmColor: color,
        };
        let hicon = CreateIconIndirect(&ii).map_err(|e| format!("CreateIconIndirect 失败: {e}"))?;
        let _ = DeleteObject(color);
        let _ = DeleteObject(mask);
        let _ = DeleteDC(dc);
        Ok(hicon)
    }
}
