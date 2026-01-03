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
        let pipeline_str = "srtsrc uri=\"srt://:7000?mode=listener\" ! tsdemux ! queue ! h264parse ! avdec_h264 ! videoconvert ! videoscale ! video/x-raw,format=RGBA,width=1280,height=720 ! appsink name=sink sync=false";
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

        pipeline.set_state(gst::State::Playing).ok();
        let bus = pipeline.bus().unwrap();
        for _msg in bus.iter_timed(gst::ClockTime::NONE) {}
    });
}
