// Graphics pipeline composited by the tab: the backend forwards the surface commands of the RDP
// graphics pipeline (MS-RDPEGFX) with H.264 still compressed on Windows, decoded to YUV planes
// on Linux, where WebKitGTK has no usable H.264 decoder (see rdp_gpu.rs).
// As in the Windows client, the graphics card does the work: WebCodecs decodes H.264 with the
// hardware decoder, the picture goes to a WebGL texture without coming back to the processor,
// and surfaces, copies and the bitmap cache are textures composited with framebuffer blits.

import { codecString, prepareAccessUnit } from "./h264";

/** a texture with its framebuffer (blits read and write through framebuffers) */
interface Tex {
  tex: WebGLTexture;
  fb: WebGLFramebuffer;
  w: number;
  h: number;
}
interface Surf extends Tex {
  /** position on the output, target size (0 = surface size) */
  map: { x: number; y: number; tw: number; th: number } | null;
}
/** a command, run once the picture it waits for (decoder timestamp) is decoded */
interface Step {
  wait?: number;
  run: (frame?: VideoFrame) => void;
}

export interface GpuEvents {
  /** a frame of the server is composited (the argument of [GpuCompositor.endFrame]) */
  frameDone(arg: number): void;
  /** the tab cannot go on compositing (the backend takes over again) */
  failed(reason: string): void;
}

/** for the GUI tests: the compositor fed with hand-made messages */
(globalThis as { __orbitermGfxGpu?: unknown }).__orbitermGfxGpu = () =>
  GpuCompositor;

/**
 * Can this tab composite the pipeline? WebGL 2, and the WebCodecs decoder when the tab decodes
 * H.264 itself (Windows; on Linux the backend decodes it and sends the YUV planes).
 */
export function gpuCapable(
  gl: WebGLRenderingContext | WebGL2RenderingContext | null,
  decodes: boolean,
): gl is WebGL2RenderingContext {
  return (
    (!decodes || typeof VideoDecoder !== "undefined") &&
    typeof WebGL2RenderingContext !== "undefined" &&
    gl instanceof WebGL2RenderingContext
  );
}

// YUV 4:2:0 to RGB as MS-RDPEGFX 3.3.8.3.1 (the backend's own conversion, avc444.rs)
const YUV_VS = `#version 300 es
in vec2 p;
void main() { gl_Position = vec4(p, 0.0, 1.0); }`;
const YUV_FS = `#version 300 es
precision highp float;
precision highp int;
uniform sampler2D ty;
uniform sampler2D tu;
uniform sampler2D tv;
uniform ivec2 origin;
out vec4 o;
void main() {
  ivec2 p = ivec2(gl_FragCoord.xy) - origin;
  float y = floor(texelFetch(ty, p, 0).r * 255.0 + 0.5);
  float u = floor(texelFetch(tu, p / 2, 0).r * 255.0 + 0.5) - 128.0;
  float v = floor(texelFetch(tv, p / 2, 0).r * 255.0 + 0.5) - 128.0;
  float l = 256.0 * y;
  o = vec4(
    clamp(floor((l + 403.0 * v) / 256.0), 0.0, 255.0),
    clamp(floor((l - 48.0 * u - 120.0 * v) / 256.0), 0.0, 255.0),
    clamp(floor((l + 475.0 * u) / 256.0), 0.0, 255.0),
    255.0) / 255.0;
}`;

/** The YUV conversion program and its plane textures (made on the first use) */
interface Yuv {
  prog: WebGLProgram;
  vao: WebGLVertexArrayObject;
  origin: WebGLUniformLocation | null;
  planes: WebGLTexture[];
}

export class GpuCompositor {
  private surfaces = new Map<number, Surf>();
  private tiles = new Map<number, Tex>();
  private steps: Step[] = [];
  private head = 0;
  private frames = new Map<number, VideoFrame>();
  private decoder: VideoDecoder | null = null;
  private codec = "";
  private needKey = true;
  private seq = 0;
  private lastOut = 0;
  private frame: Tex;
  private tmp: Tex | null = null;
  private yuvGl: Yuv | null = null;
  private dead = false;
  /** statistics: H.264 pictures decoded, decode time (ms, total), dropped updates, bitmaps */
  readonly stats = { pictures: 0, decodeMs: 0, dropped: 0, bitmaps: 0 };
  private submitted = new Map<number, number>();
  // bindings last made (the context is shared with the 2D picture of surface.ts: forgotten
  // before each message)
  private rfb: WebGLFramebuffer | null | undefined;
  private dfb: WebGLFramebuffer | null | undefined;
  private rowLength = -1;

