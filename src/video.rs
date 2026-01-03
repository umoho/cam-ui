use eframe::egui;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;
use parking_lot::Mutex;
use std::sync::Arc;

pub fn spawn_gst_thread(buffer: Arc<Mutex<Option<egui::ColorImage>>>) {
    std::thread::spawn(move || {
        // 采集 RGBA 原始像素，适配 egui
        // let pipeline_str = "videotestsrc ! video/x-raw,format=RGBA,width=1280,height=720 ! appsink name=sink sync=false";
        let pipeline_str = "videotestsrc ! video/x-raw,width=1280,height=720 ! videoconvert ! cairooverlay name=overlay ! videoconvert ! appsink name=sink sync=false";
        // let pipeline_str = "srtsrc uri=\"srt://:7000?mode=listener\" ! tsdemux ! queue ! h264parse ! avdec_h264 ! videoconvert ! videoscale ! video/x-raw,format=RGBA,width=1280,height=720 ! appsink name=sink sync=false";
        let pipeline = gst::parse::launch(pipeline_str).expect("Pipeline error");
        let sink = pipeline
            .downcast_ref::<gst::Bin>()
            .unwrap()
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
        let bus = pipeline.bus().unwrap();
        for _msg in bus.iter_timed(gst::ClockTime::NONE) {}
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
