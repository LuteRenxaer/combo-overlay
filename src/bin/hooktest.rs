//! 最小化钩子测试：只安装 WH_KEYBOARD_LL + 消息循环，事件写入 hooktest.log。
//! 模式（环境变量 HOOKTEST_MODE）：free=FreeConsole；hide=ShowWindow(SW_HIDE) 隐藏控制台。

use std::fs::OpenOptions;
use std::io::Write;

use windows::Win32::System::Console::{FreeConsole, GetConsoleWindow};
use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SHOW_WINDOW_CMD};

use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, KBDLLHOOKSTRUCT, MSG, SetWindowsHookExW, TranslateMessage,
    WH_KEYBOARD_LL,
};

fn log(msg: &str) {
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(r"F:\phirLie\combo-overlay\hooktest.log")
    {
        let _ = writeln!(f, "{msg}");
    }
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let kb = unsafe { *(lparam.0 as *const KBDLLHOOKSTRUCT) };
        log(&format!("EVENT code={code} msg=0x{:X} vk={}", wparam.0, kb.vkCode));
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn main() {
    unsafe {
        match std::env::var("HOOKTEST_MODE").as_deref() {
            Ok("free") => {
                let _ = FreeConsole();
                log("mode=FreeConsole");
            }
            Ok("hide") => {
                let _ = ShowWindow(GetConsoleWindow(), SHOW_WINDOW_CMD(0)); // SW_HIDE
                log("mode=hide-console");
            }
            _ => log("mode=none"),
        }
        let hmod: HINSTANCE = match std::env::var("HOOKTEST_HMOD_NULL") {
            Ok(_) => {
                log("hmod=NULL");
                HINSTANCE::default()
            }
            Err(_) => {
                log("hmod=exe");
                GetModuleHandleW(None).unwrap_or_default().into()
            }
        };
        match SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), hmod, 0) {
            Ok(h) => log(&format!("installed {h:?}")),
            Err(e) => {
                log(&format!("install err {e}"));
                return;
            }
        }
        log("pumping");
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            let _ = DispatchMessageW(&msg);
        }
    }
}
