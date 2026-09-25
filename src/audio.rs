//! click.ogg 音效播放：启动时解码一次，每次按键用 `play_raw` 往
//! OutputStream 内置混音器里投一份采样副本，允许快速连按重叠播放。

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use rodio::buffer::SamplesBuffer;
use rodio::{Decoder, OutputStream, Source};

pub struct ClickSound {
    /// 输出句柄（内置混音器）。原 OutputStream 已 `mem::forget`，进程存活期间常驻。
    handle: rodio::OutputStreamHandle,
    samples: Vec<f32>,
    channels: u16,
    sample_rate: u32,
    volume: f32,
}

impl ClickSound {
    pub fn new(path: &Path, volume: f32) -> Result<Self, String> {
        let file = File::open(path).map_err(|e| format!("打开音效 {path:?} 失败: {e}"))?;
        let decoder = Decoder::new(BufReader::new(file)).map_err(|e| format!("解码 ogg 失败: {e}"))?;
        let sample_rate = decoder.sample_rate();
        let channels = decoder.channels();
        // play_raw 要求 f32 采样
        let samples: Vec<f32> = decoder.map(|s| s as f32 / 32768.0).collect();
        if samples.is_empty() {
            return Err("音效解码结果为空".into());
        }
        let (stream, handle) = OutputStream::try_default()
            .map_err(|e| format!("音频输出设备初始化失败: {e}"))?;
        // 故意泄漏 OutputStream：它一旦 drop 音频设备就会关闭。
        // 本程序生命周期内只有一个实例，泄漏即常驻，量级可忽略。
        std::mem::forget(stream);
        Ok(ClickSound {
            handle,
            samples,
            channels,
            sample_rate,
            volume,
        })
    }

    /// 播放一次点击音（可重叠，播完自动释放）。
    pub fn play(&self) {
        let src =
            SamplesBuffer::new(self.channels, self.sample_rate, self.samples.clone()).amplify(self.volume);
        let _ = self.handle.play_raw(src);
    }
}
