/** Typed Tauri bridge calls for one terminal session. */

import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  TerminalCloseRequest,
  TerminalInputRequest,
  TerminalResizeRequest,
  TerminalSpawnRequest,
  TerminalSpawnResponse,
} from "@muxyard/contracts";

export async function spawnTerminal(
  request: TerminalSpawnRequest,
): Promise<TerminalSpawnResponse> {
  return invoke<TerminalSpawnResponse>("terminal_spawn", { request });
}

export async function attachTerminal(
  streamId: string,
  channel: Channel<ArrayBuffer>,
): Promise<void> {
  await invoke("terminal_attach", { stream_id: streamId, channel });
}

export async function inputTerminal(
  streamId: string,
  data: string,
): Promise<void> {
  const request: TerminalInputRequest = { stream_id: streamId, data };
  await invoke("terminal_input", { request });
}

export async function resizeTerminal(
  streamId: string,
  rows: number,
  cols: number,
): Promise<void> {
  const request: TerminalResizeRequest = {
    stream_id: streamId,
    rows,
    cols,
  };
  await invoke("terminal_resize", { request });
}

export async function closeTerminal(streamId: string): Promise<void> {
  const request: TerminalCloseRequest = { stream_id: streamId };
  await invoke("terminal_close", { request });
}
