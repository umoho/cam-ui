use eframe::egui;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;
use parking_lot::Mutex;
use std::sync::Arc;
use tokio::sync::mpsc;

pub(crate) mod record;

pub fn spawn_gst_thread(
    buffer: Arc<Mutex<Option<egui::ColorImage>>>,
    mut rec_cmd_rx: mpsc::UnboundedReceiver<record::RecordCommand>,
) {
    std::thread::spawn(move || {
        // 采集 RGBA 原始像素，适配 egui
        let pipeline_str = r#"
            videotestsrc name=src !
            video/x-raw !
            videoconvert !
            tee name=t

            t. ! queue name=q_prev !
            videoscale !
            video/x-raw,width=1280,height=720 !
            cairooverlay name=overlay !
            videoconvert !
            video/x-raw,format=RGBA !
            appsink name=sink sync=false
            "#;
        let pipeline = gst::parse::launch(pipeline_str)
            .expect("Pipeline error")
            .dynamic_cast::<gst::Pipeline>()
            .unwrap();

        let tee = pipeline.by_name("t").unwrap();

        let sink = pipeline
            .by_name("sink")
            .unwrap()
            .dynamic_cast::<gst_app::AppSink>()
            .expect("Sink error");
        sink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    let buffer_gst = sample.buffer().ok_or(gst::FlowError::Error)?;
                    let caps = sample.caps().expect("No caps");
                    let info = gst_video::VideoInfo::from_caps(caps).expect("Invalid caps");

                    let map = buffer_gst
                        .map_readable()
                        .map_err(|_| gst::FlowError::Error)?;

                    // 构建 egui 兼容的图像格式
                    let pixels = map.as_slice();
                    let color_image = egui::ColorImage::from_rgba_unmultiplied(
                        [info.width() as usize, info.height() as usize],
                        pixels,
                    );

                    *buffer.lock() = Some(color_image);
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );

        let overlay = pipeline
            .dynamic_cast_ref::<gst::Pipeline>()
            .unwrap()
            .by_name("overlay")
            .unwrap();
        overlay.connect("draw", false, draw_overlay);

        pipeline.set_state(gst::State::Playing).ok();

        let mut current_recording: Option<record::ActiveRecording> = None;
        let bus = pipeline.bus().unwrap();

        loop {
            // 1. 处理来自 UI 的指令 (非阻塞)
            while let Ok(cmd) = rec_cmd_rx.try_recv() {
                match cmd {
                    record::RecordCommand::Start(settings) => {
                        if current_recording.is_none() {
                            match record::start_recording(&pipeline, &tee, settings) {
                                Ok(active) => current_recording = Some(active),
                                Err(e) => eprintln!("Start Rec Error: {}", e),
                            }
                        }
                    }
                    record::RecordCommand::Stop => {
                        if let Some(active) = current_recording.take() {
                            // 这里调用之前定义的 stop_recording
                            record::stop_recording(&pipeline, &tee, active);
                        }
                    }
                }
            }

            // 2. 处理总线消息 (带超时的轮询，防止 CPU 占用 100%)
            if let Some(msg) = bus.timed_pop(gst::ClockTime::from_mseconds(10)) {
                use gst::MessageView;
                match msg.view() {
                    MessageView::Error(err) => {
                        eprintln!("Pipeline Error: {}", err.error());
                        break; // 发生错误退出循环
                    }
                    MessageView::Eos(_) => break, // 收到结束信号退出
                    _ => (),
                }
            }

            // NOTE: 如果需要极高性能，可以移除 sleep
            // 但在带有指令轮询的循环中，适当的微小延迟是有益的
        }
        // 3. 退出前的清理 (防止程序崩溃导致文件损坏)
        if let Some(active) = current_recording.take() {
            record::stop_recording(&pipeline, &tee, active);
        }
        let _ = pipeline.set_state(gst::State::Null);
    });
}

fn draw_overlay(values: &[cairo::glib::Value]) -> Option<cairo::glib::Value> {
    // values[0]: cairooverlay 元素本身
    // values[1]: cairo::Context
    // values[2]: timestamp
    // values[3]: duration
    let _overlay = values[0].get::<gst::Element>().unwrap();
    let cr = values[1].get::<cairo::Context>().unwrap();

    // 获取当前分辨率
    // NOTE: 可以缓存这些信息以提高性能
    let pad = _overlay.static_pad("sink").unwrap();
    let caps = pad.current_caps().unwrap();
    let structure = caps.structure(0).unwrap();
    let width = structure.get::<i32>("width").unwrap() as f64;
    let height = structure.get::<i32>("height").unwrap() as f64;

    // --- 开始绘制 ---

    // 绘制三分参考线
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.5); // 白色，0.5 透明度
    cr.set_line_width(2.0);
    // 垂直线
    for i in 1..3 {
        let x = width / 3.0 * i as f64;
        cr.move_to(x, 0.0);
        cr.line_to(x, height);
    }
    // 水平线
    for i in 1..3 {
        let y = height / 3.0 * i as f64;
        cr.move_to(0.0, y);
        cr.line_to(width, y);
    }
    cr.stroke().expect("Stroke failed");

    None
}
