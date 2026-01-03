mod icons;
mod ui;
mod video;

use eframe::egui;
use parking_lot::Mutex;
use std::sync::Arc;

fn main() -> eframe::Result {
    // 1. 初始化 GStreamer
    gstreamer::init().expect("GStreamer init failed");

    // 2. 创建共享图像缓冲区 (RGBA)
    let frame_buffer = Arc::new(Mutex::new(None));

    // 3. 启动视频采集线程
    video::spawn_gst_thread(frame_buffer.clone());

    // 4. 运行 egui
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
            Ok(Box::new(ui::CameraApp::new(frame_buffer)))
        }),
    )
}
