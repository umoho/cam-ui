use std::path::PathBuf;

use gstreamer as gst;
use gstreamer::prelude::*;

#[derive(Debug, Clone)]
pub enum RecordCommand {
    Start(RecordSettings),
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum VideoEncoder {
    H264,
    H265,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Container {
    MP4,
    MOV,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Resolution {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct RecordSettings {
    pub res: Resolution,
    pub enc: VideoEncoder,
    pub container: Container,
    pub filepath: PathBuf,
}

/// 内部结构, 用于记住当前正在录制的组件, 以便后续释放.
pub(super) struct ActiveRecording {
    bin: gst::Element,
    video_tee_pad: gst::Pad,
    audio_tee_pad: gst::Pad,
}

/// 可能的错误: [BoolError], [PadLinkError].
pub(super) fn start_recording(
    pipeline: &gst::Pipeline,
    video_tee: &gst::Element,
    audio_tee: &gst::Element,
    settings: RecordSettings,
) -> Result<ActiveRecording, Box<dyn std::error::Error + Send + Sync>> {
    // 1. 根据配置映射插件名称
    let enc_plugin = match settings.enc {
        VideoEncoder::H264 => "x264enc tune=zerolatency",
        VideoEncoder::H265 => "x265enc tune=zerolatency",
    };
    let mux_plugin = match settings.container {
        Container::MP4 => "mp4mux faststart=true", // 加上 faststart 提高兼容性
        Container::MOV => "qtmux",
    };
    let path_str = settings.filepath.to_string_lossy();

    // 2. 构造录制分支字符串 (Bin)
    // 流程：队列缓冲 -> 格式转换 -> 缩放尺寸 -> 编码 -> 封装 -> 写入文件
    // NOTE: format=I420 修复 QuickTime Player 打不开 MP4 的问题
    let bin_desc = format!(
        "bin.(
            queue name=q_v !
            videoconvert !
            videoscale !
            video/x-raw,width={w},height={h},format=I420 !
            {enc_v} !
            mux.video_0

            queue name=q_a !
            audioconvert !
            audioresample !
            fdkaacenc !
            aacparse !
            mux.audio_0

            {mux} name=mux !
            filesink location={path}
        )",
        w = settings.res.width,
        h = settings.res.height,
        enc_v = enc_plugin,
        mux = mux_plugin,
        path = path_str
    );
    let bin = gst::parse::bin_from_description(&bin_desc, false)?;
    pipeline.add(&bin)?;

    // 添加 Ghost Pads
    let v_inner_sink = bin.by_name("q_v").unwrap().static_pad("sink").unwrap();
    let v_ghost_pad = gst::GhostPad::builder_with_target(&v_inner_sink)?
        .name("v_sink")
        .build();
    v_ghost_pad.set_active(true)?;
    bin.add_pad(&v_ghost_pad)?;

    let a_inner_sink = bin.by_name("q_a").unwrap().static_pad("sink").unwrap();
    let a_ghost_pad = gst::GhostPad::builder_with_target(&a_inner_sink)?
        .name("a_sink")
        .build();
    a_ghost_pad.set_active(true)?;
    bin.add_pad(&a_ghost_pad)?;

    let video_tee_pad = video_tee.request_pad_simple("src_%u").unwrap();
    video_tee_pad.link(&v_ghost_pad)?;

    let audio_tee_pad = audio_tee.request_pad_simple("src_%u").unwrap();
    audio_tee_pad.link(&a_ghost_pad)?;

    // 启动该分支的状态 (同步到父管线的 Playing 状态)
    bin.sync_state_with_parent()?;

    Ok(ActiveRecording {
        bin: bin.into(),
        video_tee_pad,
        audio_tee_pad,
    })
}

pub(super) fn stop_recording(
    pipeline: &gst::Pipeline,
    video_tee: &gst::Element,
    audio_tee: &gst::Element,
    active: ActiveRecording,
) {
    // GStreamer 对象（Element, Pad等）内部是引用计数，克隆代价很小
    let pipeline_c = pipeline.clone();
    let bin_el = active.bin.clone();
    let v_tee_src = active.video_tee_pad.clone();
    let a_tee_src = active.audio_tee_pad.clone();
    let vt_clone = video_tee.clone();
    let at_clone = audio_tee.clone();

    v_tee_src
        .clone()
        .add_probe(gst::PadProbeType::IDLE, move |v_src, _info| {
            println!("Tee pad is idle, starting safe teardown...");

            let bin = bin_el.clone().dynamic_cast::<gst::Bin>().unwrap();

            let v_ghost_pad = bin.static_pad("v_sink").unwrap();
            let a_ghost_pad = bin.static_pad("a_sink").unwrap();

            // 断开视频和音频
            let _ = v_src.unlink(&v_ghost_pad);
            let _ = a_tee_src.unlink(&a_ghost_pad);

            // 发送 EOS
            bin.send_event(gst::event::Eos::new());

            // 为后台清理线程准备克隆
            let bin_for_cleanup = bin_el.clone();
            let tv_for_cleanup = vt_clone.clone();
            let ta_for_cleanup = at_clone.clone();
            let vp_for_cleanup = v_tee_src.clone();
            let ap_for_cleanup = a_tee_src.clone();
            let pipe_for_cleanup = pipeline_c.clone();

            std::thread::spawn(move || {
                // 给编码器排空数据的时间
                std::thread::sleep(std::time::Duration::from_millis(600));

                bin_for_cleanup.set_state(gst::State::Null).ok();
                tv_for_cleanup.release_request_pad(&vp_for_cleanup);
                ta_for_cleanup.release_request_pad(&ap_for_cleanup);
                pipe_for_cleanup.remove(&bin_for_cleanup).ok();

                println!("AV Recording Stopped and cleaned up.");
            });

            gst::PadProbeReturn::Remove
        });
}
