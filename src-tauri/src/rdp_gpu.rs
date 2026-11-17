//! Graphics pipeline composited by the tab ("GPU passthrough", Windows).
//!
//! Like the Windows client, which decodes H.264 with the graphics card and composites the
//! surfaces there, the tab decodes the H.264 streams itself (WebCodecs, hardware decoder) and
//! keeps the surfaces and the bitmap cache in WebGL textures. The backend no longer converts
//! pictures to RGBA nor sends them: it forwards the surface commands of the graphics pipeline
//! (MS-RDPEGFX) as they arrive, H.264 still compressed. The other codecs (ClearCodec, planar,
//! RemoteFX) are still decoded here and sent as RGBA areas of their surface.
//!
//! Messages for the tab (binary, little endian, `id` = surface, `rect` = left, top, right,
//! bottom with right and bottom exclusive, all u16):
//! - `20` graphics reset (w, h): every surface is deleted;
//! - `21` surface created (id, w, h), `22` deleted (id);
//! - `23` surface mapped to the output (id, x, y, target w, target h; 0 = surface size);
//! - `24` solid fill (id, r, g, b, n, n × rect);
//! - `25` surface to surface (source id, destination id, rect, n, n × point x, y);
//! - `26` surface to cache (id, slot, rect), `27` cache to surface (slot, id, n, n × point),
//!   `28` cache slot evicted (slot);
//! - `29` RGBA area (id, rect, pixels);
//! - `30` H.264 (id, codec: 0 AVC420, 1 AVC444, 2 AVC444v2, views: bit 0 main, bit 1 auxiliary,
//!   then for each view present: n, n × rect, length u32, Annex B bitstream);
//! - `31` H.264 decoded here, AVC420 (Linux, the tab has no H.264 decoder): id, n, n × rect,
//!   then the area sent (x, y, w, h, even x and y) and its YUV 4:2:0 planes, rows packed: Y (w × h),
//!   U and V (ceil(w/2) × ceil(h/2)); the tab converts them as MS-RDPEGFX 3.3.8.3.1 does;
//! - `32` end of a frame (frame id u32), turned by the session into its frame marker;
//! - `33` several of the above in one WebSocket message: each as its length (u32) and bytes.

use std::sync::{Arc, Mutex};

use ironrdp_egfx::client::{BitmapUpdate, GpuAvc, GraphicsPipelineHandler, Surface};
use ironrdp_egfx::decode::Yuv420Frame;
use ironrdp_egfx::pdu::{
    CacheToSurfacePdu, Codec1Type, EvictCacheEntryPdu, MapSurfaceToScaledOutputPdu, Point, SolidFillPdu,
    SurfaceToCachePdu, SurfaceToSurfacePdu,
};
use ironrdp_pdu::geometry::ExclusiveRectangle;

pub const RESET: u8 = 20;
pub const END_FRAME: u8 = 32;

/// Messages waiting for the tab beyond this size (a tab that stopped reading): they are
/// dropped and the desktop is asked again in full.
const MAX_QUEUED: usize = 256 << 20;

#[derive(Default)]
pub struct GpuOut {
    /// passthrough on: the handler records the commands
    pub on: bool,
    pub msgs: Vec<Vec<u8>>,
    pub bytes: usize,
    /// messages were dropped: the tab's picture is incomplete
    pub overflow: bool,
}

impl GpuOut {
    pub fn clear(&mut self) {
        self.msgs.clear();
        self.bytes = 0;
        self.overflow = false;
    }

    pub fn take(&mut self) -> Vec<Vec<u8>> {
        self.bytes = 0;
        std::mem::take(&mut self.msgs)
    }
}

pub type GpuQueue = Arc<Mutex<GpuOut>>;

/// Messages joined into one (`33`).
#[derive(Default)]
pub struct Batch(Vec<u8>);

impl Batch {
    pub fn push(&mut self, m: &[u8]) {
        if self.0.is_empty() {
            self.0.push(33);
        }
        self.0.extend_from_slice(&(m.len() as u32).to_le_bytes());
        self.0.extend_from_slice(m);
    }

    pub fn take(&mut self) -> Option<Vec<u8>> {
        (!self.0.is_empty()).then(|| std::mem::take(&mut self.0))
    }
}

/// Handler of the graphics pipeline: records the commands for the tab while the passthrough
/// is on, does nothing otherwise (the session image is composed by IronRDP).
pub struct GfxEvents(pub GpuQueue);

impl GfxEvents {
    fn push(&mut self, build: impl FnOnce() -> Vec<u8>) {
        let Ok(mut q) = self.0.lock() else { return };
        if !q.on || q.overflow {
            return;
        }
        let m = build();
        if q.bytes + m.len() > MAX_QUEUED {
            q.overflow = true;
            q.clear();
            q.overflow = true;
            return;
        }
        q.bytes += m.len();
        q.msgs.push(m);
    }
}

fn u16s(m: &mut Vec<u8>, values: &[u16]) {
    for v in values {
        m.extend_from_slice(&v.to_le_bytes());
    }
}

