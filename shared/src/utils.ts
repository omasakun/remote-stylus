import { unpackMultiple } from 'msgpackr'

export function assert(condition: boolean, message = 'Assertion failed.'): asserts condition {
  if (!condition) throw new Error(message)
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
