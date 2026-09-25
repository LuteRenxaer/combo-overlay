# combo-overlay · 趣味连击悬浮计数器

把 phirLie（`prpr/src/scene/game.rs`）里的 **COMBO** 分离出来做成的独立小工具：

- 屏幕顶部一个**完全透明、无边框、置顶、点击穿透**的悬浮窗（不抢焦点、不占任务栏）；
- 大号数字 = 你按下过的键盘键数（全局键盘钩子**只计数、不记录按键内容**）；
- 每按一键播放一次 `assets/click.ogg`，数字弹跳一下（1.45 倍 → 回落，0.22s 缓出）；
- 数字与 `COMBO` 标签的排版、白色配色、顶部居中布局，均复刻自 `game.rs` 的 `ui()`（`ComboNumber`/`Combo` 分支），字体使用 `assets/font.ttf`（即游戏里 PGR_FONT 同款）。
- **实现说明**：程序采用**控制台子系统 + 启动即隐藏控制台窗口**。实测本机环境下 `WH_KEYBOARD_LL` 全局钩子在 GUI 子系统（`#![windows_subsystem = "windows"]`）下收不到任何按键事件，改为控制台子系统后稳定工作；控制台在 `main()` 里立刻 `ShowWindow(SW_HIDE)`，对用户不可见。

## 运行

```powershell
cd combo-overlay
cargo run --release
```

或直接运行编译产物：

```powershell
target\release\combo-overlay.exe
```

## 热键

| 按键 | 作用 |
|---|---|
| `F9` | 连击清零 |
| `F10` | 退出程序 |

## 系统托盘

程序启动后会在通知区（任务栏右下角）显示一个白色圆点图标，**右键**弹出设置菜单：

| 菜单项 | 作用 |
|---|---|
| 大号（15%）/ 中号（9%）/ 小号（6%） | 切换悬浮文字大小（当前项打勾） |
| 按键音效 | 开/关每次按键的 click.ogg 播放 |
| 清零 (F9) | 连击清零 |
| 退出 (F10) | 退出程序 |

菜单弹出期间的方向键/回车不会被计入连击数。

## 参数

- `--click <路径>`：指定 click.ogg（默认找 exe 旁 `assets/click.ogg`）
- `--font <路径>`：指定字体（默认同上）
- `--debug` 或环境变量 `COMBO_OVERLAY_DEBUG=1`：把每次按键写入 exe 旁的 `combo-overlay.log`

## 说明

- 长按一个键不重复计数（按住只算 1 次），松手后再按才算新的一次；修饰键（Shift/Ctrl 等）同样计数。
- 计数只发生在内存里，不落盘、不上传；日志默认关闭，仅排障时用 `--debug` 打开。
- 音量在 `src/main.rs` 的 `CLICK_VOLUME`（默认 0.9）；数字/标签大小在 `src/combo.rs` 的 `Layout::for_screen_scaled` 里，按屏幕高度等比缩放。
- 若在**独占全屏**游戏里看不到悬浮窗，属系统限制（分层窗口无法盖过独占全屏），窗口化/无边框全屏均可正常显示。
- `src/bin/hooktest.rs` 是钩子调试工具（环境变量 `HOOKTEST_MODE=hide|free` 控制隐藏/释放控制台），不影响主程序构建。
