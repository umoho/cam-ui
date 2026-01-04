mod file;
mod icons;
mod ui;
mod video;

use eframe::egui;
use parking_lot::Mutex;
use std::sync::Arc;
use tokio::sync::mpsc;

fn main() -> eframe::Result {
    // 1. 初始化 GStreamer
    gstreamer::init().expect("GStreamer init failed");

    // 2. 创建共享图像缓冲区 (RGBA)
    let frame_buffer = Arc::new(Mutex::new(None));

    // 音频电平，通常为 [-60, 0]
    let audio_level = Arc::new(Mutex::new(-60.0f32));

    // 3. 创建录制指令通道
    // 使用 unbounded_channel 因为指令频率低，且不希望 UI 线程被阻塞
    let (rec_cmd_tx, rec_cmd_rx) = mpsc::unbounded_channel();

    // 4. 启动视频采集线程
    video::spawn_gst_thread(frame_buffer.clone(), audio_level.clone(), rec_cmd_rx);

    // 5. 运行 egui
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            // .with_fullscreen(true) // Kiosk 模式通常全屏
            .with_inner_size([1280.0, 720.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Professional Camera",
        options,
        Box::new(|cc| {
            // 启用内建的 SVG 支持
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(ui::CameraApp::new(
                frame_buffer,
                audio_level,
                rec_cmd_tx,
            )))
        }),
    )
}