fn rect(m: &mut Vec<u8>, r: &ExclusiveRectangle) {
    u16s(m, &[r.left, r.top, r.right, r.bottom]);
}

fn points(m: &mut Vec<u8>, p: &[Point]) {
    u16s(m, &[p.len() as u16]);
    for p in p {
        u16s(m, &[p.x, p.y]);
    }
}

fn clamp16(v: u32) -> u16 {
    u16::try_from(v).unwrap_or(u16::MAX)
}

/// Message `30`, see the module documentation.
pub fn avc_message(avc: &GpuAvc<'_>) -> Vec<u8> {
    let codec = match avc.codec_id {
        Codec1Type::Avc444 => 1,
        Codec1Type::Avc444v2 => 2,
        _ => 0,
    };
    let views = u8::from(avc.main.is_some()) | (u8::from(avc.auxiliary.is_some()) << 1);
    let size: usize = [avc.main, avc.auxiliary].iter().flatten().map(|(r, d)| 6 + r.len() * 8 + d.len()).sum();
    let mut m = Vec::with_capacity(5 + size);
    m.push(30);
    u16s(&mut m, &[avc.surface_id]);
    m.push(codec);
    m.push(views);
    for (rects, data) in [avc.main, avc.auxiliary].into_iter().flatten() {
        u16s(&mut m, &[rects.len() as u16]);
        for r in rects {
            rect(&mut m, r);
        }
        m.extend_from_slice(&(data.len() as u32).to_le_bytes());
        m.extend_from_slice(data);
    }
    m
}

/// Message `31`, see the module documentation; `None` when no rectangle is inside the picture.
pub fn yuv_message(surface_id: u16, rects: &[ExclusiveRectangle], picture: &Yuv420Frame) -> Option<Vec<u8>> {
    let (pw, ph) = (picture.width(), picture.height());
    let rects: Vec<_> = rects
        .iter()
        .map(|r| (r.left, r.top, r.right.min(pw), r.bottom.min(ph)))
        .filter(|(l, t, r, b)| l < r && t < b)
        .collect();
    // the area sent: the rectangles' bounds, on whole 2x2 chroma blocks
    let left = rects.iter().map(|r| r.0).min()? & !1;
    let top = rects.iter().map(|r| r.1).min()? & !1;
    let right = rects.iter().map(|r| r.2).max()?;
    let bottom = rects.iter().map(|r| r.3).max()?;
    let (w, h) = (usize::from(right - left), usize::from(bottom - top));
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    let (l, t) = (usize::from(left), usize::from(top));
    let mut m = Vec::with_capacity(13 + rects.len() * 8 + w * h + 2 * cw * ch);
    m.push(31);
    u16s(&mut m, &[surface_id, rects.len() as u16]);
    for (rl, rt, rr, rb) in &rects {
        u16s(&mut m, &[*rl, *rt, *rr, *rb]);
    }
    u16s(&mut m, &[left, top, w as u16, h as u16]);
    for row in t..t + h {
        m.extend_from_slice(picture.luma_row(row)?.get(l..l + w)?);
    }
    for plane in [Yuv420Frame::u_row, Yuv420Frame::v_row] {
        for row in t / 2..t / 2 + ch {
            m.extend_from_slice(plane(picture, row)?.get(l / 2..l / 2 + cw)?);
        }
    }
    Some(m)
}

impl GraphicsPipelineHandler for GfxEvents {
    fn on_reset_graphics(&mut self, width: u32, height: u32) {
        self.push(|| {
            let mut m = vec![RESET];
            u16s(&mut m, &[clamp16(width), clamp16(height)]);
            m
        });
    }

    fn on_surface_created(&mut self, s: &Surface) {
        self.push(|| {
            let mut m = vec![21];
            u16s(&mut m, &[s.id, s.width, s.height]);
            m
        });
    }

    fn on_surface_deleted(&mut self, id: u16) {
        self.push(|| {
            let mut m = vec![22];
            u16s(&mut m, &[id]);
            m
        });
    }

    fn on_surface_mapped(&mut self, id: u16, x: u32, y: u32) {
        self.push(|| {
            let mut m = vec![23];
            u16s(&mut m, &[id, clamp16(x), clamp16(y), 0, 0]);
            m
        });
    }

    fn on_map_surface_to_scaled_output(&mut self, p: &MapSurfaceToScaledOutputPdu) {
        self.push(|| {
            let mut m = vec![23];
            u16s(
                &mut m,
                &[
                    p.surface_id,
                    clamp16(p.output_origin_x),
                    clamp16(p.output_origin_y),
                    clamp16(p.target_width),
                    clamp16(p.target_height),
                ],
            );
            m
        });
    }

    fn on_solid_fill(&mut self, p: &SolidFillPdu) {
        self.push(|| {
            let mut m = vec![24];
            u16s(&mut m, &[p.surface_id]);
            m.extend_from_slice(&[p.fill_pixel.r, p.fill_pixel.g, p.fill_pixel.b]);
            u16s(&mut m, &[p.rectangles.len() as u16]);
            for r in &p.rectangles {
                rect(&mut m, r);
            }
            m
        });
    }

