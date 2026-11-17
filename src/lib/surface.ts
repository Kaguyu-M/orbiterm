// Remote desktop surface: draws the changed areas sent by the backend.
// WebGL keeps the desktop in a GPU texture: each area is uploaded with texSubImage2D and
// the texture is drawn once per frame. This is much cheaper than putImageData on WebKitGTK
// (Linux), where a 2D canvas is redrawn on the CPU. The 2D canvas stays as a fallback.

export interface Surface {
  readonly kind: "webgl" | "2d";
  /** WebGL 2 context of the canvas, shared with the compositor of the graphics pipeline */
  readonly gl2?: WebGL2RenderingContext;
  /** new desktop size (clears the picture) */
  resize(w: number, h: number): void;
  /** RGBA pixels of an area, rows of w * 4 bytes */
  put(x: number, y: number, w: number, h: number, rgba: Uint8Array): void;
  /** end of a frame: show what was put since the last call */
  present(): void;
}

class Canvas2D implements Surface {
  readonly kind = "2d";
  private ctx: CanvasRenderingContext2D | null = null;
  constructor(private canvas: HTMLCanvasElement) {}
  resize(w: number, h: number) {
    this.canvas.width = w;
    this.canvas.height = h;
    this.ctx = this.canvas.getContext("2d", { alpha: false });
  }
  put(x: number, y: number, w: number, h: number, rgba: Uint8Array) {
    // WebSocket frames are plain ArrayBuffers
    const px = new Uint8ClampedArray(
      rgba.buffer as ArrayBuffer,
      rgba.byteOffset,
      w * h * 4,
    );
    this.ctx?.putImageData(new ImageData(px, w, h), x, y);
  }
  present() {}
}

const VS = `attribute vec2 p;
varying vec2 uv;
void main() {
  uv = vec2((p.x + 1.0) * 0.5, (1.0 - p.y) * 0.5);
  gl_Position = vec4(p, 0.0, 1.0);
}`;
const FS = `precision mediump float;
varying vec2 uv;
uniform sampler2D t;
void main() { gl_FragColor = vec4(texture2D(t, uv).rgb, 1.0); }`;

class WebGL implements Surface {
  readonly kind = "webgl";
  private tex: WebGLTexture | null = null;
  private prog: WebGLProgram | null = null;
  private buf: WebGLBuffer | null = null;
  private loc = 0;
  private dirty = false;
  private lost = false;
  private size = { w: 0, h: 0 };

  private constructor(
    private canvas: HTMLCanvasElement,
    private gl: WebGLRenderingContext,
    onRestore: () => void,
  ) {
    // the GPU may drop the context (driver reset, sleep): the browser gives it back
    // empty, so the texture is rebuilt and the caller asks for the whole desktop again
    canvas.addEventListener("webglcontextlost", (e) => {
      e.preventDefault();
      this.lost = true;
    });
    canvas.addEventListener("webglcontextrestored", () => {
      if (!this.init()) return;
      this.lost = false;
      if (this.size.w) this.resize(this.size.w, this.size.h);
      onRestore();
    });
  }

  static create(
    canvas: HTMLCanvasElement,
    onRestore: () => void,
  ): WebGL | null {
    const opts: WebGLContextAttributes = {
      alpha: false,
      antialias: false,
      depth: false,
      stencil: false,
      // the whole texture is drawn on every frame: nothing to keep between frames
      preserveDrawingBuffer: false,
      powerPreference: "high-performance",
    };
    // WebGL 2 when available: the tab can then composite the graphics pipeline (gfxgpu.ts)
    const gl = (canvas.getContext("webgl2", opts) ??
      canvas.getContext("webgl", opts) ??
      canvas.getContext(
        "experimental-webgl",
        opts,
      )) as WebGLRenderingContext | null;
    if (!gl) return null;
    const s = new WebGL(canvas, gl, onRestore);
    return s.init() ? s : null;
  }

  get gl2(): WebGL2RenderingContext | undefined {
    return typeof WebGL2RenderingContext !== "undefined" &&
      this.gl instanceof WebGL2RenderingContext
      ? this.gl
      : undefined;
  }

  /** Binds what drawing needs: the compositor of the graphics pipeline shares the context. */
  private bind() {
    const gl = this.gl;
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.useProgram(this.prog);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.buf);
    gl.enableVertexAttribArray(this.loc);
    gl.vertexAttribPointer(this.loc, 2, gl.FLOAT, false, 0, 0);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.tex);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
  }

  private init(): boolean {
    const gl = this.gl;
    const shader = (type: number, src: string) => {
      const sh = gl.createShader(type);
      if (!sh) return null;
      gl.shaderSource(sh, src);
      gl.compileShader(sh);
      return gl.getShaderParameter(sh, gl.COMPILE_STATUS) ? sh : null;
    };
    const vs = shader(gl.VERTEX_SHADER, VS),
      fs = shader(gl.FRAGMENT_SHADER, FS);
    const prog = gl.createProgram();
    if (!vs || !fs || !prog) return false;
    gl.attachShader(prog, vs);
    gl.attachShader(prog, fs);
    gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) return false;
    gl.useProgram(prog);
    this.prog = prog;
    this.buf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, this.buf);
    gl.bufferData(
      gl.ARRAY_BUFFER,
      new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]),
      gl.STATIC_DRAW,
    );
    const loc = gl.getAttribLocation(prog, "p");
    this.loc = loc;
    gl.enableVertexAttribArray(loc);
    gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
    gl.uniform1i(gl.getUniformLocation(prog, "t"), 0);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    this.tex = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, this.tex);
    // the canvas is the size of the desktop: one texel per pixel, no filtering needed
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    return gl.getError() === gl.NO_ERROR;
  }

  resize(w: number, h: number) {
    const gl = this.gl;
    this.size = { w, h };
    this.canvas.width = w;
    this.canvas.height = h;
    if (this.lost) return;
    this.bind();
    gl.viewport(0, 0, w, h);
    gl.texImage2D(
      gl.TEXTURE_2D,
      0,
      gl.RGBA,
      w,
      h,
      0,
      gl.RGBA,
      gl.UNSIGNED_BYTE,
      null,
    );
    this.dirty = true;
  }

  put(x: number, y: number, w: number, h: number, rgba: Uint8Array) {
    if (this.lost) return;
    const gl = this.gl;
    this.bind();
    gl.texSubImage2D(
      gl.TEXTURE_2D,
      0,
      x,
      y,
      w,
      h,
      gl.RGBA,
      gl.UNSIGNED_BYTE,
      rgba.subarray(0, w * h * 4),
    );
    this.dirty = true;
  }

  present() {
    if (!this.dirty || this.lost) return;
    this.dirty = false;
    this.bind();
    this.gl.viewport(0, 0, this.size.w, this.size.h);
    this.gl.drawArrays(this.gl.TRIANGLE_STRIP, 0, 4);
  }
}

/** WebGL when available (and not disabled), else the 2D canvas. `onRestore`: the GPU
 * context came back after a loss, the whole desktop must be sent again. */
export function createSurface(
  canvas: HTMLCanvasElement,
  allowGpu: boolean,
  onRestore: () => void,
): Surface {
  if (allowGpu) {
    try {
      const s = WebGL.create(canvas, onRestore);
      if (s) return s;
    } catch {
      /* fall back to 2D */
    }
  }
  return new Canvas2D(canvas);
}
