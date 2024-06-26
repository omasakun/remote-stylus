import {
  MsgpackPointerEvent,
  Peer,
  Reorderer,
  SignalingRoom,
  SignalingServer,
  never,
  run,
  type SignalingMessage,
} from '@remote-stylus/shared'
import { useEffect, useState } from 'react'
import { injectPointerEvent, resetPointerDevice } from './services/pointer'

const server = new SignalingServer()

type Status =
  | {
      type: 'creating-room'
    }
  | {
      type: 'waiting-for-connection'
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
  return (
    <div className='container mx-auto my-8 text-center font-medium'>
      <Inner />
    </div>
  )
}

export function Inner() {
  const [status, setStatus] = useState<Status>({ type: 'creating-room' })

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
        setStatus({ type: 'error', message: 'Failed to create a new room' })
        return
      }

      if (aborted) {
        void room.dispose()
        return
      }

      setStatus({ type: 'waiting-for-connection', roomId: room.roomId })

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
        setStatus({ type: 'connected' })

        // Resend the signal messages that were almost simultaneously generated with the connection start to prevent them from being missed.
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
      }
    })
    return () => {
      aborted = true
      void cleanup.then((fn) => fn?.())
    }
  }, [])

  async function onConnected(peer: Peer) {
    // TODO: implement screen capture in Rust, so that we can capture the screen without the permission dialog

    const stream = await navigator.mediaDevices.getDisplayMedia({
      video: {
        displaySurface: 'monitor',
      },
    })
    peer.addStream(stream)

    resetPointerDevice()

    peer.on('data:pointer', (data) => {
      const event = MsgpackPointerEvent.deserialize(data).info
      // console.log(event)
      console.log('pointer event', event.eventType)

      if (event.button < 0) event.button = 0 // TODO: move this to the client?
      injectPointerEvent(event)
    })
  }

  async function onStream(peer: Peer, stream: MediaStream) {
    // TODO
  }

  if (status.type === 'creating-room') {
    return <p>Creating room...</p>
  }
  if (status.type === 'waiting-for-connection') {
    return (
      <div className='flex flex-col gap-2'>
        <p>Room ID</p>
        <p className='text-3xl font-bold'>{status.roomId}</p>
      </div>
    )
  }
  if (status.type === 'connected') {
    return <p>Connected</p>
  }
  if (status.type === 'closed') {
    return <p>Connection closed</p>
  }
  if (status.type === 'error') {
    return <p>Error: {status.message}</p>
  }

  never(status)
}
