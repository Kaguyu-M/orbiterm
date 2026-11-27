// Background image of the terminals: read once from the local file (data: URL, allowed by the
// page's content security policy), shared by every terminal.
import { api } from "./api";

const cache = new Map<string, Promise<string>>();

export function imageUrl(path: string): Promise<string> {
  let p = cache.get(path);
  if (!p) {
    p = api.imageDataUrl(path);
    cache.set(path, p);
    p.catch(() => cache.delete(path));
  }
  return p;
}
