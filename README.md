# combo-overlay · PhigroscomboDesktop

使用rust编写
模仿不算高仿
## 运行

```powershell
cd combo-overlay
cargo run --release
```

或直接运行编译产物：

```powershell
target\release\combo-overlay.exe
```

程序为单实例：已有一个实例在运行时，再次启动会弹出提示并自动退出（避免重复计数）。

## 热键

| 按键 | 作用 |
|---|---|
| `F9` | 连击清零 |
| `F10` | 退出程序 |
| `Q` / `W` / `E` / `R` | **写谱模式**下切换音效（见下） |

## 写谱模式

适合给音游（phigros 类）写谱时**用键盘听音符音效**。开启后按键音效随音符类型切换：

| 按键 | 音效 |
|---|---|
| `Q` | `tap` |
| `W` | `drag` |
| `E` | `flick` |
| `R` | `tap`（同 Q） |

其他键仍播放默认 `Tap`。开启方式：

- 托盘菜单勾选 **写谱模式（Q/W/E/R 音效）**（可随时切换）
- 或启动时加 `--chart` / 环境变量 `COMBO_OVERLAY_CHART=1`，启动即进入写谱模式

## 系统托盘

程序启动后会在通知区（任务栏右下角）显示一个白色圆点图标，**右键**弹出设置菜单：

(1.png)

菜单弹出期间的方向键/回车不会被计入连击数。

## 游玩检测(依赖ai,写的不可能完全能用)

系统托盘右键菜单新增「游玩检测」开关。开启后自动识别正在游玩的音游
（Phira / osu! / 冰与火之舞 ADOFAI），累计 GOOD / MISS 判定并实时显示在悬浮窗
COMBO 下方；本局结束后展示统计并自动清零。

数据源按精确度分四档，互相独立、同时启用：

1. **osu! 内存读取**（`rosu-memory-lib`）：osu!stable 的游玩状态与判定计数，
   完全自主，无需游戏侧配合。count300 计入 GOOD 之外的 Perfect 档，count100/50
   计入 GOOD，count_miss 计入 MISS。
2. **本地 UDP 判定总线**（`127.0.0.1:45876`）：Phira / 冰与火之舞等游戏侧可通过
   UDP 上报精确判定。协议见下。
3. **画面检测**（`screen.rs`）：真正"看"游戏画面——对前台音游窗口截图做像素分析，
   用**判定文字颜色脉冲**识别判定，用**画面活跃度**区分游玩场景与选歌等静止界面
   （连续活跃约 1 秒判定进入游玩，连续静止约 2 秒结束本局）。配色方案按游戏区分：
   Phira / Phigros 系为 Perfect 金黄、Good 亮蓝、Bad 橙、Miss 灰；冰与火之舞
   （ADOFAI）为 Perfect 绿、EPerfect/LPerfect 黄绿、Early/Late 浅红、
   Too Early/Too Late/Miss 红。面向 Phira / 冰与火之舞，osu! 走内存数据源不
   重复计数。实验性功能：颜色阈值 / 判定区域在 `src/playdetect/screen.rs`
   顶部常量可调。
4. **窗口 / 进程探测**：识别前台是否为目标音游（进程名 + 窗口标题），作为画面
   检测失效时的兜底（前台持续 2 秒判定进入游玩，离开 3 秒结束本局；画面检测已
   判定游玩时不再叠加进入）。

### UDP 协议（每行一个事件，空格分隔）

```text
scene enter <game>           进入游戏场景（开始游玩）
scene exit  <game>           离开游戏场景（本局结束）
judge <game> <kind>          一次判定，kind = perfect | good | bad | miss
combo <game> <n>             当前连击数（可选，占位透传，不计入统计）
reset                        手动清空当前会话统计
```

`<game>` 支持：`phira`、`osu`、`adofai`。

发送示例（Linux / WSL）：

```bash
echo "scene enter phira" > /dev/udp/127.0.0.1/45876
echo "judge phira good"  > /dev/udp/127.0.0.1/45876
echo "scene exit phira"  > /dev/udp/127.0.0.1/45876
```

Python 示例：

```python
import socket
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
s.sendto(b"scene enter phira", ("127.0.0.1", 45876))
s.sendto(b"judge phira miss",  ("127.0.0.1", 45876))
s.sendto(b"scene exit phira",  ("127.0.0.1", 45876))
```

## 参数

- `--tap <路径>`：指定 click.ogg（默认找 exe 旁 `assets/click.ogg`）
- `--drag <路径>`：指定 drag.ogg（默认同上）
- `--flick <路径>`：指定 flick.ogg（默认同上）
- `--font <路径>`：指定字体（默认同上）
- `--chart` 或环境变量 `COMBO_OVERLAY_CHART=1`：启动即进入写谱模式
- `--debug` 或环境变量 `COMBO_OVERLAY_DEBUG=1`：把每次按键（含音效种类）写入 exe 旁的 `combo-overlay.log`

## 说明

- 长按一个键不重复计数（按住只算 1 次），松手后再按才算新的一次；修饰键（Shift/Ctrl 等）同样计数。
- 计数只发生在内存里，不落盘、不上传；日志默认关闭，仅排障时用 `--debug` 打开。
- 音量在 `src/main.rs` 的 `CLICK_VOLUME`（默认 0.9）；数字/标签大小在 `src/combo.rs` 的 `Layout::for_screen_scaled` 里，按屏幕高度等比缩放。
- 若在**独占全屏**游戏里看不到悬浮窗，属系统限制（分层窗口无法盖过独占全屏），窗口化/无边框全屏均可正常显示。
- `src/bin/hooktest.rs` 是钩子调试工具（环境变量 `HOOKTEST_MODE=hide|free` 控制隐藏/释放控制台），不影响主程序构建。