  constructor(
    private gl: WebGL2RenderingContext,
    private ev: GpuEvents,
  ) {
    this.frame = this.target(0, 0);
  }

  /** a texture (w×h, or sized later) with its framebuffer */
  private target(w: number, h: number): Tex {
    const gl = this.gl;
    const tex = gl.createTexture()!;
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    const fb = gl.createFramebuffer()!;
    if (w && h) {
      gl.texStorage2D(gl.TEXTURE_2D, 1, gl.RGBA8, w, h);
      this.attach(fb, tex);
    }
    return { tex, fb, w, h };
  }

  private attach(fb: WebGLFramebuffer, tex: WebGLTexture) {
    const gl = this.gl;
    gl.bindFramebuffer(gl.FRAMEBUFFER, fb);
    gl.framebufferTexture2D(
      gl.FRAMEBUFFER,
      gl.COLOR_ATTACHMENT0,
      gl.TEXTURE_2D,
      tex,
      0,
    );
    this.rfb = this.dfb = fb;
  }

  private drop(t: Tex) {
    this.gl.deleteFramebuffer(t.fb);
    this.gl.deleteTexture(t.tex);
    if (this.rfb === t.fb) this.rfb = undefined;
    if (this.dfb === t.fb) this.dfb = undefined;
  }

  private read(fb: WebGLFramebuffer | null) {
    if (this.rfb !== fb)
      this.gl.bindFramebuffer(this.gl.READ_FRAMEBUFFER, (this.rfb = fb));
  }

  private draw(fb: WebGLFramebuffer | null) {
    if (this.dfb !== fb)
      this.gl.bindFramebuffer(this.gl.DRAW_FRAMEBUFFER, (this.dfb = fb));
  }

  /** Leaves the context as the 2D picture expects it (default framebuffer). */
  private release() {
    this.read(null);
    this.draw(null);
    if (this.rowLength)
      this.gl.pixelStorei(this.gl.UNPACK_ROW_LENGTH, (this.rowLength = 0));
  }

  // ------------------------------------------------------------------ queue

  private push(step: Step) {
    if (this.dead) return;
    this.steps.push(step);
    this.pump();
  }

  private pump() {
    if (this.head >= this.steps.length || this.dead) return;
    // bindings made by others since the last run are unknown
    this.rfb = this.dfb = undefined;
    this.rowLength = -1;
    while (this.head < this.steps.length && !this.dead) {
      const s = this.steps[this.head];
      let frame: VideoFrame | undefined;
      if (s.wait !== undefined) {
        frame = this.frames.get(s.wait);
        // a picture the decoder skipped: a later one is already out
        if (!frame && s.wait > this.lastOut) break;
        this.frames.delete(s.wait);
      }
      this.head++;
      try {
        s.run(frame);
      } catch (e) {
        this.fail(`commande graphique : ${e}`);
      } finally {
        frame?.close();
      }
    }
    if (!this.dead) this.release();
    if (this.head > 1024 || this.head === this.steps.length) {
      this.steps = this.steps.slice(this.head);
      this.head = 0;
    }
  }

  private fail(reason: string) {
    if (this.dead) return;
    this.dispose();
    this.ev.failed(reason);
  }

  // ------------------------------------------------------------------ messages

  /** A message of the backend (types 20 to 33); false when it is not one of them. */
  message(buf: ArrayBuffer): boolean {
    const dv = new DataView(buf);
    if (dv.getUint8(0) !== 33) return this.command(dv);
    for (let o = 1; o + 4 <= buf.byteLength;) {
      const len = dv.getUint32(o, true);
      this.command(new DataView(buf, o + 4, len));
      o += 4 + len;
    }
    return true;
  }

