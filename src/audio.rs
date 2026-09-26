//! 音效播放：click / drag / flick 三个 ogg 启动时各解码一次，共用一个输出设备，
//! 每次按键用 `play_raw` 往内置混音器投一份采样副本，允许快速连按重叠播放。
//!
//! 单个音效缺失/解码失败时降级为用 click 的采样代替（日志记录）；全部失败则整体禁用。

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use rodio::buffer::SamplesBuffer;
use rodio::{Decoder, OutputStream, Source};

/// 音效种类（对应写谱模式下的 Q / W / E / R）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SoundKind {
    Click,
    Drag,
    Flick,
}

pub struct SoundSet {
    /// 输出句柄（内置混音器）。原 OutputStream 已 `mem::forget`，进程存活期间常驻。
    handle: rodio::OutputStreamHandle,
    /// 三组采样：[click, drag, flick]
    sounds: [Vec<f32>; 3],
    channels: u16,
    sample_rate: u32,
    volume: f32,
}

fn decode(path: &Path) -> Option<(Vec<f32>, u16, u32)> {
    let file = File::open(path).ok()?;
    let decoder = Decoder::new(BufReader::new(file)).ok()?;
    let sample_rate = decoder.sample_rate();
    let channels = decoder.channels();
    let samples: Vec<f32> = decoder.map(|s| s as f32 / 32768.0).collect();
    if samples.is_empty() {
        None
    } else {
        Some((samples, channels, sample_rate))
    }
}

impl SoundSet {
    /// 加载三组音效。缺失/解码失败的项用 click 的采样兜底。
    pub fn load(click: &Path, drag: &Path, flick: &Path, volume: f32) -> Result<Self, String> {
        let c = decode(click).ok_or_else(|| format!("click.ogg 解码失败: {click:?}"))?;
        let (cs, ch, sr) = (c.0, c.1, c.2);

        let fallback = |name: &str, p: &Path| -> Vec<f32> {
            match decode(p) {
                Some((s, _, _)) => s,
                None => {
                    crate::log_line(&format!("{name} 缺失/解码失败，降级用 click.ogg 音效: {p:?}"));
                    cs.clone()
                }
            }
        };
        let ds = fallback("drag.ogg", drag);
        let fs = fallback("flick.ogg", flick);

        let (stream, handle) = OutputStream::try_default()
            .map_err(|e| format!("音频输出设备初始化失败: {e}"))?;
        // 故意泄漏 OutputStream：它一旦 drop 音频设备就会关闭。
        // 本程序生命周期内只有一个实例，泄漏即常驻，量级可忽略。
        std::mem::forget(stream);

        Ok(SoundSet {
            handle,
            sounds: [cs, ds, fs],
            channels: ch,
            sample_rate: sr,
            volume,
        })
    }

    /// 播放一种音效（可重叠，播完自动释放）。
    pub fn play(&self, kind: SoundKind) {
        let idx = match kind {
            SoundKind::Click => 0,
            SoundKind::Drag => 1,
            SoundKind::Flick => 2,
        };
        let src = SamplesBuffer::new(self.channels, self.sample_rate, self.sounds[idx].clone())
            .amplify(self.volume);
        let _ = self.handle.play_raw(src);
    }
}
