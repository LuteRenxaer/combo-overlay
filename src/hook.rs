//! 全局低级键盘钩子（WH_KEYBOARD_LL）：只上报“哪个键按下/抬起”，
//! 由 `crate::handle_key` 决定计数。**不记录按键内容，无隐私问题。**

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, HHOOK, KBDLLHOOKSTRUCT, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

pub struct KeyHook(HHOOK);

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let msg = wparam.0 as u32;
        match msg {
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let kb = unsafe { *(lparam.0 as *const KBDLLHOOKSTRUCT) };
                crate::handle_key(kb.vkCode, true);
            }
            WM_KEYUP | WM_SYSKEYUP => {
                let kb = unsafe { *(lparam.0 as *const KBDLLHOOKSTRUCT) };
                crate::handle_key(kb.vkCode, false);
            }
            _ => {}
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

impl KeyHook {
    /// 安装全局钩子。钩子回调运行在安装它的线程上（主线程的消息循环内）。
    pub fn install() -> Result<Self, String> {
        unsafe {
            let hook = SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(hook_proc),
                GetModuleHandleW(None).unwrap_or_default(),
                0,
            )
            .map_err(|e| format!("SetWindowsHookExW 失败: {e}"))?;
            Ok(KeyHook(hook))
        }
    }
}

impl Drop for KeyHook {
    fn drop(&mut self) {
        unsafe {
            let _ = UnhookWindowsHookEx(self.0);
        }
    }
}
