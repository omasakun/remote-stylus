import { unpackMultiple } from 'msgpackr'

export function assert(condition: boolean, message = 'Assertion failed.'): asserts condition {
  if (!condition) throw new Error(message)
}

export function never(_: never): never {
  throw new Error('never')
}

export function run<T>(fn: () => T): T {
  return fn()
}

export function concatUint8(as: Uint8Array[]) {
  const size = as.reduce((s, a) => s + a.length, 0)
  const res = new Uint8Array(size)
  let offset = 0
  for (const a of as) {
    res.set(a, offset)
    offset += a.length
  }
  return res
}

export class UnpackStream {
  #incomplete: Uint8Array | undefined = undefined
  next(data: Uint8Array): unknown[] {
    if (this.#incomplete) data = concatUint8([this.#incomplete, data])
    this.#incomplete = undefined

    try {
      return unpackMultiple(data)
    } catch (e: any) {
      if (e.incomplete) {
        this.#incomplete = data.subarray(e.lastPosition)
        return e.values ?? []
      }
      throw e
    }
  }
}

export class Reorderer<T extends { i: number }> {
  lastI = -1
  buffer = new Map<number, T>()
  constructor(private callback: (data: T) => void) {}
  push(data: T) {
    if (data.i === this.lastI + 1) {
      this.callback(data)
      this.lastI = data.i
      while (this.buffer.has(this.lastI + 1)) {
        this.callback(this.buffer.get(this.lastI + 1)!)
        this.buffer.delete(this.lastI + 1)
        this.lastI++
      }
    } else if (data.i > this.lastI + 1) {
      this.buffer.set(data.i, data)
    } else {
      // ignore
    }
  }
}

export interface MsgpackPointerEventInfo {
  eventType: 'up' | 'move' | 'down' | 'cancel'
  pointerId: number
  pointerType: string
  isPrimary: boolean
  normalizedX: number
  normalizedY: number
  button: number
  buttons: number
  width: number
  height: number
  pressure: number
  tangentialPressure: number
  tiltX: number
  tiltY: number
  twist: number
}

export class MsgpackPointerEvent {
  constructor(public info: MsgpackPointerEventInfo) {}
  static fromEvent(
    eventType: 'up' | 'move' | 'down' | 'cancel',
    e: PointerEvent,
    rect: DOMRect,
  ): MsgpackPointerEvent {
    return new MsgpackPointerEvent({
      eventType: eventType,
      pointerId: e.pointerId,
      pointerType: e.pointerType,
      isPrimary: e.isPrimary,
      normalizedX: (e.clientX - rect.left) / rect.width,
      normalizedY: (e.clientY - rect.top) / rect.height,
      button: e.button,
      buttons: e.buttons,
      width: e.width,
      height: e.height,
      pressure: e.pressure,
      tangentialPressure: e.tangentialPressure,
      tiltX: e.tiltX,
      tiltY: e.tiltY,
      twist: e.twist,
    })
  }
  static deserialize(data: unknown): MsgpackPointerEvent {
    assert(Array.isArray(data))
    return new MsgpackPointerEvent({
      eventType: data[0] as MsgpackPointerEventInfo['eventType'],
      pointerId: data[1] as number,
      pointerType: data[2] as string,
      isPrimary: data[3] as boolean,
      normalizedX: data[4] as number,
      normalizedY: data[5] as number,
      button: data[6] as number,
      buttons: data[7] as number,
      width: data[8] as number,
      height: data[9] as number,
      pressure: data[10] as number,
      tangentialPressure: data[11] as number,
      tiltX: data[12] as number,
      tiltY: data[13] as number,
      twist: data[14] as number,
    })
  }
  serialize(): unknown {
    return [
      this.info.eventType,
      this.info.pointerId,
      this.info.pointerType,
      this.info.isPrimary,
      this.info.normalizedX,
      this.info.normalizedY,
      this.info.button,
      this.info.buttons,
      this.info.width,
      this.info.height,
      this.info.pressure,
      this.info.tangentialPressure,
      this.info.tiltX,
      this.info.tiltY,
      this.info.twist,
    ]
  }
}
