// Minimal typings for the parts of noVNC used by VncView.svelte.
declare module "@novnc/novnc" {
  export default class RFB extends EventTarget {
    constructor(target: HTMLElement, url: string, options?: { credentials?: { password?: string; username?: string }; wsProtocols?: string[]; shared?: boolean });
    scaleViewport: boolean;
    resizeSession: boolean;
    clipViewport: boolean;
    viewOnly: boolean;
    focusOnClick: boolean;
    qualityLevel: number;
    compressionLevel: number;
    background: string;
    readonly capabilities: { power: boolean };
    sendCredentials(credentials: { password?: string; username?: string }): void;
    sendCtrlAltDel(): void;
    sendKey(keysym: number, code: string | null, down?: boolean): void;
    clipboardPasteFrom(text: string): void;
    focus(): void;
    blur(): void;
    disconnect(): void;
    toDataURL(type?: string): string;
  }
}
