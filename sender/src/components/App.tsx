import { Button } from '@/components/ui/button'
import { InputOTP, InputOTPGroup, InputOTPSlot } from '@/components/ui/input-otp'
import {
  MsgpackPointerEvent,
  Peer,
  Reorderer,
  SignalingRoom,
  SignalingServer,
  Video,
  never,
  run,
  type SignalingMessage,
} from '@remote-stylus/shared'
import { useEffect, useRef, useState } from 'react'

const server = new SignalingServer()

type Status =
  | {
      type: 'idle'
    }
  | {
      type: 'connecting'
      roomId: string
    }
  | {
      type: 'connected'
    }
  | {
      type: 'closed'
    }
  | {
      type: 'error'
      message: string
    }

export function App() {
  const peerRef = useRef<Peer | null>(null)
  const [status, setStatus] = useState<Status>({ type: 'idle' })
  const [roomId, setRoomId] = useState<string | null>(null)

  const videoRef = useRef<HTMLVideoElement>(null)
  const [videoStream, setVideoStream] = useState<MediaStream | null>(null)

  useEffect(() => {
    if (!roomId) return
    setStatus({ type: 'connecting', roomId })

    const cleanup = run(async () => {
      const reorderer = new Reorderer<SignalingMessage>((message) => {
        console.log('received signal', message.data)
        peer.signal(message.data)
      })

      const room = SignalingRoom.join(server, roomId, (message) => {
        if (message.to == 1) reorderer.push(message)
      })

      if (!room) {
        setStatus({ type: 'error', message: 'Failed to join room' })
        return
      }

      const peer = new Peer({ initiator: false, trickle: false })
      peerRef.current = peer

      const queue: SignalingMessage[] = []

      peer.on('data:signal', (data: SignalingMessage) => {
        if (data.to === 1) reorderer.push(data)
      })

      peer.on('signal', (data) => {
        const item: SignalingMessage = { i: queue.length, from: 1, to: 0, data }
        queue.push(item)
        console.log('sending signal', data)
        if (peer.connected) {
          peer.sendObject('signal', item)
        } else {
          void room.sendMessage(item)
        }
      })

      peer.on('connect', async () => {
        setStatus({ type: 'connected' })

        // 接続開始とほぼ同時に発生した signal メッセージがもれないように、接続後に念のため再送信する
        queue.forEach((item) => {
          peer.sendObject('signal', item.data)
        })
        void room.dispose()

        void onConnected(peer)
      })

      peer.on('stream', (stream) => {
        void onStream(peer, stream)
      })

      peer.on('close', () => {
        setStatus({ type: 'closed' })
      })

      peer.on('error', (error) => {
        setStatus({ type: 'error', message: error.message })
      })

      return () => {
        void room.dispose()
        peer.destroy()
        peerRef.current = null
      }
    })

    return () => {
      void cleanup.then((fn) => fn?.())
    }
  }, [roomId])

  async function onConnected(peer: Peer) {
    // TODO
  }

  async function onStream(peer: Peer, stream: MediaStream) {
    setVideoStream(stream)
  }

  function onPointerEvent(
    eventType: 'start' | 'move' | 'end',
    e: React.PointerEvent<HTMLVideoElement>,
  ) {
    e.preventDefault()
    e.stopPropagation()

    const peer = peerRef.current
    if (!peer) return

    const rect = e.currentTarget.getBoundingClientRect()
    const event = MsgpackPointerEvent.fromEvent(eventType, e.nativeEvent, rect)
    peer.sendObject('pointer', event.serialize())
  }

  if (status.type === 'idle') {
    return (
      <div>
        <div>
          {roomId ? (
            <p>Room ID: {roomId}</p>
          ) : (
            <div className='flex gap-4'>
              <InputOTP maxLength={6} onComplete={(roomId) => setRoomId(roomId)}>
                <InputOTPGroup>
                  <InputOTPSlot index={0} />
                  <InputOTPSlot index={1} />
                  <InputOTPSlot index={2} />
                  <InputOTPSlot index={3} />
                  <InputOTPSlot index={4} />
                  <InputOTPSlot index={5} />
                </InputOTPGroup>
              </InputOTP>
            </div>
          )}
        </div>
      </div>
    )
  }
  if (status.type === 'connecting') {
    return <div>Connecting to room {status.roomId}</div>
  }
  if (status.type === 'connected') {
    return (
      <div>
        <Button
          onClick={() => {
            videoRef.current?.requestFullscreen()
          }}>
          Fullscreen
        </Button>
        <Video
          ref={videoRef}
          srcObject={videoStream}
          autoPlay
          playsInline
          muted
          onPointerDown={(e) => onPointerEvent('start', e)}
          onPointerMove={(e) => onPointerEvent('move', e)}
          onPointerUp={(e) => onPointerEvent('end', e)}
          onPointerCancel={(e) => onPointerEvent('end', e)}
          style={{
            touchAction: 'none',
          }}
        />
      </div>
    )
  }
  if (status.type === 'closed') {
    return <div>Connection closed</div>
  }
  if (status.type === 'error') {
    return <div>Error: {status.message}</div>
  }

  never(status)
}
