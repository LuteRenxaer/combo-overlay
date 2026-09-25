//! # COMBO（连击）模块
//!
//! 本模块从 phirLie 的 `prpr/src/scene/game.rs` 中分离出来，只保留“连击”这一件事：
//! 数值状态 + 弹跳动画 + 屏幕顶部居中绘制。
//!
//! ## 原实现出处（便于对照）
//! - **数值语义**：`prpr/src/judge.rs`（`fn judge` 中 199~209 行）
//!   `Perfect | Good => self.combo += 1`，其余判定 `self.combo = 0`；`combo()` 返回当前连击。
//! - **渲染**：`game.rs` `ui()` 中 `UIElement::ComboNumber / Combo` 分支（约 675~759 行）
//!   - 数字：`size(1.0)`、水平居中、锚点 `(0.5, 0.5)`、颜色取 chart 元素色（默认白色），字体 `PGR_FONT`；
//!   - 标签：`size(0.4)`、位于数字下方、文字 `"COMBO"`；
//!   - 整体随 `combo_offset_x / combo_offset_y` 偏移，默认水平居中、贴近屏幕顶部。
//!
//! ## 本工具的语义（按你的需求重新定义）
//! - **每次按下键盘任意键** → `register_press()`，连击数 +1；
//! - 显示的数字 = 用户总共按下过的键数（全局键盘钩子只计数、不记录具体按键，无隐私问题）；
//! - 每次按键触发一次“弹跳”动画（1.45 倍 → 1.0，约 0.22 秒缓出）并伴随轻微上浮。

use std::time::Instant;

use ab_glyph::{point, Font, FontArc, Glyph, PxScale, ScaleFont};

/// 屏幕顶部连击文字的排版参数（相对屏幕高度等比缩放，模仿游戏中 size 1.0 / 0.4 的大小关系）。
pub struct Layout {
    pub margin_top_px: f32,
    pub number_px: f32,
    pub gap_px: f32,
    pub label_px: f32,
    pub margin_bottom_px: f32,
}

impl Layout {
    /// 按屏幕高度与整体缩放系数生成排版（scale=1.0 即原版大号）。
    /// 托盘菜单三档：大 1.0（15%）/ 中 0.6（9%）/ 小 0.4（6%）。
    pub fn for_screen_scaled(screen_height: i32, scale: f32) -> Self {
        let sh = screen_height as f32;
        Layout {
            margin_top_px: sh * 0.10 * scale,
            number_px: sh * 0.15 * scale,
            gap_px: sh * 0.03 * scale,
            label_px: sh * 0.055 * scale,
            margin_bottom_px: sh * 0.04 * scale,
        }
    }

    pub fn window_height(&self) -> i32 {
        (self.margin_top_px + self.number_px + self.gap_px + self.label_px + self.margin_bottom_px).round() as i32
    }

    pub fn number_center_y(&self) -> f32 {
        self.margin_top_px + self.number_px * 0.5
    }

    pub fn label_center_y(&self) -> f32 {
        self.margin_top_px + self.number_px + self.gap_px + self.label_px * 0.5
    }
}

/// 连击状态机：对应游戏里的 `judge.combo()`，此处语义改为“每按一键 +1”。
pub struct Combo {
    pub value: u64,
    /// 最近一次 +1（或重置）的时刻，用于弹跳动画。
    pop_start: Instant,
}

impl Default for Combo {
    fn default() -> Self {
        Self::new()
    }
}

impl Combo {
    pub fn new() -> Self {
        Combo {
            value: 0,
            pop_start: Instant::now(),
        }
    }

    /// 按下一键：连击 +1（对应游戏中 `Perfect | Good => combo += 1`）。
    pub fn register_press(&mut self) {
        self.value = self.value.saturating_add(1);
        self.pop_start = Instant::now();
    }

    /// 清零（热键 F9），对应游戏中的 `reset()`。
    pub fn reset(&mut self) {
        self.value = 0;
        self.pop_start = Instant::now();
    }

    /// 弹跳缩放系数：按下瞬间 1.45，0.22 秒内缓出回落到 1.0。
    pub fn pop_scale(&self, now: Instant) -> f32 {
        const T: f32 = 0.22;
        let t = now.duration_since(self.pop_start).as_secs_f32();
        if t >= T {
            1.0
        } else {
            1.0 + 0.45 * (1.0 - t / T) * (1.0 - t / T)
        }
    }

