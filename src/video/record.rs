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
    tee_pad: gst::Pad,
}

/// 可能的错误: [BoolError], [PadLinkError].
pub(super) fn start_recording(
    pipeline: &gst::Pipeline,
    tee: &gst::Element,
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
        "queue ! videoconvert ! videoscale ! \
            video/x-raw,width={w},height={h},format=I420 ! \
            {enc} ! {mux} ! filesink location={path}",
        w = settings.res.width,
        h = settings.res.height,
        enc = enc_plugin,
        mux = mux_plugin,
        path = path_str
    );

    // 3. 将字符串转为 Element (Bin)
    let bin = gst::parse::bin_from_description(&bin_desc, true)?;
    // 4. 将新创建的 Bin 添加进运行中的 Pipeline
    pipeline.add(&bin)?;
    // 5. 从 Tee 申请一个动态出口 Pad (Request Pad)
    // "src_%u" 是 tee 的命名模板，GStreamer 会自动分配 src_0, src_1 等
    let tee_src_pad = tee
        .request_pad_simple("src_%u")
        .expect("Failed to request pad from tee");
    // 6. 获取 Bin 的入口 Pad (通常是刚才 queue 的 sink pad)
    let bin_sink_pad = bin
        .static_pad("sink")
        .expect("Failed to get sink pad from recording bin");
    // 7. 物理链接
    tee_src_pad.link(&bin_sink_pad)?;
    // 8. 启动该分支的状态 (同步到父管线的 Playing 状态)
    bin.sync_state_with_parent()?;

    Ok(ActiveRecording {
        bin: bin.into(),
        tee_pad: tee_src_pad,
    })
}

pub(super) fn stop_recording(
    pipeline: &gst::Pipeline,
    tee: &gst::Element,
    active: ActiveRecording,
) {
    let pipeline = pipeline.clone();
    let tee = tee.clone();
    let bin = active.bin.clone();
    let tee_pad = active.tee_pad.clone();

    // 1. 在 tee 的出口 pad 上添加一个 IDLE 探针
    // IDLE 探针会在该线路上没有数据流动（空闲）时触发回调
    tee_pad.add_probe(gst::PadProbeType::IDLE, move |pad, _info| {
        println!("Tee pad is idle, starting safe teardown...");

        // 2. 立即断开链接，防止录制分支的状态变化反向阻塞 tee
        // 我们需要获取 bin 的 sink pad 来执行 unlink
        if let Some(bin_sink_pad) = bin.static_pad("sink") {
            let _ = pad.unlink(&bin_sink_pad);
        }

        // 3. 向录制分支发送 EOS，确保文件正确关闭
        bin.send_event(gst::event::Eos::new());

        // 4. 由于我们在探针回调内（属于流线程），不能直接在这里做繁重的清理
        // 建议在一个新线程中完成最后的 Null 切换和移除，或者稍后在主循环处理
        let bin_inner = bin.clone();
        let tee_inner = tee.clone();
        let pad_inner = pad.clone();
        let pipeline_inner = pipeline.clone();

        std::thread::spawn(move || {
            // 给 EOS 一点时间流过编码器
            std::thread::sleep(std::time::Duration::from_millis(300));

            bin_inner.set_state(gst::State::Null).ok();
            tee_inner.release_request_pad(&pad_inner);
            pipeline_inner.remove(&bin_inner).ok();

            println!("Recording stopped and file finalized.");
        });

        // 5. 返回 Remove 意味着探针执行一次后自动移除
        gst::PadProbeReturn::Remove
    });
}
