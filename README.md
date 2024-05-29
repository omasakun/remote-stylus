# Remote Stylus

Turn your iPad into a drawing tablet for your PC.

```
PC at Home
  |
  |
  | Internet
  | (WebRTC)
  |
  |
iPad --------- ESP32 --------- Keyboard
        BLE          Bluetooth
```

## HID Bridge

iPad does not allow browser app to access full keyboard input (e.g. `Alt+Tab` or `F11`), which is required for a comfortable remote desktop experience. To work around this limitation, this project uses a cheap [ESP32](https://en.wikipedia.org/wiki/ESP32) microcontroller to bridge the HID input from a physical bluetooth keyboard to the iPad.

See [bridge/README.md](bridge) for more information.

## Signaling Server

The WebRTC signaling server is implemented as a simple [cloudflare worker](https://developers.cloudflare.com/workers/) with [D1 database](https://developers.cloudflare.com/d1/) that exchanges the WebRTC offer and answer between the iPad and the PC.

See [signaling/README.md](signaling) for more information.
