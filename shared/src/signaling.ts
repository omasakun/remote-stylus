import { EventEmitter } from 'eventemitter3'
import SimplePeer from 'simple-peer'
import { assert } from './utils'

const SIGNALING_SERVER = 'https://signaling.o137.workers.dev'
const APP_ID = 'remote-stylus'

const ROOM_EXTEND_INTERVAL = 60 * 1000
const MESSAGE_POLL_INTERVAL = 2000

export class SignalingServer {
  constructor(
    private baseUrl: string = SIGNALING_SERVER,
    private appId: string = APP_ID,
  ) {}

  async createNewRoom() {
    const url = `${this.baseUrl}/rooms?app_id=${encodeURIComponent(this.appId)}`
    const response = await fetch(url, { method: 'POST' })
    assert(response.ok, 'Failed to create a new room')

    const { room } = await response.json()
    assert(typeof room === 'string')
    assert(room.startsWith(this.appId + '-'))

    return room.slice(this.appId.length + 1)
  }

  async extendRoom(roomId: string) {
    const url = `${this.baseUrl}/rooms/${this.appId}-${roomId}/extend`
    const response = await fetch(url, { method: 'POST' })
    assert(response.ok, 'Failed to extend room')
  }

  async deleteRoom(roomId: string) {
    const url = `${this.baseUrl}/rooms/${this.appId}-${roomId}`
    const response = await fetch(url, { method: 'DELETE' })
    assert(response.ok, 'Failed to delete room')
  }

  async getMessages(roomId: string, since = -1) {
    const url = `${this.baseUrl}/rooms/${this.appId}-${roomId}/messages?since=${since}`
    const response = await fetch(url)
    assert(response.ok, 'Failed to get messages')

    const { messages } = await response.json()
    assert(Array.isArray(messages))

    return messages as { id: number; body: string }[]
  }

  async postMessage(roomId: string, messageBody: string) {
    const url = `${this.baseUrl}/rooms/${this.appId}-${roomId}/messages`
    const response = await fetch(url, { method: 'POST', body: messageBody })
    assert(response.ok, 'Failed to post message')
  }
}

export type SignalingMessage = { i: number; from: number; to: number; data: SimplePeer.SignalData }

interface SignalingRoomEvents {
  expired: () => void
}

export class SignalingRoom extends EventEmitter<SignalingRoomEvents> {
  private extendTimeout: ReturnType<typeof setTimeout> | undefined
  private receiverTimeout: ReturnType<typeof setTimeout> | undefined
  private expired = false
  private constructor(
    private server: SignalingServer,
    readonly roomId: string,
    private isHost: boolean,
    onMessage: (message: SignalingMessage) => void,
  ) {
    super()
    if (isHost) {
      const extend = async () => {
        try {
          await server.extendRoom(roomId)
          this.extendTimeout = setTimeout(extend, ROOM_EXTEND_INTERVAL)
        } catch {
          this.set_expired()
        }
      }
      this.extendTimeout = setTimeout(extend, ROOM_EXTEND_INTERVAL)
    }

    let lastMessageId = -1
    const receiver = async () => {
      try {
        const messages = await server.getMessages(roomId, lastMessageId)
        for (const message of messages) {
          lastMessageId = message.id
          onMessage(JSON.parse(message.body))
        }
        this.receiverTimeout = setTimeout(receiver, MESSAGE_POLL_INTERVAL)
      } catch {
        this.set_expired()
      }
    }
    receiver()
  }
  static async create(server: SignalingServer, onMessage: (message: SignalingMessage) => void) {
    try {
      const room = await server.createNewRoom()
      return new SignalingRoom(server, room, true, onMessage)
    } catch {
      return null
    }
  }
  static join(
    server: SignalingServer,
    room: string,
    onMessage: (message: SignalingMessage) => void,
  ) {
    try {
      return new SignalingRoom(server, room, false, onMessage)
    } catch {
      return null
    }
  }
  async sendMessage(message: SignalingMessage) {
    await this.server.postMessage(this.roomId, JSON.stringify(message))
  }
  isExpired() {
    return this.expired
  }
  set_expired() {
    if (this.expired) return
    this.expired = true
    if (this.extendTimeout) clearTimeout(this.extendTimeout)
    if (this.receiverTimeout) clearTimeout(this.receiverTimeout)
    this.emit('expired')
  }
  async dispose() {
    this.set_expired()
    if (this.isHost) await this.server.deleteRoom(this.roomId)
  }
}