    /// 弹跳伴随的轻微上浮（像素）。
    pub fn pop_lift(&self, now: Instant) -> f32 {
        const T: f32 = 0.22;
        let t = now.duration_since(self.pop_start).as_secs_f32();
        if t >= T {
            0.0
        } else {
            8.0 * (1.0 - t / T) * (1.0 - t / T)
        }
    }
}

/// 在当前帧把连击绘制进 RGBA（直通 alpha）缓冲。
///
/// 布局复刻 game.rs：大号数字在上、小号 "COMBO" 标签在下，水平居中，白色 + 轻投影。
pub fn render(
    buf: &mut [u8],
    w: u32,
    h: u32,
    font: &FontArc,
    combo: &Combo,
    now: Instant,
    layout: &Layout,
) {
    let cx = w as f32 / 2.0;
    let pop = combo.pop_scale(now);
    let lift = combo.pop_lift(now);

    let num_size = layout.number_px * pop;
    let num_cy = layout.number_center_y() - lift;
    let label_cy = layout.label_center_y();
    let text = combo.value.to_string();

    // 数字：黑色投影 + 白色主体（游戏中数字为白色，随元素 alpha）
    draw_text_centered(buf, w, h, font, &text, num_size, [0.0, 0.0, 0.0, 0.30], cx + 2.0, num_cy + 3.0);
    draw_text_centered(buf, w, h, font, &text, num_size, [1.0, 1.0, 1.0, 0.96], cx, num_cy);

    // 标签 "COMBO"（游戏中标签 size 0.4，位于数字正下方）
    draw_text_centered(buf, w, h, font, "COMBO", layout.label_px, [0.0, 0.0, 0.0, 0.25], cx + 1.0, label_cy + 2.0);
    draw_text_centered(buf, w, h, font, "COMBO", layout.label_px, [1.0, 1.0, 1.0, 0.75], cx, label_cy);
}

/// 用 ab_glyph 在 RGBA 缓冲上绘制一段水平、垂直居中的文本（直通 alpha 混合）。
/// `color` 为 `[r, g, b, a]`，各分量 0..=1。
fn draw_text_centered(
    buf: &mut [u8],
    w: u32,
    h: u32,
    font: &FontArc,
    text: &str,
    size_px: f32,
    color: [f32; 4],
    cx: f32,
    cy: f32,
) {
    if size_px <= 0.0 || text.is_empty() {
        return;
    }
    let scale = PxScale::from(size_px);
    let sf = font.as_scaled(scale);

    // 手动排版（ab_glyph 0.2 不提供 layout）：按水平 advance + kerning 推进笔位
    let mut glyphs: Vec<Glyph> = Vec::with_capacity(text.chars().count());
    let mut pen_x = 0.0f32;
    for c in text.chars() {
        let mut g = sf.scaled_glyph(c);
        if let Some(prev) = glyphs.last() {
            pen_x += sf.kern(prev.id, g.id);
        }
        g.position = point(pen_x, 0.0);
        pen_x += sf.h_advance(g.id);
        glyphs.push(g);
    }
    let width = pen_x;
    // 基线置于垂直中心：(ascent + descent) / 2 是中心到基线的偏移（descent 为负）
    let baseline = cy + (sf.ascent() + sf.descent()) / 2.0;
    let offset_x = cx - width / 2.0;

    for mut g in glyphs {
        g.position.x += offset_x;
        g.position.y += baseline;
        if let Some(og) = sf.outline_glyph(g) {
            let b = og.px_bounds();
            og.draw(|dx, dy, cov| {
                let px = b.min.x as i32 + dx as i32;
                let py = b.min.y as i32 + dy as i32;
                if px >= 0 && py >= 0 && px < w as i32 && py < h as i32 {
                    let i = ((py as usize) * (w as usize) + px as usize) * 4;
                    let a = (color[3] * cov).clamp(0.0, 1.0);
                    buf[i] = (color[0] * 255.0 * a + buf[i] as f32 * (1.0 - a)) as u8;
                    buf[i + 1] = (color[1] * 255.0 * a + buf[i + 1] as f32 * (1.0 - a)) as u8;
                    buf[i + 2] = (color[2] * 255.0 * a + buf[i + 2] as f32 * (1.0 - a)) as u8;
                    buf[i + 3] = (255.0 * a + buf[i + 3] as f32 * (1.0 - a)) as u8;
                }
            });
        }
    }
}
