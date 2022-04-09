use clap::{ArgEnum, Parser};
use gst::prelude::*;

#[derive(Debug, Parser)]
struct Args {
    /// The RTSP url.
    #[clap(long)]
    url: String,

    /// The RTSP user id for authentication.
    #[clap(long)]
    user_id: Option<String>,

    /// The RTSP user password for authentication.
    #[clap(long)]
    password: Option<String>,


    /// The kind of sink to use.
    #[clap(arg_enum, default_value_t = SinkKind::default())]
    sink: SinkKind,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ArgEnum)]
enum SinkKind {
    /// Playback the RTSP stream locally.
    Playback,

    /// Send the RTSP stream to KVS.
    Kvs,
}

impl Default for SinkKind {
    fn default() -> Self {
        SinkKind::Playback
    }
}

fn setup_playback(
    pipeline: &gst::Pipeline,
    rtsp_source: &gst::Element,
    rtph264depay: &gst::Element,
) -> anyhow::Result<()> {
    let h264_parse = gst::ElementFactory::make("h264parse", Some("h264parse"))?;
    let avdec_h264 = gst::ElementFactory::make("avdec_h264", Some("avdec_h264"))?;
    let convert = gst::ElementFactory::make("videoconvert", Some("videoconvert"))?;
    let sink = gst::ElementFactory::make("autovideosink", Some("videosink"))?;

    pipeline.add_many(&[
        rtsp_source,
        rtph264depay,
        &h264_parse,
        &avdec_h264,
        &convert,
        &sink,
    ])?;
    gst::Element::link_many(&[
        rtph264depay,
        &h264_parse,
        &avdec_h264,
        &convert,
        &sink,
    ])?;

    Ok(())
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let args = Args::parse();

    gst::init()?;

    let pipeline = gst::Pipeline::new(Some("rtsp-to-kvs-pipeline"));
    let rtsp_source = gst::ElementFactory::make("rtspsrc", Some("source"))?;
    rtsp_source.try_set_property("location", args.url)?;
    if let Some(user_id) = args.user_id {
        rtsp_source.try_set_property("user-id", user_id)?;
    }
    if let Some(password) = args.password {
        rtsp_source.try_set_property("user-pw", password)?;
    }
    let rtph264depay = gst::ElementFactory::make("rtph264depay", Some("rtph264depay"))?;
    match args.sink {
        SinkKind::Playback => setup_playback(&pipeline, &rtsp_source, &rtph264depay)?,
        SinkKind::Kvs => todo!(),
    }

    rtsp_source.connect_pad_added(move |src, src_pad| {
        log::info!("[rtspsrc] Received new pad {} from {}", src_pad.name(), src.name());

        let sink_pad = rtph264depay.static_pad("sink").expect("Filed to get static sink pad from rtph264depay");
        if sink_pad.is_linked() {
            log::info!("Already linked; ignoring");
            return;
        }

        let new_pad_caps = src_pad.current_caps().expect("Failed to get caps of new pad");
        let new_pad_struct = new_pad_caps.structure(0).expect("Failed to get first structure of code");
        if new_pad_struct.name() == "application/x-rtp" && new_pad_struct.get("media").map_or(false, |m: String| m == "video") {
            match src_pad.link(&sink_pad) {
                Ok(_) => log::info!("Link succeeded"),
                Err(err) => log::error!("Failed to link: {}", err),
            }
        }
    });
    pipeline.set_state(gst::State::Playing)?;

    let bus = pipeline.bus().unwrap();
    for msg in bus.iter_timed(gst::ClockTime::NONE) {
        use gst::MessageView;

        match msg.view() {
            MessageView::Error(err) => {
                gst_error!(err);
                break;
            }
            MessageView::Warning(warning) => {
                gst_warn!(warning);
            }
            MessageView::Info(info) => {
                gst_info!(info);
            }
            MessageView::Eos(..) => {
                log::error!("Received EOS");
                break;
            }
            _ => {},
        }
    }
    
    pipeline.set_state(gst::State::Null)?;

    Ok(())
}

#[macro_export]
macro_rules! gst_log {
    ($logger:ident $obj:expr) => {
        if let Some(element) = $obj.src().map(|s| s.path_string()) {
            log::$logger!("element {}: {}", element, $obj.error());
        } else {
            log::$logger!("unknown element: {}", $obj.error());
        }
        if let Some(dbg) = $obj.debug() {
            log::debug!("{}", dbg);
        }
    }
}

#[macro_export]
macro_rules! gst_info {
    ($obj:expr) => {
        gst_log!(info $obj) 
    };
}


#[macro_export]
macro_rules! gst_warn {
    ($obj:expr) => {
        gst_log!(warn $obj) 
    };
}


#[macro_export]
macro_rules! gst_error {
    ($obj:expr) => {
        gst_log!(error $obj) 
    };
}