  private command(dv: DataView): boolean {
    const t = dv.getUint8(0);
    const u = (o: number) => dv.getUint16(o, true);
    const rects = (o: number, n: number) =>
      Array.from({ length: n }, (_, i) => [
        u(o + i * 8),
        u(o + i * 8 + 2),
        u(o + i * 8 + 4),
        u(o + i * 8 + 6),
      ]);
    const points = (o: number, n: number) =>
      Array.from({ length: n }, (_, i) => [u(o + i * 4), u(o + i * 4 + 2)]);
    const bytes = (o: number, len = dv.byteLength - o) =>
      new Uint8Array(dv.buffer, dv.byteOffset + o, len);
    switch (t) {
      case 20:
        this.push({ run: () => this.resetSurfaces() });
        return true;
      case 21: {
        const [id, w, h] = [u(1), u(3), u(5)];
        this.push({ run: () => this.create(id, w, h) });
        return true;
      }
      case 22: {
        const id = u(1);
        this.push({ run: () => this.remove(id) });
        return true;
      }
      case 23: {
        const [id, x, y, tw, th] = [u(1), u(3), u(5), u(7), u(9)];
        this.push({
          run: () => {
            const s = this.surfaces.get(id);
            if (s) s.map = { x, y, tw, th };
          },
        });
        return true;
      }
      case 24: {
        const id = u(1);
        const [r, g, b] = [dv.getUint8(3), dv.getUint8(4), dv.getUint8(5)];
        const list = rects(8, u(6));
        this.push({ run: () => this.fill(id, r, g, b, list) });
        return true;
      }
      case 25: {
        const [src, dst] = [u(1), u(3)];
        const [l, tp, r, b] = [u(5), u(7), u(9), u(11)];
        const pts = points(15, u(13));
        this.push({
          run: () => this.copy(src, dst, l, tp, r - l, b - tp, pts),
        });
        return true;
      }
      case 26: {
        const [id, slot] = [u(1), u(3)];
        const [l, tp, r, b] = [u(5), u(7), u(9), u(11)];
        this.push({ run: () => this.toCache(id, slot, l, tp, r - l, b - tp) });
        return true;
      }
      case 27: {
        const [slot, id] = [u(1), u(3)];
        const pts = points(7, u(5));
        this.push({ run: () => this.fromCache(slot, id, pts) });
        return true;
      }
      case 28: {
        const slot = u(1);
        this.push({ run: () => this.evict(slot) });
        return true;
      }
      case 29: {
        const id = u(1);
        const [l, tp, r, b] = [u(3), u(5), u(7), u(9)];
        const px = bytes(11);
        this.push({ run: () => this.bitmap(id, l, tp, r - l, b - tp, px) });
        return true;
      }
      case 30: {
        const id = u(1);
        if (dv.getUint8(3) !== 0) {
          // AVC444: not composited by the tab yet
          this.fail("AVC444");
          return true;
        }
        const n = u(5);
        const list = rects(7, n);
        const lenAt = 7 + n * 8;
        this.avc(id, list, bytes(lenAt + 4, dv.getUint32(lenAt, true)));
        return true;
      }
      case 31: {
        const [id, n] = [u(1), u(3)];
        const list = rects(5, n);
        const at = 5 + n * 8;
        const [x, y, w, h] = [u(at), u(at + 2), u(at + 4), u(at + 6)];
        const cw = Math.ceil(w / 2),
          ch = Math.ceil(h / 2);
        const o = at + 8;
        if (dv.byteLength < o + w * h + 2 * cw * ch) return true;
        const planes = [
          bytes(o, w * h),
          bytes(o + w * h, cw * ch),
          bytes(o + w * h + cw * ch, cw * ch),
        ];
        this.push({ run: () => this.yuv(id, list, x, y, w, h, planes) });
        return true;
      }
    }
    return false;
  }

  /** End of a server frame: `frameDone(arg)` once everything before it is composited. */
  endFrame(arg: number) {
    this.push({ run: () => this.ev.frameDone(arg) });
  }

  // ------------------------------------------------------------------ H.264

  private configure(codec: string) {
    this.decoder?.close();
    this.frames.forEach((f) => f.close());
    this.frames.clear();
    this.codec = codec;
    this.decoder = new VideoDecoder({
      output: (f) => {
        if (this.dead) return f.close();
        const ts = f.timestamp;
        const at = this.submitted.get(ts);
        if (at !== undefined) {
          this.stats.decodeMs += performance.now() - at;
          this.submitted.delete(ts);
        }
        this.stats.pictures++;
        this.lastOut = Math.max(this.lastOut, ts);
        this.frames.set(ts, f);
        this.pump();
      },
      error: (e) => this.fail(`décodeur H.264 : ${e.message}`),
    });
    this.decoder.configure({
      codec,
      optimizeForLatency: true,
      hardwareAcceleration: "prefer-hardware",
    });
    this.needKey = true;
  }

