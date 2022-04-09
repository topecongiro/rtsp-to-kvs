use anyhow::Context;
use clap::{Args, Parser, Subcommand};
use gst::prelude::*;

#[derive(Debug, Parser)]
#[clap(author, version, about)]
#[clap(infer_subcommands = true)]
struct Cli {
    /// The kind of sink to use.
    #[clap(subcommand)]
    command: Commands,
}

impl Cli {
    fn rtsp_config(&self) -> &RtspConfig {
        match &self.command {
            Commands::PlayBack { rtsp_config: rtsp_setting } => rtsp_setting,
            Commands::Kvs { rtsp_config: rtsp_setting, .. } => rtsp_setting,
        }
    }
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Playback the RTSP stream (video only)
    PlayBack {
        #[clap(flatten)]
        rtsp_config: RtspConfig,
    },
    /// Stream the RTSP stream to Kinesis Video Stream
    Kvs {
        #[clap(flatten)]
        kvs_config: KvsConfig,
        #[clap(flatten)]
        rtsp_config: RtspConfig,
    },
}

#[derive(Debug, Args)]
struct RtspConfig {
    /// The RTSP url.
    #[clap(long)]
    url: String,

    /// The RTSP user id for authentication.
    #[clap(long)]
    user_id: Option<String>,

    /// The RTSP user password for authentication.
    #[clap(long)]
    password: Option<String>,
}

#[derive(Debug, Args)]
struct KvsConfig {
        /// AWS access key.
        #[clap(long)]
        aws_access_key_id: String,
        /// AWS secret key.
        #[clap(long)]
        aws_secret_key: String,
        /// The stream name of Kinesis Video Stream
        #[clap(long)]
        stream_name: String,
        /// AWS region.
        #[clap(long)]
        aws_region: Option<String>,
}

fn create_element(factoryname: &str, name: &str) -> anyhow::Result<gst::Element> {
    gst::ElementFactory::make(factoryname, Some(name))
        .with_context(|| format!("Failed to create {}", factoryname))
}

fn setup_playback(
    pipeline: &gst::Pipeline,
    rtsp_source: &gst::Element,
    rtph264depay: &gst::Element,
) -> anyhow::Result<()> {
    let h264_parse = create_element("h264parse", "h264parse")?;
    let avdec_h264 = create_element("avdec_h264", "avdec_h264")?;
    let convert = create_element("videoconvert", "videoconvert")?;
    let sink = create_element("autovideosink", "videosink")?;

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

fn setup_kvssink(
    pipeline: &gst::Pipeline,
    rtsp_source: &gst::Element,
    rtph264depay: &gst::Element,
    kvs_config: &KvsConfig,
) -> anyhow::Result<()> {

    let h264_parse = create_element("h264parse", "h264parse")?;
    let kvssink = create_element("kvssink", "kvssink")?;
    kvssink.try_set_property("access-key", kvs_config.aws_access_key_id.as_str())?;
    kvssink.try_set_property("secret-key", kvs_config.aws_secret_key.as_str())?;
    kvssink.try_set_property("stream-name", kvs_config.stream_name.as_str())?;
    if let Some(ref aws_region) = kvs_config.aws_region {
        kvssink.try_set_property("aws-region", aws_region)?;
    }

    pipeline.add_many(&[
        rtsp_source,
        rtph264depay,
        &h264_parse,
        &kvssink,
    ])?;
    gst::Element::link_many(&[
        rtph264depay,
        &h264_parse,
        &kvssink,
    ])?;

    Ok(())
}

fn rtspsrc(rtsp_config: &RtspConfig) -> anyhow::Result<gst::Element> {
    let rtsp_source = create_element("rtspsrc", "source")?;
    rtsp_source.try_set_property("location", rtsp_config.url.as_str())?;
    if let Some(ref user_id) = rtsp_config.user_id {
        rtsp_source.try_set_property("user-id", user_id)?;
    }
    if let Some(ref password) = rtsp_config.password {
        rtsp_source.try_set_property("user-pw", password)?;
    }
    Ok(rtsp_source)
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let args = Cli::parse();

    gst::init()?;

    let pipeline = gst::Pipeline::new(Some("rtsp-to-kvs-pipeline"));
    let rtsp_source = rtspsrc(args.rtsp_config())?;
    let rtph264depay = create_element("rtph264depay", "rtph264depay")?;
    match args.command {
        Commands::PlayBack { .. } => setup_playback(&pipeline, &rtsp_source, &rtph264depay)?,
        Commands::Kvs { kvs_config, .. } => setup_kvssink(&pipeline, &rtsp_source, &rtph264depay, &kvs_config)?,
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
