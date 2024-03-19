import {
  Peer,
  Reorderer,
  SignalingRoom,
  SignalingServer,
  run,
  type SignalingMessage,
} from '@remote-stylus/shared'
import { useEffect, useRef, useState } from 'react'

const server = new SignalingServer()

export function App() {
  const videoRef = useRef<HTMLVideoElement>(null)
  const [error, setError] = useState<string | null>(null)
  const [roomId, setRoomId] = useState<string | null>(null)

  useEffect(() => {
    let aborted = false
    const cleanup = run(async () => {
      const reorderer = new Reorderer<SignalingMessage>((message) => {
        console.log('received signal', message.data)
        peer.signal(message.data)
      })

      const room = await SignalingRoom.create(server, (message) => {
        if (message.to == 0) reorderer.push(message)
      })

      if (!room) {
        setError('Failed to create room')
        return
      }

      if (aborted) {
        void room.dispose()
        return
      }

      setRoomId(room.roomId)

      const peer = new Peer({ initiator: true, trickle: false })
      const queue: SignalingMessage[] = []

      peer.on('data:signal', (data: SignalingMessage) => {
        if (data.to === 0) reorderer.push(data)
      })

      peer.on('signal', (data) => {
        const item: SignalingMessage = { i: queue.length, from: 0, to: 1, data }
        queue.push(item)
        console.log('sending signal', data)
        if (peer.connected) {
          peer.sendObject('signal', item)
        } else {
          void room.sendMessage(item)
        }
      })

      peer.on('connect', () => {
        console.log('connected')

        // 接続開始とほぼ同時に発生した signal メッセージがもれないように、接続後に念のため再送信する
        queue.forEach((item) => {
          peer.sendObject('signal', item.data)
        })
        void room.dispose()
      })

      peer.on('stream', (stream) => {
        if (videoRef.current) {
          videoRef.current.srcObject = stream
        }
      })

      return () => {
        void room.dispose()
        peer.destroy()
      }
    })
    return () => {
      aborted = true
      void cleanup.then((fn) => fn?.())
    }
  }, [])

  return (
    <div>
      <h1>Receiver</h1>
      {error ? <p>{error}</p> : null}
      {roomId === null ? <p>Connecting...</p> : <p>Room ID: {roomId}</p>}
      <video ref={videoRef} autoPlay playsInline />
    </div>
  )
}
