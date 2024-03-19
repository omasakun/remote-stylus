DROP TABLE IF EXISTS rooms;
DROP TABLE IF EXISTS messages;

CREATE TABLE rooms (
  id TEXT PRIMARY KEY,
  expires_at INTEGER NOT NULL
);

CREATE INDEX rooms_expires_at ON rooms (expires_at);

CREATE TABLE messages (
  id INTEGER PRIMARY KEY,
  room TEXT NOT NULL,
  body TEXT NOT NULL,
  FOREIGN KEY (room) REFERENCES rooms (id) ON DELETE CASCADE
);

CREATE INDEX messages_room ON messages (room);
