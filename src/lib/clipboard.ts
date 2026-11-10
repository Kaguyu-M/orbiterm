// System clipboard through the native plugin: WebKitGTK (Linux) refuses
// navigator.clipboard.readText(), so the web API is only a fallback.
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";

export async function clipRead(): Promise<string> {
  try {
    return (await readText()) ?? "";
  } catch {
    return (await navigator.clipboard?.readText().catch(() => "")) ?? "";
  }
}

export async function clipWrite(text: string): Promise<void> {
  try {
    await writeText(text);
  } catch {
    await navigator.clipboard?.writeText(text).catch(() => {});
  }
}
