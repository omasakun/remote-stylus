import { MsgpackPointerEventInfo } from '@remote-stylus/shared'
import { invoke } from '@tauri-apps/api'

export async function resetPointerDevice() {
  await invoke('plugin:pointer|reset')
}

export async function injectPointerEvent(event: MsgpackPointerEventInfo) {
  await invoke('plugin:pointer|inject', { event })
}