  private avc(id: number, list: number[][], bytes: Uint8Array) {
    let au;
    try {
      au = prepareAccessUnit(bytes);
    } catch {
      this.stats.dropped++;
      return;
    }
    if (au.sps) {
      const c = codecString(au.sps);
      if (c !== this.codec || this.decoder?.state !== "configured") {
        try {
          this.configure(c);
        } catch (e) {
          this.fail(`décodeur H.264 : ${e}`);
          return;
        }
      }
    }
    if (!this.decoder || (this.needKey && !au.key)) {
      // no picture to start from: the server sends a key picture after a reset
      this.stats.dropped++;
      return;
    }
    this.needKey = false;
    const ts = ++this.seq;
    this.submitted.set(ts, performance.now());
    try {
      this.decoder.decode(
        new EncodedVideoChunk({
          type: au.key ? "key" : "delta",
          timestamp: ts,
          data: au.data,
        }),
      );
    } catch (e) {
      this.fail(`décodeur H.264 : ${e}`);
      return;
    }
    this.push({
      wait: ts,
      run: (frame) => frame && this.picture(id, list, frame),
    });
  }

  /** A decoded picture: its regions go to the surface, at the same place. */
  private picture(id: number, list: number[][], frame: VideoFrame) {
    const s = this.surfaces.get(id);
    if (!s) return;
    const gl = this.gl;
    gl.bindTexture(gl.TEXTURE_2D, this.frame.tex);
    gl.pixelStorei(gl.UNPACK_COLORSPACE_CONVERSION_WEBGL, gl.NONE);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, frame);
    gl.pixelStorei(
      gl.UNPACK_COLORSPACE_CONVERSION_WEBGL,
      gl.BROWSER_DEFAULT_WEBGL,
    );
    const fw = frame.displayWidth,
      fh = frame.displayHeight;
    if (fw !== this.frame.w || fh !== this.frame.h) {
      this.attach(this.frame.fb, this.frame.tex);
      this.frame.w = fw;
      this.frame.h = fh;
    }
    for (const [l, t, r, b] of list)
      this.blit(this.frame, l, t, r - l, b - t, s, l, t);
  }

  private yuvProgram(): Yuv {
    if (this.yuvGl) return this.yuvGl;
    const gl = this.gl;
    const sh = (type: number, src: string) => {
      const s = gl.createShader(type)!;
      gl.shaderSource(s, src);
      gl.compileShader(s);
      if (!gl.getShaderParameter(s, gl.COMPILE_STATUS))
        throw new Error(`shader : ${gl.getShaderInfoLog(s)}`);
      return s;
    };
    const prog = gl.createProgram()!;
    gl.attachShader(prog, sh(gl.VERTEX_SHADER, YUV_VS));
    gl.attachShader(prog, sh(gl.FRAGMENT_SHADER, YUV_FS));
    gl.bindAttribLocation(prog, 0, "p");
    gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS))
      throw new Error(`programme : ${gl.getProgramInfoLog(prog)}`);
    const vao = gl.createVertexArray()!;
    gl.bindVertexArray(vao);
    const buf = gl.createBuffer()!;
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.bufferData(
      gl.ARRAY_BUFFER,
      new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]),
      gl.STATIC_DRAW,
    );
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
    gl.bindVertexArray(null);
    gl.useProgram(prog);
    ["ty", "tu", "tv"].forEach((n, i) =>
      gl.uniform1i(gl.getUniformLocation(prog, n), i + 1),
    );
    const planes = [0, 1, 2].map(() => {
      const t = gl.createTexture()!;
      gl.bindTexture(gl.TEXTURE_2D, t);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
      return t;
    });
    this.yuvGl = {
      prog,
      vao,
      origin: gl.getUniformLocation(prog, "origin"),
      planes,
    };
    return this.yuvGl;
  }

  /** An AVC420 picture decoded by the backend: its YUV area converted into the surface's rectangles. */
  private yuv(
    id: number,
    list: number[][],
    x: number,
    y: number,
    w: number,
    h: number,
    planes: Uint8Array[],
  ) {
    const s = this.surfaces.get(id);
    if (!s || !w || !h) return;
    this.stats.pictures++;
    const gl = this.gl;
    const p = this.yuvProgram();
    if (this.rowLength)
      gl.pixelStorei(gl.UNPACK_ROW_LENGTH, (this.rowLength = 0));
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    const sizes = [
      [w, h],
      [Math.ceil(w / 2), Math.ceil(h / 2)],
      [Math.ceil(w / 2), Math.ceil(h / 2)],
    ];
    planes.forEach((data, i) => {
      gl.activeTexture(gl.TEXTURE1 + i);
      gl.bindTexture(gl.TEXTURE_2D, p.planes[i]);
      gl.texImage2D(
        gl.TEXTURE_2D,
        0,
        gl.R8,
        sizes[i][0],
        sizes[i][1],
        0,
        gl.RED,
        gl.UNSIGNED_BYTE,
        data,
      );
    });
    gl.activeTexture(gl.TEXTURE0);
    this.draw(s.fb);
    gl.useProgram(p.prog);
    gl.uniform2i(p.origin, x, y);
    gl.bindVertexArray(p.vao);
    gl.viewport(x, y, w, h);
    gl.enable(gl.SCISSOR_TEST);
    for (const [l, t, r, b] of list) {
      const cl = Math.max(l, x),
        ct = Math.max(t, y),
        cr = Math.min(r, x + w, s.w),
        cb = Math.min(b, y + h, s.h);
      if (cr <= cl || cb <= ct) continue;
      gl.scissor(cl, ct, cr - cl, cb - ct);
      gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    }
    gl.disable(gl.SCISSOR_TEST);
    gl.bindVertexArray(null);
  }

  // ------------------------------------------------------------------ surface commands

  /** Copies a w×h area of a texture to a surface, both clipped. */
  private blit(
    src: Tex,
    sx: number,
    sy: number,
    w: number,
    h: number,
    dst: Surf,
    dx: number,
    dy: number,
  ) {
    if (dx < 0) [w, sx, dx] = [w + dx, sx - dx, 0];
    if (dy < 0) [h, sy, dy] = [h + dy, sy - dy, 0];
    w = Math.min(w, src.w - sx, dst.w - dx);
    h = Math.min(h, src.h - sy, dst.h - dy);
    if (w <= 0 || h <= 0 || sx < 0 || sy < 0) return;
    const gl = this.gl;
    this.read(src.fb);
    this.draw(dst.fb);
    gl.blitFramebuffer(
      sx,
      sy,
      sx + w,
      sy + h,
      dx,
      dy,
      dx + w,
      dy + h,
      gl.COLOR_BUFFER_BIT,
      gl.NEAREST,
    );
  }

  private create(id: number, w: number, h: number) {
    this.remove(id);
    if (!w || !h) return;
    const s: Surf = { ...this.target(w, h), map: null };
    const gl = this.gl;
    gl.clearColor(0, 0, 0, 1);
    gl.clear(gl.COLOR_BUFFER_BIT); // the target is bound
    this.surfaces.set(id, s);
  }

  private remove(id: number) {
    const s = this.surfaces.get(id);
    if (!s) return;
    this.drop(s);
    this.surfaces.delete(id);
  }

  private resetSurfaces() {
    for (const id of [...this.surfaces.keys()]) this.remove(id);
  }

  private fill(id: number, r: number, g: number, b: number, list: number[][]) {
    const s = this.surfaces.get(id);
    if (!s) return;
    const gl = this.gl;
    this.draw(s.fb);
    gl.enable(gl.SCISSOR_TEST);
    gl.clearColor(r / 255, g / 255, b / 255, 1);
    for (const [l, t, rr, bb] of list) {
      if (rr <= l || bb <= t) continue;
      gl.scissor(l, t, rr - l, bb - t);
      gl.clear(gl.COLOR_BUFFER_BIT);
    }
    gl.disable(gl.SCISSOR_TEST);
  }

  /** Copies an area of a surface into a texture of its own (a cache tile, the scratch tile). */
  private grab(s: Surf, x: number, y: number, w: number, h: number, into: Tex) {
    const gl = this.gl;
    this.read(s.fb);
    gl.bindTexture(gl.TEXTURE_2D, into.tex);
    gl.copyTexSubImage2D(gl.TEXTURE_2D, 0, 0, 0, x, y, w, h);
  }

  private copy(
    srcId: number,
    dstId: number,
    x: number,
    y: number,
    w: number,
    h: number,
    pts: number[][],
  ) {
    const src = this.surfaces.get(srcId),
      dst = this.surfaces.get(dstId);
    if (!src || !dst) return;
    w = Math.min(w, src.w - x);
    h = Math.min(h, src.h - y);
    if (w <= 0 || h <= 0) return;
    if (src !== dst) {
      for (const [px, py] of pts) this.blit(src, x, y, w, h, dst, px, py);
      return;
    }
    // a scroll: the area is read once, before any write (as the internal compositor does)
    if (!this.tmp || this.tmp.w < w || this.tmp.h < h) {
      const tw = Math.max(w, this.tmp?.w ?? 0),
        th = Math.max(h, this.tmp?.h ?? 0);
      if (this.tmp) this.drop(this.tmp);
      this.tmp = this.target(tw, th);
    }
    this.grab(src, x, y, w, h, this.tmp);
    for (const [px, py] of pts) this.blit(this.tmp, 0, 0, w, h, dst, px, py);
  }

  private toCache(
    id: number,
    slot: number,
    x: number,
    y: number,
    w: number,
    h: number,
  ) {
    const s = this.surfaces.get(id);
    if (!s) return;
    w = Math.min(w, s.w - x);
    h = Math.min(h, s.h - y);
    if (w <= 0 || h <= 0) return;
    this.evict(slot);
    const tile = this.target(w, h);
    this.grab(s, x, y, w, h, tile);
    this.tiles.set(slot, tile);
  }

  private fromCache(slot: number, id: number, pts: number[][]) {
    const tile = this.tiles.get(slot),
      s = this.surfaces.get(id);
    if (!tile || !s) return;
    for (const [px, py] of pts)
      this.blit(tile, 0, 0, tile.w, tile.h, s, px, py);
  }

  private evict(slot: number) {
    const tile = this.tiles.get(slot);
    if (!tile) return;
    this.drop(tile);
    this.tiles.delete(slot);
  }

  private bitmap(
    id: number,
    x: number,
    y: number,
    w: number,
    h: number,
    px: Uint8Array,
  ) {
    const s = this.surfaces.get(id);
    if (!s || w <= 0 || h <= 0 || px.length < w * h * 4) return;
    this.stats.bitmaps++;
    const cw = Math.min(w, s.w - x),
      ch = Math.min(h, s.h - y);
    if (cw <= 0 || ch <= 0) return;
    const gl = this.gl;
    gl.bindTexture(gl.TEXTURE_2D, s.tex);
    const rl = cw === w ? 0 : w;
    if (this.rowLength !== rl)
      gl.pixelStorei(gl.UNPACK_ROW_LENGTH, (this.rowLength = rl));
    gl.texSubImage2D(
      gl.TEXTURE_2D,
      0,
      x,
      y,
      cw,
      ch,
      gl.RGBA,
      gl.UNSIGNED_BYTE,
      px,
    );
  }

  // ------------------------------------------------------------------ display

  /** Draws the surfaces mapped to the output on the canvas (w×h, the desktop size). */
  present(w: number, h: number) {
    if (this.dead) return;
    const gl = this.gl;
    gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, null);
    gl.viewport(0, 0, w, h);
    gl.clearColor(0, 0, 0, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    for (const s of this.surfaces.values()) {
      if (!s.map) continue;
      const tw = s.map.tw || s.w,
        th = s.map.th || s.h;
      gl.bindFramebuffer(gl.READ_FRAMEBUFFER, s.fb);
      // the textures hold the top row first, the canvas wants it last: flipped by the blit
      gl.blitFramebuffer(
        0,
        0,
        s.w,
        s.h,
        s.map.x,
        h - s.map.y,
        s.map.x + tw,
        h - s.map.y - th,
        gl.COLOR_BUFFER_BIT,
        tw === s.w && th === s.h ? gl.NEAREST : gl.LINEAR,
      );
    }
    gl.bindFramebuffer(gl.READ_FRAMEBUFFER, null);
    this.rfb = this.dfb = null;
  }

  dispose() {
    if (this.dead) return;
    this.dead = true;
    try {
      this.decoder?.close();
    } catch {
      /* already closed */
    }
    this.frames.forEach((f) => f.close());
    this.frames.clear();
    this.resetSurfaces();
    for (const slot of [...this.tiles.keys()]) this.evict(slot);
    if (this.tmp) this.drop(this.tmp);
    this.drop(this.frame);
    if (this.yuvGl) {
      this.yuvGl.planes.forEach((t) => this.gl.deleteTexture(t));
      this.gl.deleteVertexArray(this.yuvGl.vao);
      this.gl.deleteProgram(this.yuvGl.prog);
    }
    this.release();
  }
}
