//! Linux: H.264 decoding for the RDP graphics pipeline through the system's GStreamer.
//!
//! No codec is bundled: GStreamer picks what the system has — the graphics card (VA-API:
//! `vah264dec`, `vaapih264dec`) first, else the processor (`avdec_h264` from gst-libav,
//! `openh264dec`). Without any, the pipeline does not advertise H.264 and the server sends
//! RemoteFX / bitmaps as before.
//!
//! The pipeline returns the decoded picture as YUV 4:2:0 planes: IronRDP converts them with
//! the colour transform of MS-RDPEGFX and recombines the two views of AVC444.

use std::time::Duration;

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use ironrdp_egfx::decode::{DecodedFrame, DecoderError, DecoderResult, H264Decoder, Yuv420Frame};

/// Decoders tried in order (environment ORBITERM_H264_DECODER forces one).
const DECODERS: &[&str] = &["vah264dec", "vaapih264dec", "avdec_h264", "openh264dec"];

/// Longest wait for the picture of one access unit (the stream has no B-frames: the decoder
/// returns each picture as soon as its data is in).
const PICTURE_TIMEOUT: Duration = Duration::from_millis(1500);

pub struct GstH264 {
    pipeline: gst::Pipeline,
    src: gst_app::AppSrc,
    sink: gst_app::AppSink,
    decoder: &'static str,
    frame: u64,
}

impl GstH264 {
    /// A decoder using the first H.264 decoder element available, None without one.
    pub fn new() -> Option<Self> {
        if let Err(e) = gst::init() {
            log::warn!("H.264 : GStreamer indisponible ({e})");
            return None;
        }
        let forced = std::env::var("ORBITERM_H264_DECODER").ok();
        let candidates: Vec<&str> = match &forced {
            Some(name) => DECODERS.iter().copied().filter(|d| d == name).chain(std::iter::once(name.as_str())).take(1).collect(),
            None => DECODERS.to_vec(),
        };
        for name in candidates {
            if gst::ElementFactory::find(name).is_none() {
                continue;
            }
            match Self::with_decoder(name) {
                Ok(d) => {
                    log::info!("H.264 : décodage par GStreamer ({name})");
                    return Some(d);
                }
                Err(e) => log::warn!("H.264 : décodeur {name} inutilisable : {e}"),
            }
        }
        log::warn!("H.264 : aucun décodeur GStreamer (gst-libav, VA-API ou openh264) : image RemoteFX");
        None
    }

    fn with_decoder(name: &str) -> Result<Self, String> {
        let decoder: &'static str = DECODERS.iter().find(|d| **d == name).copied().unwrap_or("decoder");
        // libav: no frame threading (it holds pictures back); slices are decoded in parallel
        let options = if name == "avdec_h264" { " max-threads=0 thread-type=slice" } else { "" };
        let desc = format!(
            "appsrc name=src format=time is-live=false \
               caps=video/x-h264,stream-format=byte-stream,alignment=au \
             ! h264parse ! {name}{options} ! videoconvert \
             ! video/x-raw,format=I420 \
             ! appsink name=sink sync=false emit-signals=false max-buffers=8"
        );
        let pipeline = gst::parse::launch(&desc)
            .map_err(|e| e.to_string())?
            .downcast::<gst::Pipeline>()
            .map_err(|_| "pipeline".to_string())?;
        let src = pipeline
            .by_name("src")
            .and_then(|e| e.downcast::<gst_app::AppSrc>().ok())
            .ok_or("appsrc")?;
        let sink = pipeline
            .by_name("sink")
            .and_then(|e| e.downcast::<gst_app::AppSink>().ok())
            .ok_or("appsink")?;
        pipeline.set_state(gst::State::Playing).map_err(|e| e.to_string())?;
        Ok(Self { pipeline, src, sink, decoder, frame: 0 })
    }

    /// Pushes one access unit and waits for its picture.
    fn picture(&mut self, data: &[u8]) -> DecoderResult<Yuv420Frame> {
        let annex_b = to_annex_b(data);
        let mut buffer = gst::Buffer::from_mut_slice(annex_b);
        {
            let b = buffer.get_mut().ok_or_else(|| DecoderError::msg("tampon GStreamer"))?;
            // a steady 60 i/s clock: only the order matters
            b.set_pts(gst::ClockTime::from_nseconds(self.frame * 16_666_667));
        }
        self.frame += 1;
        self.src
            .push_buffer(buffer)
            .map_err(|e| DecoderError::msg(format!("{} : {e:?}", self.decoder)))?;
        let sample = self
            .sink
            .try_pull_sample(gst::ClockTime::from_mseconds(PICTURE_TIMEOUT.as_millis() as u64))
            .ok_or_else(|| DecoderError::msg(format!("{} : pas d'image", self.decoder)))?;
        yuv_planes(&sample)
    }
}

