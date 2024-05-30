// Signaling server for WebRTC

// User inputs a 6-digit room ID to perform WebRTC signaling
// Internally, it calls a worker with `app_id:room_id` (app_id is a 1-24 character string, room_id is a 6-digit number)
// Rooms are automatically cleaned up after 10 minute unless explicitly extended
// Expired rooms are cleaned up with a low probability when a new room is created

import { zValidator } from '@hono/zod-validator'
import { Hono } from 'hono'
import { bodyLimit } from 'hono/body-limit'
import { cors } from 'hono/cors'
import { csrf } from 'hono/csrf'
import { HTTPException } from 'hono/http-exception'
import { z } from 'zod'

const VACCUM_PROBABILITY = 0.01
const NEW_ROOM_RETRIES = 3 // the number of retries to create a new room with a random ID
const ROOM_EXPIRES = 600 * 1000 // 10 minutes
const MESSAGE_MAX_SIZE = 100 * 1024 // 100 KB

type Bindings = {
  D1: D1Database
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

// Create a new room
// worker/rooms?app_id=xxx
app.post('/rooms', zValidator('query', appIdQuery), async (c) => {
  const { D1 } = c.env
  const { app_id } = c.req.valid('query')
  const time = Date.now()

  if (Math.random() < VACCUM_PROBABILITY) c.executionCtx.waitUntil(vaccumRooms(c.env.D1, time))

  const room = await createNewRoom(D1, app_id, time)
  if (room === undefined) throw new HTTPException(500, { message: 'Failed to create a new room' })
  return c.json({ room }, 201)
})

// Extend the expiration of a room
// worker.
app.post('/rooms/:room/extend', zValidator('param', roomParam), async (c) => {
  const { D1 } = c.env
  const { room } = c.req.valid('param')
  const time = Date.now()

  const stmt = D1.prepare('UPDATE rooms SET expires_at = ? WHERE id = ? AND expires_at > ?')
  const result = await stmt.bind(time + ROOM_EXPIRES, room, time).run()
  if (!result.meta.changed_db) throw new HTTPException(404, { message: 'Room not found' })
  return c.text('', 204)
})

// Delete a room
app.delete('/rooms/:room', zValidator('param', roomParam), async (c) => {
  const { D1 } = c.env
  const { room } = c.req.valid('param')

  const stmt = D1.prepare('DELETE FROM rooms WHERE id = ?')
  await stmt.bind(room).run()
  return c.text('', 204)
})

// Get messages from a room
app.get(
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

// Post a message to a room
app.post(
  '/rooms/:room/messages',
  zValidator('param', roomParam),
  bodyLimit({ maxSize: MESSAGE_MAX_SIZE }),
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

function isAllowedOrigin(origin: string | undefined) {
  if (origin === undefined) return false
  if (origin === 'https://o137.net') return true
  if (origin.startsWith('https://') && origin.endsWith('.o137.net')) return true
  if (origin.startsWith('http://localhost:')) return true
  if (origin === 'https://tauri.localhost') return true
  return false
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

// TODO: room can be deleted between existence check and actual operation!
async function checkRoomExists(d1: D1Database, room: string, time: number) {
  const stmt = d1.prepare('SELECT 1 FROM rooms WHERE id = ? AND expires_at > ?')
  const result = await stmt.bind(room, time).first()
  return !!result
}

async function vaccumRooms(d1: D1Database, time: number) {
  const stmt = d1.prepare('DELETE FROM rooms WHERE expires_at < ?')
  await stmt.bind(time).run()
}

function randomDigits(length: number) {
  let result = ''
  for (let i = 0; i < length; i++) result += Math.floor(Math.random() * 10).toString()
  return result
}

export default {
  fetch: app.fetch,
}
