<script lang="ts" module>
  // Original icon set (drawn for Orbiterm, 24x24 grid). Colourful like a classic
  // Windows toolbar, but not copied from any other product.
  const S = 'stroke-linecap="round" stroke-linejoin="round"';
  export const ICONS: Record<string, string> = {
    session: `<rect x="2" y="4" width="17" height="12" rx="1.5" fill="#2f6db5"/><rect x="4" y="6" width="13" height="8" fill="#cfe5ff"/><rect x="7" y="17" width="7" height="2" fill="#5b6470"/><circle cx="18.5" cy="17.5" r="4.5" fill="#2e9d4a"/><path d="M18.5 15v5M16 17.5h5" stroke="#fff" stroke-width="1.8" ${S}/>`,
    servers: `<rect x="3" y="3" width="18" height="5" rx="1" fill="#55606e"/><rect x="3" y="9.5" width="18" height="5" rx="1" fill="#6b7686"/><rect x="3" y="16" width="18" height="5" rx="1" fill="#55606e"/><circle cx="6" cy="5.5" r="1" fill="#5fe08a"/><circle cx="6" cy="12" r="1" fill="#5fe08a"/><circle cx="6" cy="18.5" r="1" fill="#f0c040"/><path d="M10 5.5h8M10 12h8M10 18.5h8" stroke="#c9d1db" stroke-width="1.2"/>`,
    tools: `<path d="M14.5 3.5a4.5 4.5 0 0 0-4.2 6.1L3.6 16.3a1.8 1.8 0 0 0 2.6 2.6l6.7-6.7a4.5 4.5 0 0 0 6.1-4.2l-2.6 2.6-2.4-.6-.6-2.4z" fill="#8a96a8" stroke="#4f5967" stroke-width="1"/><path d="M15 15l5 5" stroke="#d9822b" stroke-width="3" ${S}/>`,
    sessions: `<path d="M12 2.5l2.8 5.8 6.2.9-4.5 4.4 1.1 6.2L12 16.9l-5.6 2.9 1.1-6.2L3 9.2l6.2-.9z" fill="#f2b632" stroke="#c98a10" stroke-width="1" ${S}/>`,
    view: `<rect x="2.5" y="4" width="19" height="16" rx="1.5" fill="#fff" stroke="#47607e" stroke-width="1.4"/><rect x="2.5" y="4" width="19" height="4" fill="#47607e"/><rect x="4.5" y="10" width="6" height="8" fill="#9cc3ec"/><path d="M12.5 11h7M12.5 14h7M12.5 17h5" stroke="#8c99aa" stroke-width="1.3"/>`,
    split: `<rect x="2.5" y="3.5" width="19" height="17" rx="1.5" fill="#1d2027" stroke="#47607e" stroke-width="1.4"/><path d="M12 3.5v17M2.5 12h19" stroke="#8fb8e8" stroke-width="1.4"/><path d="M4.5 6.5l1.5 1-1.5 1M14 15.5l1.5 1-1.5 1" stroke="#6fe08f" stroke-width="1.1" ${S}/>`,
    multiexec: `<rect x="2" y="3" width="9" height="7" rx="1" fill="#1d2027"/><rect x="13" y="3" width="9" height="7" rx="1" fill="#1d2027"/><rect x="7.5" y="14" width="9" height="7" rx="1" fill="#1d2027"/><path d="M4 6.5h3M15 6.5h3M9.5 17.5h3" stroke="#6fe08f" stroke-width="1.4"/><path d="M12 10.5v3M8 10.5l2 3M16 10.5l-2 3" stroke="#d9822b" stroke-width="1.4" ${S}/>`,
    tunnel: `<path d="M2 21V12a10 9 0 0 1 20 0v9z" fill="#8a6a46"/><path d="M6 21v-8a6 5.5 0 0 1 12 0v8z" fill="#1d2027"/><path d="M12 21v-6" stroke="#f0c040" stroke-width="1.5" stroke-dasharray="1.5 1.5"/>`,
    packages: `<path d="M12 2.5l9 4.5v10l-9 4.5-9-4.5V7z" fill="#d6a35c" stroke="#9c6f2f" stroke-width="1"/><path d="M3 7l9 4.5L21 7M12 11.5v10" stroke="#9c6f2f" stroke-width="1"/><path d="M7.5 4.8l9 4.5" stroke="#f3dcb4" stroke-width="1.5"/>`,
    settings: `<path d="M12 8.5a3.5 3.5 0 1 0 0 7 3.5 3.5 0 0 0 0-7z" fill="#fff"/><path d="M10.4 2.5h3.2l.5 2.6 1.9.8 2.2-1.5 2.3 2.3-1.5 2.2.8 1.9 2.6.5v3.2l-2.6.5-.8 1.9 1.5 2.2-2.3 2.3-2.2-1.5-1.9.8-.5 2.6h-3.2l-.5-2.6-1.9-.8-2.2 1.5-2.3-2.3 1.5-2.2-.8-1.9-2.6-.5v-3.2l2.6-.5.8-1.9-1.5-2.2 2.3-2.3 2.2 1.5 1.9-.8z" fill="#7b8797" stroke="#4f5967" stroke-width=".8"/><circle cx="12" cy="12" r="3" fill="#e9edf2"/>`,
    help: `<circle cx="12" cy="12" r="9.5" fill="#2f6db5"/><path d="M9.3 9.3a2.8 2.8 0 1 1 3.9 2.6c-.8.4-1.2 1-1.2 1.9v.6" stroke="#fff" stroke-width="2" fill="none" ${S}/><circle cx="12" cy="17.6" r="1.2" fill="#fff"/>`,
    xserver: `<circle cx="12" cy="12" r="9.5" fill="#1f8f5a"/><path d="M8 7.5l8 9M16 7.5l-8 9" stroke="#fff" stroke-width="2.4" ${S}/>`,
    xserver_off: `<circle cx="12" cy="12" r="9.5" fill="#9aa3ad"/><path d="M8 7.5l8 9M16 7.5l-8 9" stroke="#fff" stroke-width="2.4" ${S}/>`,
    exit: `<path d="M7.5 5.8a8 8 0 1 0 9 0" stroke="#c0392b" stroke-width="2.4" fill="none" ${S}/><path d="M12 2.5v9" stroke="#c0392b" stroke-width="2.4" ${S}/>`,
    terminal: `<rect x="2" y="3.5" width="20" height="17" rx="1.5" fill="#1d2027"/><rect x="2" y="3.5" width="20" height="3" rx="1.5" fill="#4b5563"/><path d="M5.5 10l3 2.5-3 2.5M10.5 16h6" stroke="#6fe08f" stroke-width="1.8" fill="none" ${S}/>`,
    ssh: `<rect x="2" y="3.5" width="20" height="17" rx="1.5" fill="#1d2027"/><path d="M5 9l2.5 2-2.5 2" stroke="#6fe08f" stroke-width="1.6" fill="none" ${S}/><rect x="11.5" y="11.5" width="8" height="6.5" rx="1" fill="#f2b632"/><path d="M13.3 11.5V10a2.2 2.2 0 0 1 4.4 0v1.5" stroke="#f2b632" stroke-width="1.5" fill="none"/>`,
    telnet: `<rect x="2" y="3.5" width="20" height="17" rx="1.5" fill="#1d2027"/><path d="M5 9l2.5 2-2.5 2" stroke="#6fe08f" stroke-width="1.6" fill="none" ${S}/><path d="M12 14h3l1.5-2.5 2 5 1.5-2.5" stroke="#7fb6f0" stroke-width="1.5" fill="none" ${S}/>`,
    rlogin: `<rect x="2" y="3.5" width="20" height="17" rx="1.5" fill="#1d2027"/><path d="M5 9l2.5 2-2.5 2" stroke="#6fe08f" stroke-width="1.6" fill="none" ${S}/><path d="M12 17v-6h3a1.7 1.7 0 0 1 0 3.4h-3M14.5 14.4l3 2.6" stroke="#e8a85c" stroke-width="1.5" fill="none" ${S}/>`,
    rdp: `<rect x="2" y="3.5" width="20" height="13" rx="1.5" fill="#2f6db5"/><rect x="4" y="5.5" width="16" height="9" fill="#bfe0ff"/><rect x="9" y="17.5" width="6" height="2" fill="#5b6470"/><rect x="6.5" y="19.5" width="11" height="1.5" rx=".7" fill="#5b6470"/><path d="M10 7.5l5 4-2.2.3 1.3 2.4-1 .5-1.3-2.4-1.8 1.4z" fill="#1d2027"/>`,
    vnc: `<rect x="2" y="3.5" width="20" height="13" rx="1.5" fill="#6b4fb5"/><rect x="4" y="5.5" width="16" height="9" fill="#e4dbff"/><rect x="9" y="17.5" width="6" height="2" fill="#5b6470"/><rect x="6.5" y="19.5" width="11" height="1.5" rx=".7" fill="#5b6470"/><path d="M6.5 10s2.2-3 5.5-3 5.5 3 5.5 3-2.2 3-5.5 3-5.5-3-5.5-3z" fill="#fff" stroke="#6b4fb5" stroke-width="1"/><circle cx="12" cy="10" r="1.6" fill="#6b4fb5"/>`,
    ftp: `<path d="M2.5 6.5a1.5 1.5 0 0 1 1.5-1.5h5l2 2h9a1.5 1.5 0 0 1 1.5 1.5v10a1.5 1.5 0 0 1-1.5 1.5H4a1.5 1.5 0 0 1-1.5-1.5z" fill="#f0c040" stroke="#c99a1a" stroke-width="1"/><path d="M9 11v6M7 13l2-2 2 2M15 17v-6M13 15l2 2 2-2" stroke="#2f6db5" stroke-width="1.6" fill="none" ${S}/>`,
    sftp: `<path d="M2.5 6.5a1.5 1.5 0 0 1 1.5-1.5h5l2 2h9a1.5 1.5 0 0 1 1.5 1.5v10a1.5 1.5 0 0 1-1.5 1.5H4a1.5 1.5 0 0 1-1.5-1.5z" fill="#f0c040" stroke="#c99a1a" stroke-width="1"/><rect x="9" y="12" width="7" height="5.5" rx="1" fill="#2f6db5"/><path d="M10.5 12v-1.3a2 2 0 0 1 4 0V12" stroke="#2f6db5" stroke-width="1.4" fill="none"/>`,
    serial: `<path d="M3 7h18l-2.2 10H5.2z" fill="#7b8797" stroke="#4f5967" stroke-width="1"/><g fill="#f2b632"><circle cx="7" cy="10" r="1"/><circle cx="10" cy="10" r="1"/><circle cx="13" cy="10" r="1"/><circle cx="16" cy="10" r="1"/><circle cx="8.5" cy="13.5" r="1"/><circle cx="11.5" cy="13.5" r="1"/><circle cx="14.5" cy="13.5" r="1"/></g>`,
    shell: `<rect x="2" y="3.5" width="20" height="17" rx="1.5" fill="#173b2a"/><path d="M5.5 9l3.5 3-3.5 3M11 16h7" stroke="#6fe08f" stroke-width="2" fill="none" ${S}/>`,
    wsl: `<rect x="2" y="3.5" width="20" height="17" rx="1.5" fill="#3a2a17"/><path d="M5.5 9l3 2.5-3 2.5" stroke="#f2b632" stroke-width="1.8" fill="none" ${S}/><ellipse cx="16" cy="13" rx="3" ry="4" fill="#f2f2f2"/><ellipse cx="16" cy="14" rx="2" ry="2.6" fill="#fff"/><circle cx="15" cy="11" r=".6" fill="#1d2027"/><circle cx="17" cy="11" r=".6" fill="#1d2027"/><path d="M15.3 12.2h1.4l-.7.8z" fill="#f2b632"/>`,
    mosh: `<rect x="2" y="3.5" width="20" height="17" rx="1.5" fill="#1d2027"/><path d="M13.5 5.5l-5 7.5h4l-1.5 6 5.5-8h-4z" fill="#f2b632"/>`,
    browser: `<circle cx="12" cy="12" r="9.5" fill="#2f8fd0"/><path d="M2.5 12h19M12 2.5c3 3 3 16 0 19M12 2.5c-3 3-3 16 0 19M4.5 7h15M4.5 17h15" stroke="#d8eeff" stroke-width="1.1" fill="none"/>`,
    folder: `<path d="M2.5 6.5a1.5 1.5 0 0 1 1.5-1.5h5l2 2h9a1.5 1.5 0 0 1 1.5 1.5v10a1.5 1.5 0 0 1-1.5 1.5H4a1.5 1.5 0 0 1-1.5-1.5z" fill="#f0c040" stroke="#c99a1a" stroke-width="1"/>`,
    folder_open: `<path d="M2.5 6.5A1.5 1.5 0 0 1 4 5h5l2 2h8a1.5 1.5 0 0 1 1.5 1.5V10H6.5L3 19z" fill="#d9a92c"/><path d="M6 10h16l-3.5 9.5H3z" fill="#f5cd55" stroke="#c99a1a" stroke-width="1" ${S}/>`,
    file: `<path d="M5.5 2.5h9l4 4v15h-13z" fill="#fff" stroke="#7b8797" stroke-width="1.2" ${S}/><path d="M14.5 2.5v4h4" fill="none" stroke="#7b8797" stroke-width="1.2"/><path d="M8 11h8M8 14h8M8 17h5" stroke="#aab4c0" stroke-width="1.1"/>`,
    link: `<path d="M10 14l4-4M8.5 11.5l-2 2a2.8 2.8 0 0 0 4 4l2-2M15.5 12.5l2-2a2.8 2.8 0 0 0-4-4l-2 2" stroke="#2f6db5" stroke-width="1.8" fill="none" ${S}/>`,
    home: `<path d="M3 11.5L12 4l9 7.5" stroke="#47607e" stroke-width="1.8" fill="none" ${S}/><path d="M5.5 10v10h13V10" fill="#fff" stroke="#47607e" stroke-width="1.5"/><rect x="10" y="14" width="4" height="6" fill="#d9822b"/>`,
    plus: `<path d="M12 5v14M5 12h14" stroke="currentColor" stroke-width="2" ${S}/>`,
    back: `<path d="M15 5l-7 7 7 7" stroke="currentColor" stroke-width="2.2" fill="none" ${S}/>`,
    forward: `<path d="M9 5l7 7-7 7" stroke="currentColor" stroke-width="2.2" fill="none" ${S}/>`,
    close: `<path d="M6.5 6.5l11 11M17.5 6.5l-11 11" stroke="currentColor" stroke-width="2" ${S}/>`,
    refresh: `<path d="M19 12a7 7 0 1 1-2.1-5" stroke="#2e9d4a" stroke-width="2" fill="none" ${S}/><path d="M17.5 3.5v4h-4" stroke="#2e9d4a" stroke-width="2" fill="none" ${S}/>`,
    up: `<path d="M12 19V6M6.5 11.5L12 6l5.5 5.5" stroke="#2f6db5" stroke-width="2" fill="none" ${S}/>`,
    upload: `<path d="M12 16V5M7.5 9.5L12 5l4.5 4.5" stroke="#2e9d4a" stroke-width="2" fill="none" ${S}/><path d="M4 15v4.5h16V15" stroke="#5b6470" stroke-width="1.8" fill="none" ${S}/>`,
    download: `<path d="M12 4v11M7.5 10.5L12 15l4.5-4.5" stroke="#2f6db5" stroke-width="2" fill="none" ${S}/><path d="M4 15v4.5h16V15" stroke="#5b6470" stroke-width="1.8" fill="none" ${S}/>`,
    newfolder: `<path d="M2.5 6.5a1.5 1.5 0 0 1 1.5-1.5h5l2 2h9a1.5 1.5 0 0 1 1.5 1.5v10a1.5 1.5 0 0 1-1.5 1.5H4a1.5 1.5 0 0 1-1.5-1.5z" fill="#f0c040" stroke="#c99a1a" stroke-width="1"/><path d="M12 10.5v6M9 13.5h6" stroke="#2e9d4a" stroke-width="2" ${S}/>`,
    search: `<circle cx="10.5" cy="10.5" r="6" stroke="#47607e" stroke-width="2" fill="#eaf3ff"/><path d="M15 15l5.5 5.5" stroke="#47607e" stroke-width="2.4" ${S}/>`,
    edit: `<path d="M4 20l1-4.5L15.5 5l3.5 3.5L8.5 19z" fill="#f2b632" stroke="#9c6f2f" stroke-width="1" ${S}/><path d="M4 20l1-4.5 3.5 3.5z" fill="#4f5967"/>`,
    trash: `<path d="M5 7h14l-1.2 13.5H6.2z" fill="#c9d1db" stroke="#5b6470" stroke-width="1.2" ${S}/><path d="M3.5 7h17M9.5 7V4.5h5V7M10 10.5v7M14 10.5v7" stroke="#5b6470" stroke-width="1.4" fill="none" ${S}/>`,
    copy: `<rect x="8" y="8" width="12" height="12" rx="1.5" fill="#fff" stroke="#47607e" stroke-width="1.4"/><path d="M16 8V5.5A1.5 1.5 0 0 0 14.5 4h-9A1.5 1.5 0 0 0 4 5.5v9A1.5 1.5 0 0 0 5.5 16H8" fill="none" stroke="#47607e" stroke-width="1.4"/>`,
    play: `<path d="M7 4.5l12 7.5-12 7.5z" fill="#2e9d4a" stroke="#1f7035" stroke-width="1" ${S}/>`,
    record: `<circle cx="12" cy="12" r="7" fill="#d63a3a"/>`,
    stop: `<rect x="6" y="6" width="12" height="12" rx="1.5" fill="#4f5967"/>`,
    macro: `<rect x="2.5" y="5" width="19" height="14" rx="2" fill="#fff" stroke="#47607e" stroke-width="1.4"/><path d="M6 9h2M10 9h2M14 9h4M6 12.5h4M12 12.5h6M7 16h10" stroke="#47607e" stroke-width="1.6" ${S}/><circle cx="19.5" cy="5.5" r="3" fill="#d63a3a"/>`,
    network: `<circle cx="12" cy="5" r="2.5" fill="#2f6db5"/><circle cx="5" cy="18.5" r="2.5" fill="#2e9d4a"/><circle cx="19" cy="18.5" r="2.5" fill="#d9822b"/><path d="M12 7.5v4.5M12 12l-6 5M12 12l6 5" stroke="#5b6470" stroke-width="1.5"/>`,
    scan: `<circle cx="12" cy="12" r="9" fill="#173b2a"/><circle cx="12" cy="12" r="5.5" fill="none" stroke="#3fae6a" stroke-width="1"/><path d="M12 12L19 6.5" stroke="#6fe08f" stroke-width="1.8" ${S}/><circle cx="15.5" cy="15" r="1.2" fill="#6fe08f"/>`,
    ports: `<rect x="3" y="6" width="18" height="12" rx="1.5" fill="#55606e"/><rect x="5.5" y="9" width="4" height="4" fill="#1d2027"/><rect x="10.5" y="9" width="4" height="4" fill="#1d2027"/><rect x="15.5" y="9" width="3" height="4" fill="#1d2027"/><circle cx="7.5" cy="15.5" r=".9" fill="#5fe08a"/><circle cx="12.5" cy="15.5" r=".9" fill="#f0c040"/>`,
    wol: `<path d="M7.5 7a7 7 0 1 0 9 0" stroke="#2e9d4a" stroke-width="2.2" fill="none" ${S}/><path d="M12 3v8" stroke="#2e9d4a" stroke-width="2.2" ${S}/><path d="M3 21h18" stroke="#5b6470" stroke-width="1.6" stroke-dasharray="2 2"/>`,
    web: `<rect x="2.5" y="4" width="19" height="16" rx="1.5" fill="#fff" stroke="#2f8fd0" stroke-width="1.4"/><rect x="2.5" y="4" width="19" height="3.5" fill="#2f8fd0"/><path d="M6 12l-2 2 2 2M18 12l2 2-2 2M13.5 10.5l-3 7" stroke="#2f8fd0" stroke-width="1.5" fill="none" ${S}/>`,
    keyboard: `<rect x="2" y="6" width="20" height="12" rx="1.5" fill="#e9edf2" stroke="#5b6470" stroke-width="1.2"/><path d="M5 9.5h1M8 9.5h1M11 9.5h1M14 9.5h1M17 9.5h2M5 12.5h2M9 12.5h1M12 12.5h1M15 12.5h1M18 12.5h1M7 15.5h10" stroke="#5b6470" stroke-width="1.5" ${S}/>`,
    key: `<circle cx="7.5" cy="12" r="4.5" fill="#f2b632" stroke="#9c6f2f" stroke-width="1"/><circle cx="6.5" cy="12" r="1.3" fill="#fff"/><path d="M12 12h9.5v3M18.5 12v2.5" stroke="#9c6f2f" stroke-width="2" fill="none" ${S}/>`,
    star: `<path d="M12 2.5l2.8 5.8 6.2.9-4.5 4.4 1.1 6.2L12 16.9l-5.6 2.9 1.1-6.2L3 9.2l6.2-.9z" fill="#f2b632" stroke="#c98a10" stroke-width="1" ${S}/>`,
    lock: `<rect x="5" y="10.5" width="14" height="10" rx="1.5" fill="#f2b632" stroke="#9c6f2f" stroke-width="1"/><path d="M8 10.5V8a4 4 0 0 1 8 0v2.5" stroke="#5b6470" stroke-width="2" fill="none"/>`,
    info: `<circle cx="12" cy="12" r="9.5" fill="#2f6db5"/><path d="M12 11v6" stroke="#fff" stroke-width="2.2" ${S}/><circle cx="12" cy="7.5" r="1.3" fill="#fff"/>`,
    warning: `<path d="M12 3l10 18H2z" fill="#f2b632" stroke="#9c6f2f" stroke-width="1" ${S}/><path d="M12 9.5v5" stroke="#1d2027" stroke-width="2" ${S}/><circle cx="12" cy="17.5" r="1.1" fill="#1d2027"/>`,
    gamepad: `<path d="M6.5 8h11a4 4 0 0 1 3.9 4.8l-.8 4a2.5 2.5 0 0 1-4.3 1.2L14.5 16h-5l-1.8 2a2.5 2.5 0 0 1-4.3-1.2l-.8-4A4 4 0 0 1 6.5 8z" fill="#55606e"/><path d="M7.5 10.5v3M6 12h3" stroke="#fff" stroke-width="1.4" ${S}/><circle cx="16" cy="11" r=".9" fill="#5fe08a"/><circle cx="17.5" cy="13" r=".9" fill="#f05a5a"/>`,
    clock: `<circle cx="12" cy="12" r="9" fill="#fff" stroke="#47607e" stroke-width="1.6"/><path d="M12 7v5l3.5 2" stroke="#47607e" stroke-width="1.8" fill="none" ${S}/>`,
    chevron: `<path d="M9 6l6 6-6 6" stroke="currentColor" stroke-width="2" fill="none" ${S}/>`,
    caret: `<path d="M7 10l5 5 5-5z" fill="currentColor"/>`,
  };
</script>

<script lang="ts">
  let { name, size = 16, title = "" }: { name: string; size?: number; title?: string } = $props();
</script>

<svg
  class="ico"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  aria-hidden={title ? undefined : "true"}
  role={title ? "img" : undefined}
  aria-label={title || undefined}
>{@html ICONS[name] ?? ICONS.file}</svg>

<style>
  .ico {
    flex: 0 0 auto;
    display: inline-block;
    vertical-align: middle;
  }
</style>
