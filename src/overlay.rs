//! 透明置顶悬浮窗：无边框、无任务栏条目、全透明背景、点击穿透，
//! 通过 `UpdateLayeredWindow` 做逐像素 alpha 合成，只把连击文字画到屏幕上。

use std::ffi::c_void;
use std::mem::size_of;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC,
    CreateDIBSection, DeleteDC, DeleteObject, DIB_RGB_COLORS, GetDC, HBRUSH, RGBQUAD, SelectObject, HBITMAP, HDC,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, HICON, HCURSOR, PostQuitMessage, RegisterClassW, SetTimer,
    SetWindowPos, ShowWindow, CS_HREDRAW, CS_VREDRAW, HTTRANSPARENT, SWP_NOACTIVATE,
    SWP_NOZORDER, SW_SHOWNOACTIVATE, ULW_ALPHA, UpdateLayeredWindow, WM_DESTROY, WM_DISPLAYCHANGE, WM_NCHITTEST,
    WM_TIMER, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP, WNDCLASSW,
};

/// 悬浮窗：窗口 + 与窗口等大的 32 位 DIB（BGRA 预乘）后备缓冲。
pub struct Overlay {
    pub hwnd: HWND,
    pub width: i32,
    pub height: i32,
    screen_dc: HDC,
    mem_dc: HDC,
    bitmap: HBITMAP,
    bits: *mut u8,
}

// 仅主线程使用（static 里的 Mutex 只要求 Send）。
unsafe impl Send for Overlay {}

const TIMER_ID: usize = 1;

impl Overlay {
    /// 在屏幕左上角创建一个 `width x height` 的透明置顶悬浮窗。
    pub fn create(width: i32, height: i32) -> Result<Self, String> {
        unsafe {
            let hinst = GetModuleHandleW(None).map_err(|e| e.to_string())?;
            let class = w!("ComboOverlayClass");
            let wc = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(wndproc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinst.into(),
                hIcon: HICON::default(),
                hCursor: HCURSOR::default(),
                hbrBackground: HBRUSH::default(),
                lpszMenuName: PCWSTR::null(),
                lpszClassName: class,
            };
            // 已注册过则 RegisterClassW 返回 0，可忽略
            RegisterClassW(&wc);

            // WS_EX_LAYERED: 分层窗口；WS_EX_TRANSPARENT + WM_NCHITTEST: 点击穿透；
            // WS_EX_TOPMOST: 置顶；WS_EX_NOACTIVATE: 永不抢焦点；WS_EX_TOOLWINDOW: 不占任务栏/Alt+Tab。
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                class,
                w!("Combo Overlay"),
                WS_POPUP,
                0,
                0,
                width,
                height,
                None,
                None,
                hinst,
                None,
            )
            .map_err(|e| format!("CreateWindowExW 失败: {e}"))?;

            let screen_dc = GetDC(None);
            let mem_dc = CreateCompatibleDC(screen_dc);
            let (bitmap, bits) = Self::create_dib(screen_dc, width, height)?;
            let _ = SelectObject(mem_dc, bitmap);

            Ok(Overlay {
                hwnd,
                width,
                height,
                screen_dc,
                mem_dc,
                bitmap,
                bits,
            })
        }
    }

    fn create_dib(dc: HDC, w: i32, h: i32) -> Result<(HBITMAP, *mut u8), String> {
        unsafe {
            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER::default(),
                bmiColors: [RGBQUAD::default(); 1],
            };
            bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth = w;
            bmi.bmiHeader.biHeight = -h; // 负值 = 自上而下
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB.0;
            let mut bits: *mut c_void = std::ptr::null_mut();
            let bmp = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
                .map_err(|e| format!("CreateDIBSection 失败: {e}"))?;
            Ok((bmp, bits as *mut u8))
        }
    }

    /// 把 RGBA（直通 alpha）缓冲转成预乘 BGRA 并提交到屏幕。
    pub fn present(&mut self, rgba: &[u8]) {
        let n = (self.width * self.height) as usize;
        let buf = unsafe { std::slice::from_raw_parts_mut(self.bits, n * 4) };
        for (i, px) in rgba.chunks_exact(4).take(n).enumerate() {
            let (r, g, b, a) = (px[0] as u32, px[1] as u32, px[2] as u32, px[3] as u32);
            let o = i * 4;
            buf[o] = (b * a / 255) as u8;
            buf[o + 1] = (g * a / 255) as u8;
            buf[o + 2] = (r * a / 255) as u8;
            buf[o + 3] = a as u8;
        }
        unsafe {
            let size = SIZE {
                cx: self.width,
                cy: self.height,
            };
            let dst = POINT { x: 0, y: 0 };
            let src = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let _ = UpdateLayeredWindow(
                self.hwnd,
                self.screen_dc,
                Some(&dst),
                Some(&size),
                self.mem_dc,
                Some(&src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
    }

    pub fn show(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
    }

    pub fn start_timer(&self) {
        unsafe {
            SetTimer(self.hwnd, TIMER_ID, 16, None);
        }
    }

    /// 运行时调整窗口与后备缓冲尺寸（托盘菜单切换大小档位时调用）。
    pub fn resize(&mut self, w: i32, h: i32) -> Result<(), String> {
        unsafe {
            let _ = DeleteObject(self.bitmap);
            let _ = DeleteDC(self.mem_dc);
            let (bitmap, bits) = Self::create_dib(self.screen_dc, w, h)?;
            let mem_dc = CreateCompatibleDC(self.screen_dc);
            let _ = SelectObject(mem_dc, bitmap);
            self.width = w;
            self.height = h;
            self.bitmap = bitmap;
            self.mem_dc = mem_dc;
            self.bits = bits;
            let _ = SetWindowPos(self.hwnd, None, 0, 0, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
            Ok(())
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            // 让鼠标点击直接穿透到下层窗口
            WM_NCHITTEST => return LRESULT(HTTRANSPARENT as isize),
            WM_TIMER => crate::on_timer(),
            WM_DISPLAYCHANGE => crate::on_display_change(),
            crate::tray::WM_TRAYICON => crate::tray::on_tray_message(wparam, lparam),
            WM_DESTROY => {
                PostQuitMessage(0);
                return LRESULT(0);
            }
            _ => {}
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}
