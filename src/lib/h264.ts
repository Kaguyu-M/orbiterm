// H.264 Annex B helpers for the tab's decoder (WebCodecs).
//
// The RDP graphics pipeline uses BT.709 full-range colours (MS-RDPEGFX 3.3.8.3.1), but the
// browser converts a decoded picture to RGB with the colour description of its sequence
// parameter set (SPS), and the configuration's colorSpace does not override it. So the SPS is
// rewritten to state BT.709 full range: the conversion done by the graphics card then matches.

/** NAL units of an Annex B stream (start codes removed). */
export function nalUnits(data: Uint8Array): Uint8Array[] {
  const out: Uint8Array[] = [];
  let start = -1;
  let i = 0;
  while (i + 2 < data.length) {
    if (data[i] === 0 && data[i + 1] === 0 && (data[i + 2] === 1 || (data[i + 2] === 0 && data[i + 3] === 1))) {
      const sc = data[i + 2] === 1 ? 3 : 4;
      if (start >= 0) out.push(data.subarray(start, i));
      i += sc;
      start = i;
    } else i++;
  }
  if (start >= 0) out.push(data.subarray(start));
  // a zero byte before a 4-byte start code belongs to the start code, not to the unit
  return out.map((n) => {
    let e = n.length;
    while (e > 1 && n[e - 1] === 0) e--;
    return n.subarray(0, e);
  });
}

export const nalType = (n: Uint8Array) => n[0] & 0x1f;

function unescape(n: Uint8Array): Uint8Array {
  const out = new Uint8Array(n.length);
  let o = 0, zeros = 0;
  for (let i = 0; i < n.length; i++) {
    if (zeros >= 2 && n[i] === 3) {
      zeros = 0;
      continue;
    }
    zeros = n[i] === 0 ? zeros + 1 : 0;
    out[o++] = n[i];
  }
  return out.subarray(0, o);
}

function escape(n: Uint8Array): Uint8Array {
  const out: number[] = [];
  let zeros = 0;
  for (const b of n) {
    if (zeros >= 2 && b <= 3) {
      out.push(3);
      zeros = 0;
    }
    out.push(b);
    zeros = b === 0 ? zeros + 1 : 0;
  }
  return Uint8Array.from(out);
}

class BitReader {
  pos = 0;
  constructor(readonly b: Uint8Array) {}
  get left() {
    return this.b.length * 8 - this.pos;
  }
  bit(): number {
    if (this.pos >= this.b.length * 8) throw new Error("SPS tronqué");
    const v = (this.b[this.pos >> 3] >> (7 - (this.pos & 7))) & 1;
    this.pos++;
    return v;
  }
  bits(n: number): number {
    let v = 0;
    for (let i = 0; i < n; i++) v = v * 2 + this.bit();
    return v;
  }
  ue(): number {
    let z = 0;
    while (this.bit() === 0) if (++z > 31) throw new Error("SPS invalide");
    return z ? 2 ** z - 1 + this.bits(z) : 0;
  }
  se(): number {
    const k = this.ue();
    return k & 1 ? (k + 1) / 2 : -(k / 2);
  }
}

class BitWriter {
  private out: number[] = [];
  private cur = 0;
  private n = 0;
  bit(v: number) {
    this.cur = (this.cur << 1) | (v & 1);
    if (++this.n === 8) {
      this.out.push(this.cur);
      this.cur = 0;
      this.n = 0;
    }
  }
  bits(v: number, n: number) {
    for (let i = n - 1; i >= 0; i--) this.bit(Math.floor(v / 2 ** i) & 1);
  }
  ue(v: number) {
    const x = v + 1;
    const len = Math.floor(Math.log2(x)) + 1;
    for (let i = 0; i < len - 1; i++) this.bit(0);
    this.bits(x, len);
  }
  copy(r: BitReader, from: number, to: number) {
    for (let p = from; p < to; p++) this.bit((r.b[p >> 3] >> (7 - (p & 7))) & 1);
  }
  /** rbsp_trailing_bits, then the bytes */
  finish(): Uint8Array {
    this.bit(1);
    while (this.n) this.bit(0);
    return Uint8Array.from(this.out);
  }
}

const HIGH_PROFILES = new Set([100, 110, 122, 244, 44, 83, 86, 118, 128, 138, 139, 134, 135]);

/** "avc1.PPCCLL" codec string of an SPS unit (header byte included). */
export function codecString(sps: Uint8Array): string {
  const h = (v: number) => v.toString(16).padStart(2, "0");
  return `avc1.${h(sps[1])}${h(sps[2])}${h(sps[3])}`;
}

/** The SPS unit (header byte included) stating BT.709 full-range colours, or the same unit
 * when it already does. Throws on a stream it cannot read. */
