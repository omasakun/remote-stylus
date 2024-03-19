import { pack } from 'msgpackr'
import SimplePeer from 'simple-peer'
import { UnpackStream, assert } from '.'

const MAX_MESSAGE_LENGTH_BYTES = 16000

export class Peer extends SimplePeer {
  unpacker = new UnpackStream()
  constructor(opts?: SimplePeer.Options) {
    super(opts)
    this.on('data', (data) => {
      // console.log('received data', data)
      this.unpacker.next(data).forEach((message) => {
        // console.log('received message', message)
        const [type, data] = message as [string, unknown]
        assert(typeof type === 'string')
        this.emit(`data:${type}`, data)
      })
    })
  }
  sendObject(type: string, data: unknown) {
    const message = pack([type, data])
    for (let i = 0; i < message.length; i += MAX_MESSAGE_LENGTH_BYTES) {
      const chunk = message.subarray(i, i + MAX_MESSAGE_LENGTH_BYTES)
      this.send(chunk)
    }
  }
}