    fn on_surface_to_surface(&mut self, p: &SurfaceToSurfacePdu) {
        self.push(|| {
            let mut m = vec![25];
            u16s(&mut m, &[p.source_surface_id, p.destination_surface_id]);
            rect(&mut m, &p.source_rectangle);
            points(&mut m, &p.destination_points);
            m
        });
    }

    fn on_surface_to_cache(&mut self, p: &SurfaceToCachePdu) {
        self.push(|| {
            let mut m = vec![26];
            u16s(&mut m, &[p.surface_id, p.cache_slot]);
            rect(&mut m, &p.source_rectangle);
            m
        });
    }

    fn on_cache_to_surface(&mut self, p: &CacheToSurfacePdu) {
        self.push(|| {
            let mut m = vec![27];
            u16s(&mut m, &[p.cache_slot, p.surface_id]);
            points(&mut m, &p.destination_points);
            m
        });
    }

    fn on_evict_cache_entry(&mut self, p: &EvictCacheEntryPdu) {
        self.push(|| {
            let mut m = vec![28];
            u16s(&mut m, &[p.cache_slot]);
            m
        });
    }

    fn on_bitmap_updated(&mut self, u: &BitmapUpdate) {
        if u.data.is_empty() {
            return;
        }
        self.push(|| {
            let mut m = Vec::with_capacity(11 + u.data.len());
            m.push(29);
            u16s(&mut m, &[u.surface_id]);
            rect(&mut m, &u.destination_rectangle);
            m.extend_from_slice(&u.data);
            m
        });
    }

    fn on_gpu_avc(&mut self, avc: &GpuAvc<'_>) {
        self.push(|| avc_message(avc));
    }

    fn on_gpu_yuv(&mut self, surface_id: u16, rects: &[ExclusiveRectangle], picture: &Yuv420Frame) -> bool {
        match yuv_message(surface_id, rects, picture) {
            Some(m) => {
                self.push(|| m);
                true
            }
            None => false,
        }
    }

    fn on_frame_complete(&mut self, frame_id: u32) {
        self.push(|| {
            let mut m = vec![END_FRAME];
            m.extend_from_slice(&frame_id.to_le_bytes());
            m
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avc444_message_layout() {
        let r = [ExclusiveRectangle { left: 1, top: 2, right: 3, bottom: 4 }];
        let m = avc_message(&GpuAvc {
            surface_id: 7,
            codec_id: Codec1Type::Avc444v2,
            main: None,
            auxiliary: Some((&r, &[9, 8, 7])),
        });
        assert_eq!(m, [30, 7, 0, 2, 2, 1, 0, 1, 0, 2, 0, 3, 0, 4, 0, 3, 0, 0, 0, 9, 8, 7]);
    }

    #[test]
    fn yuv_message_layout() {
        // 6x4 picture, Y = row*10+col, U = 100+row*10+col, V = 200+...
        let y: Vec<u8> = (0..4).flat_map(|r| (0..6).map(move |c| r * 10 + c)).collect();
        let u: Vec<u8> = (0..2).flat_map(|r| (0..3).map(move |c| 100 + r * 10 + c)).collect();
        let v: Vec<u8> = u.iter().map(|x| x + 100).collect();
        let p = Yuv420Frame::new(6, 4, y, 6, u, v, 3).unwrap();
        let r = [ExclusiveRectangle { left: 3, top: 1, right: 5, bottom: 9 }];
        let m = yuv_message(4, &r, &p).unwrap();
        // area x 2..5 (w 3), y 0..4 (h 4); chroma 2 × 2 from column 1
        let head = [31, 4, 0, 1, 0, 3, 0, 1, 0, 5, 0, 4, 0, 2, 0, 0, 0, 3, 0, 4, 0];
        assert_eq!(&m[..head.len()], head);
        let body = &m[head.len()..];
        assert_eq!(&body[..12], [2, 3, 4, 12, 13, 14, 22, 23, 24, 32, 33, 34]);
        assert_eq!(&body[12..], [101, 102, 111, 112, 201, 202, 211, 212]);
        assert!(yuv_message(4, &[ExclusiveRectangle { left: 7, top: 0, right: 9, bottom: 2 }], &p).is_none());
    }

    #[test]
    fn batch_layout() {
        let mut b = Batch::default();
        assert!(b.take().is_none());
        b.push(&[22, 3, 0]);
        b.push(&[28, 1, 0]);
        assert_eq!(b.take().unwrap(), [33, 3, 0, 0, 0, 22, 3, 0, 3, 0, 0, 0, 28, 1, 0]);
        assert!(b.take().is_none());
    }

    #[test]
    fn records_only_while_on() {
        let q = GpuQueue::default();
        let mut h = GfxEvents(q.clone());
        h.on_surface_deleted(3);
        assert!(q.lock().unwrap().msgs.is_empty());
        q.lock().unwrap().on = true;
        h.on_surface_deleted(3);
        h.on_frame_complete(5);
        let msgs = q.lock().unwrap().take();
        assert_eq!(msgs, vec![vec![22, 3, 0], vec![END_FRAME, 5, 0, 0, 0]]);
    }
}