export function fullRangeSps(sps: Uint8Array): Uint8Array {
  const r = new BitReader(unescape(sps.subarray(1)));
  const profile = r.bits(8);
  r.bits(16); // constraint flags, level
  r.ue(); // seq_parameter_set_id
  let maxRefFrames = 1;
  if (HIGH_PROFILES.has(profile)) {
    const chroma = r.ue();
    if (chroma === 3) r.bit();
    r.ue(); // bit_depth_luma
    r.ue(); // bit_depth_chroma
    r.bit(); // qpprime_y_zero_transform_bypass
    if (r.bit()) {
      for (let i = 0; i < (chroma !== 3 ? 8 : 12); i++) {
        if (!r.bit()) continue;
        let last = 8, next = 8;
        for (let j = 0; j < (i < 6 ? 16 : 64); j++) {
          if (next !== 0) next = (last + r.se() + 256) % 256;
          last = next === 0 ? last : next;
        }
      }
    }
  }
  r.ue(); // log2_max_frame_num
  const pocType = r.ue();
  if (pocType === 0) r.ue();
  else if (pocType === 1) {
    r.bit();
    r.se();
    r.se();
    const n = r.ue();
    for (let i = 0; i < n; i++) r.se();
  }
  maxRefFrames = r.ue();
  r.bit(); // gaps_in_frame_num
  r.ue(); // width in macroblocks
  r.ue(); // height in map units
  if (!r.bit()) r.bit(); // frame_mbs_only, mb_adaptive
  r.bit(); // direct_8x8
  if (r.bit()) for (let i = 0; i < 4; i++) r.ue(); // cropping
  const vuiAt = r.pos;
  // end of the payload: the stop bit
  let stop = r.b.length * 8 - 1;
  while (stop > 0 && !((r.b[stop >> 3] >> (7 - (stop & 7))) & 1)) stop--;

  const w = new BitWriter();
  if (!r.bit()) {
    // no VUI: colours plus "no reordering", so each picture is returned as soon as decoded
    w.copy(r, 0, vuiAt);
    w.bit(1);
    w.bits(0, 2); // aspect ratio, overscan
    w.bits(1, 1);
    w.bits(5, 3); // video_format: unspecified
    w.bits(1, 1); // full range
    w.bits(1, 1);
    w.bits(1, 8);
    w.bits(1, 8);
    w.bits(1, 8); // BT.709 primaries, transfer, matrix
    w.bits(0, 5); // chroma location, timing, NAL HRD, VCL HRD, pic_struct
    w.bit(1); // bitstream_restriction
    w.bit(1);
    w.ue(0);
    w.ue(0);
    w.ue(15);
    w.ue(15);
    w.ue(0); // max_num_reorder_frames
    w.ue(Math.max(1, maxRefFrames)); // max_dec_frame_buffering
  } else {
    if (r.bit()) {
      if (r.bits(8) === 255) r.bits(32);
    }
    if (r.bit()) r.bit();
    const signalAt = r.pos;
    let format = 5;
    if (r.bit()) {
      format = r.bits(3);
      const full = r.bit();
      if (r.bit()) {
        const [p, t, m] = [r.bits(8), r.bits(8), r.bits(8)];
        if (full && p === 1 && t === 1 && m === 1) return sps;
      }
    }
    w.copy(r, 0, signalAt);
    w.bit(1);
    w.bits(format, 3);
    w.bit(1);
    w.bit(1);
    w.bits(1, 8);
    w.bits(1, 8);
    w.bits(1, 8);
    w.copy(r, r.pos, stop);
  }
  const body = escape(w.finish());
  const out = new Uint8Array(1 + body.length);
  out[0] = sps[0];
  out.set(body, 1);
  return out;
}

/** The stream with its SPS rewritten (see [fullRangeSps]); `sps`: the SPS found, if any;
 * `key`: it holds an IDR picture. */
export function prepareAccessUnit(data: Uint8Array): { data: Uint8Array; sps: Uint8Array | null; key: boolean } {
  const units = nalUnits(data);
  let sps: Uint8Array | null = null;
  let key = false;
  let changed = false;
  const parts = units.map((n) => {
    const t = nalType(n);
    if (t === 5) key = true;
    if (t !== 7) return n;
    const s = fullRangeSps(n);
    sps = s;
    if (s !== n) changed = true;
    return s;
  });
  if (!changed) return { data, sps, key };
  const size = parts.reduce((a, p) => a + 4 + p.length, 0);
  const out = new Uint8Array(size);
  let o = 0;
  for (const p of parts) {
    out.set([0, 0, 0, 1], o);
    out.set(p, o + 4);
    o += 4 + p.length;
  }
  return { data: out, sps, key };
}
