import type { ILink, ILinkProvider, Terminal } from "@xterm/xterm";

const URL_PATTERN = /https?:\/\/[^\s<>"'`]+/gi;
const TRAILING_PUNCTUATION = /[.,;:!?)\]}>]+$/;
const MAX_URL_LENGTH = 2048;

export type ValidatedLink = {
  url: string;
  start: number;
  length: number;
};

export function validateLinkUrl(value: string): string | null {
  if (!value || value.length > MAX_URL_LENGTH) return null;
  try {
    const url = new URL(value);
    if (url.protocol !== "http:" && url.protocol !== "https:") return null;
    if (url.username || url.password) return null;
    return url.toString();
  } catch {
    return null;
  }
}

export function findValidatedLinks(text: string): ValidatedLink[] {
  const links: ValidatedLink[] = [];
  for (const match of text.matchAll(URL_PATTERN)) {
    const raw = match[0];
    const start = match.index ?? 0;
    const trimmed = raw.replace(TRAILING_PUNCTUATION, "");
    const url = validateLinkUrl(trimmed);
    if (url) links.push({ url, start, length: trimmed.length });
  }
  return links;
}

export function createLinkProvider(
  terminal: Terminal,
  openLink: (url: string) => void = (url) => {
    window.open(url, "_blank", "noopener,noreferrer");
  },
): ILinkProvider {
  return {
    provideLinks(bufferLineNumber, callback) {
      const line = terminal.buffer.active.getLine(bufferLineNumber - 1);
      if (!line) {
        callback(undefined);
        return;
      }
      const links: ILink[] = findValidatedLinks(
        line.translateToString(true),
      ).map((link) => ({
        range: {
          start: { x: link.start + 1, y: bufferLineNumber },
          end: { x: link.start + link.length + 1, y: bufferLineNumber },
        },
        text: link.url,
        activate: (_event, text) => {
          const validated = validateLinkUrl(text);
          if (validated) openLink(validated);
        },
      }));
      callback(links.length > 0 ? links : undefined);
    },
  };
}
