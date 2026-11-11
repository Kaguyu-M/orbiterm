// Plays the PCM blocks of an RDP session (16-bit little-endian, interleaved) with Web Audio.

export class PcmPlayer {
  private ctx: AudioContext | null = null;
  private gain: GainNode | null = null;
  /** time (AudioContext clock) at which the next block starts */
  private next = 0;

  private context(): AudioContext {
    if (!this.ctx) {
      this.ctx = new AudioContext({ latencyHint: "interactive" });
      this.gain = this.ctx.createGain();
      this.gain.connect(this.ctx.destination);
    }
    return this.ctx;
  }

  /** Web views may start audio suspended until the user interacts with the page. */
  resume() {
    if (this.ctx?.state === "suspended") this.ctx.resume().catch(() => {});
  }

  play(channels: number, rate: number, pcm: DataView) {
    const ctx = this.context();
    this.resume();
    const frames = Math.floor(pcm.byteLength / 2 / channels);
    if (!frames || !rate) return;
    const buf = ctx.createBuffer(channels, frames, rate);
    for (let c = 0; c < channels; c++) {
      const out = buf.getChannelData(c);
      for (let i = 0; i < frames; i++) out[i] = pcm.getInt16((i * channels + c) * 2, true) / 32768;
    }
    const src = ctx.createBufferSource();
    src.buffer = buf;
    src.connect(this.gain!);
    const now = ctx.currentTime;
    // small safety margin; if we fell behind (or drifted too far ahead), restart from now
    if (this.next < now + 0.02 || this.next > now + 1) this.next = now + 0.06;
    src.start(this.next);
    this.next += buf.duration;
  }

  /** Volume asked by the server (0..0xFFFF per side; the average is applied). */
  volume(left: number, right: number) {
    this.context();
    this.gain!.gain.value = (left + right) / 2 / 0xffff;
  }

  close() {
    this.ctx?.close().catch(() => {});
    this.ctx = null;
    this.gain = null;
  }
}