impl Drop for GstH264 {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

/// The YUV 4:2:0 planes of an I420 sample, with their strides.
fn yuv_planes(sample: &gst::Sample) -> DecoderResult<Yuv420Frame> {
    let caps = sample.caps().ok_or_else(|| DecoderError::msg("image sans format"))?;
    let s = caps.structure(0).ok_or_else(|| DecoderError::msg("image sans format"))?;
    let width: i32 = s.get("width").map_err(|_| DecoderError::msg("largeur inconnue"))?;
    let height: i32 = s.get("height").map_err(|_| DecoderError::msg("hauteur inconnue"))?;
    let (w, h) = (u16::try_from(width).unwrap_or(0), u16::try_from(height).unwrap_or(0));
    if w == 0 || h == 0 {
        return Err(DecoderError::msg("image vide"));
    }
    let buffer = sample.buffer().ok_or_else(|| DecoderError::msg("image sans données"))?;
    let map = buffer.map_readable().map_err(|_| DecoderError::msg("image illisible"))?;
    let data = map.as_slice();
    // I420 layout of GStreamer's default video info: strides rounded up to 4 bytes,
    // or the offsets/strides of the buffer's video meta when the decoder set one
    let (offsets, strides) = match buffer.meta::<gstreamer_video::VideoMeta>() {
        Some(meta) if meta.n_planes() >= 3 => (
            [meta.offset()[0], meta.offset()[1], meta.offset()[2]],
            [meta.stride()[0] as usize, meta.stride()[1] as usize, meta.stride()[2] as usize],
        ),
        _ => {
            let ys = (usize::from(w) + 3) & !3;
            let cs = (usize::from(w).div_ceil(2) + 3) & !3;
            let ysize = ys * usize::from(h);
            let csize = cs * usize::from(h).div_ceil(2);
            ([0, ysize, ysize + csize], [ys, cs, cs])
        }
    };
    let ch = usize::from(h).div_ceil(2);
    let plane = |i: usize, rows: usize| -> DecoderResult<Vec<u8>> {
        let start = offsets[i];
        let end = start + strides[i] * rows;
        data.get(start..end).map(<[u8]>::to_vec).ok_or_else(|| DecoderError::msg("plan YUV tronqué"))
    };
    Yuv420Frame::new(w, h, plane(0, usize::from(h))?, strides[0], plane(1, ch)?, plane(2, ch)?, strides[1])
}

/// AVC-format NAL units (4-byte big-endian lengths) to an Annex B byte stream; Annex B input
/// is returned unchanged.
fn to_annex_b(data: &[u8]) -> Vec<u8> {
    if data.starts_with(&[0, 0, 0, 1]) || data.starts_with(&[0, 0, 1]) {
        return data.to_vec();
    }
    let mut out = Vec::with_capacity(data.len() + 16);
    let mut rest = data;
    while rest.len() >= 4 {
        let n = u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
        let Some(nal) = rest.get(4..4 + n) else { return data.to_vec() };
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(nal);
        rest = &rest[4 + n..];
    }
    if rest.is_empty() { out } else { data.to_vec() }
}

impl H264Decoder for GstH264 {
    fn decode(&mut self, data: &[u8]) -> DecoderResult<DecodedFrame> {
        Ok(self.picture(data)?.to_rgba())
    }

    fn decode_yuv420(&mut self, data: &[u8]) -> DecoderResult<Yuv420Frame> {
        self.picture(data)
    }

    fn supports_yuv420(&self) -> bool {
        true
    }

    fn reset(&mut self) {
        // a new stream starts with its own parameter sets: the decoder resynchronises on them
        let _ = self.src.send_event(gst::event::FlushStart::new());
        let _ = self.src.send_event(gst::event::FlushStop::new(true));
        while self.sink.try_pull_sample(gst::ClockTime::ZERO).is_some() {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avc_to_annex_b() {
        assert_eq!(to_annex_b(&[0, 0, 0, 1, 0x67]), vec![0, 0, 0, 1, 0x67]);
        assert_eq!(to_annex_b(&[0, 0, 0, 2, 0x67, 0x42, 0, 0, 0, 1, 0x68]), vec![0, 0, 0, 1, 0x67, 0x42, 0, 0, 0, 1, 0x68]);
        // not a length-prefixed stream: unchanged
        assert_eq!(to_annex_b(&[0, 0, 0, 9, 1]), vec![0, 0, 0, 9, 1]);
    }

    /// Encodes a test picture with whatever H.264 encoder GStreamer has, then decodes it.
    /// Skipped (passes) when the system has no encoder or no decoder.
    #[test]
    fn decodes_a_stream() {
        if gst::init().is_err() {
            return;
        }
        let Some(enc) = ["x264enc", "openh264enc", "vah264enc", "avenc_h264"].into_iter().find(|e| gst::ElementFactory::find(e).is_some())
        else {
            eprintln!("aucun encodeur H.264 : test sauté");
            return;
        };
        let Some(mut dec) = GstH264::new() else {
            eprintln!("aucun décodeur H.264 : test sauté");
            return;
        };
        let opts = if enc == "x264enc" { " tune=zerolatency bframes=0" } else { "" };
        let p = gst::parse::launch(&format!(
            "videotestsrc num-buffers=5 pattern=smpte ! video/x-raw,width=320,height=240,format=I420 \
             ! {enc}{opts} ! video/x-h264,profile=constrained-baseline \
             ! h264parse config-interval=-1 ! video/x-h264,stream-format=byte-stream,alignment=au \
             ! appsink name=out sync=false"
        ))
        .unwrap()
        .downcast::<gst::Pipeline>()
        .unwrap();
        let out = p.by_name("out").unwrap().downcast::<gst_app::AppSink>().unwrap();
        p.set_state(gst::State::Playing).unwrap();
        let mut decoded = 0;
        while let Some(s) = out.try_pull_sample(gst::ClockTime::from_seconds(5)) {
            let map = s.buffer().unwrap().map_readable().unwrap();
            let f = dec.decode_yuv420(map.as_slice()).expect("image décodée");
            assert_eq!((f.width(), f.height()), (320, 240));
            decoded += 1;
        }
        p.set_state(gst::State::Null).unwrap();
        assert_eq!(decoded, 5);
    }
}
