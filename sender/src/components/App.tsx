import { InputOTP, InputOTPGroup, InputOTPSlot } from '@/components/ui/input-otp'
import {
  Peer,
  Reorderer,
  SignalingRoom,
  SignalingServer,
  run,
  type SignalingMessage,
} from '@remote-stylus/shared'
import { useEffect, useState } from 'react'

const server = new SignalingServer()

export function App() {
  const [error, setError] = useState<string | null>(null)
  const [roomId, setRoomId] = useState<string | null>(null)

  useEffect(() => {
    if (!roomId) return

    const cleanup = run(async () => {
      const reorderer = new Reorderer<SignalingMessage>((message) => {
        console.log('received signal', message.data)
        peer.signal(message.data)
      })

      const room = SignalingRoom.join(server, roomId, (message) => {
        if (message.to == 1) reorderer.push(message)
      })

      if (!room) {
        setError('Failed to create room')
        return
      }

      const peer = new Peer({ initiator: false, trickle: false })
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

      peer.on('connect', () => {
        console.log('connected')

        // 接続開始とほぼ同時に発生した signal メッセージがもれないように、接続後に念のため再送信する
        queue.forEach((item) => {
          peer.sendObject('signal', item.data)
        })
        void room.dispose()

        navigator.mediaDevices.getUserMedia({ video: true }).then((stream) => {
          peer.addStream(stream)
        })
      })

      return () => {
        void room.dispose()
        peer.destroy()
      }
    })
    return () => {
      void cleanup.then((fn) => fn?.())
    }
  }, [roomId])

  if (error) {
    return <p>{error}</p>
  }

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
