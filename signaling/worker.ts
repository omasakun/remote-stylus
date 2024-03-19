// WebRTC のシグナリングを行う
// ユーザーは 6 桁の ID を入力して、シグナリングを行う
// 内部的には `app_id:room_id` という形式の Room ID を使ってワーカーを呼び出す
// 明示的に延長を行わない場合、取得から 1 分で Room は削除される
// アカウント単位で使用可能な cron の個数に制限があるので、 Room のクリーンアップはルーム生成時に低確率で実行することにする

import { zValidator } from '@hono/zod-validator'
import { Hono } from 'hono'
import { bodyLimit } from 'hono/body-limit'
import { cors } from 'hono/cors'
import { csrf } from 'hono/csrf'
import { HTTPException } from 'hono/http-exception'
import { z } from 'zod'

const VACCUM_PROBABILITY = 0.01
const NEW_ROOM_RETRIES = 3 // ランダムな ID で新しい Room を作成する際のリトライ回数
const ROOM_EXPIRES = 120 * 1000 // Room の有効期限

type Bindings = {
  D1: D1Database
}

function isAllowedOrigin(origin: string | undefined) {
  if (origin === undefined) return false
  if (origin === 'https://o137.net') return true
  if (origin.startsWith('https://') && origin.endsWith('.o137.net')) return true
  return false
}

function randomDigits(length: number) {
  let result = ''
  for (let i = 0; i < length; i++) result += Math.floor(Math.random() * 10).toString()
  return result
}

async function createNewRoom(d1: D1Database, appId: string, time: number) {
  const stmt = d1.prepare('INSERT INTO rooms (id, expires_at) VALUES (?, ?)')
  for (let i = 0; i < NEW_ROOM_RETRIES; i++) {
    const room = `${appId}-${randomDigits(6)}`
    try {
      await stmt.bind(room, time + ROOM_EXPIRES).run()
      return room
    } catch (e) {}
  }
}

// TODO: room の存在確認から実際の操作をするまでの間に room が消える可能性、稀にあると思います！
async function checkRoomExists(d1: D1Database, room: string, time: number) {
  const stmt = d1.prepare('SELECT 1 FROM rooms WHERE id = ? AND expires_at > ?')
  const result = await stmt.bind(room, time).first()
  return !!result
}

async function vaccumRooms(d1: D1Database, time: number) {
  const stmt = d1.prepare('DELETE FROM rooms WHERE expires_at < ?')
  await stmt.bind(time).run()
}

const app = new Hono<{ Bindings: Bindings }>()

// cors, csrf, origin check
app.use(
  cors({ origin: (origin) => (isAllowedOrigin(origin) ? origin : 'https://o137.net') }),
  csrf({ origin: (origin) => isAllowedOrigin(origin) }),
  async (c, next) => {
    const origin = c.req.header('origin')
    if (!isAllowedOrigin(origin)) {
      const res = new Response('Forbidden', { status: 403 })
      throw new HTTPException(403, { res })
    }
    await next()
  },
)

const roomParam = z.object({
  room: z.string().regex(/^[a-z\-]{1,24}-\d{6}$/),
})

const appIdQuery = z.object({
  app_id: z.string().regex(/^[a-z\-]{1,24}$/),
})

const offsetQuery = z.object({
  since: z
    .string()
    .optional()
    .transform((v) => (v === undefined ? -1 : parseInt(v, 10)))
    .refine((v) => Number.isInteger(v) && v >= -1, { message: 'Invalid offset' }),
})

app.post(
  // Create a new room
  '/rooms',
  zValidator('query', appIdQuery),
  async (c) => {
    const { D1 } = c.env
    const { app_id } = c.req.valid('query')
    const time = Date.now()

    if (Math.random() < VACCUM_PROBABILITY) c.executionCtx.waitUntil(vaccumRooms(c.env.D1, time))

    const room = await createNewRoom(D1, app_id, time)
    if (room === undefined) throw new HTTPException(500, { message: 'Failed to create a new room' })
    return c.json({ room }, 201)
  },
)

app.post(
  // Extend the expiration of a room
  '/rooms/:room/extend',
  zValidator('param', roomParam),
  async (c) => {
    const { D1 } = c.env
    const { room } = c.req.valid('param')
    const time = Date.now()

    const stmt = D1.prepare('UPDATE rooms SET expires_at = ? WHERE id = ? AND expires_at > ?')
    const result = await stmt.bind(time + ROOM_EXPIRES, room, time).run()
    if (!result.meta.changed_db) throw new HTTPException(404, { message: 'Room not found' })
    return c.text('', 204)
  },
)

app.delete(
  // Delete a room
  '/rooms/:room',
  zValidator('param', roomParam),
  async (c) => {
    const { D1 } = c.env
    const { room } = c.req.valid('param')

    const stmt = D1.prepare('DELETE FROM rooms WHERE id = ?')
    await stmt.bind(room).run()
    return c.text('', 204)
  },
)

app.get(
  // Get messages from a room
  '/rooms/:room/messages',
  zValidator('param', roomParam),
  zValidator('query', offsetQuery),
  async (c) => {
    const { D1 } = c.env
    const { room } = c.req.valid('param')
    const { since } = c.req.valid('query')
    const time = Date.now()

    if (!(await checkRoomExists(D1, room, time))) {
      throw new HTTPException(404, { message: 'Room not found' })
    }

    const stmt = D1.prepare(
      'SELECT id, body from messages WHERE room = ? AND id > ? ORDER BY id ASC',
    )
    const { results } = await stmt.bind(room, since).all()

    return c.json({ messages: results })
  },
)

app.post(
  // Post a message to a room
  '/rooms/:room/messages',
  zValidator('param', roomParam),
  bodyLimit({ maxSize: 100 * 1024 }),
  async (c) => {
    const { D1 } = c.env
    const { room } = c.req.valid('param')
    const body = await c.req.text()
    const time = Date.now()

    if (!(await checkRoomExists(D1, room, time))) {
      throw new HTTPException(404, { message: 'Room not found' })
    }

    const stmt = D1.prepare('INSERT INTO messages (room, body) VALUES (?, ?)')
    await stmt.bind(room, body).run()

    return c.text('', 201)
  },
)

export default {
  fetch: app.fetch,
}
