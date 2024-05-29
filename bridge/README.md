# HID Bridge

## Setup & Flash

- Make sure to install [esp-idf dependencies](https://docs.espressif.com/projects/esp-idf/en/latest/esp32/get-started/linux-macos-setup.html#step-1-install-prerequisites).
- Compilation on windows might not work due to the long path names. WSL2 is recommended.
- Instead of binding the USB device to WSL2, you can install `espflash` tool on Windows and flash the device with it.
  - In this case, edit `.cargo/config` file to use `espflash.exe` instead of `espflash`.

```sh
apt    install libudev-dev  # linux
winget install usbipd       # wsl2 (usb binding)

cargo install ldproxy
cargo install espup
cargo install espflash

espup install

# Run every time you open a new terminal
. $HOME/export-esp.sh  # linux

# Bind USB devices to WSL2
usbipd list
usbipd attach --wsl --busid <busid>

# Build and flash
cargo run
```

See [esp-rs/esp-idf-template](https://github.com/esp-rs/esp-idf-template) for more information.

## References

- [The Rust on ESP Book](https://docs.esp-rs.org/book/introduction.html)
- [Embedded Rust on Espressif](https://docs.esp-rs.org/std-training/01_intro.html)
- [esp-idf-svc & examples](https://github.com/esp-rs/esp-idf-svc)
- [esp-idf-sys & examples](https://github.com/esp-rs/esp-idf-sys)
- [esp-idf & examples](https://github.com/espressif/esp-idf)
- [grep.app](https://grep.app)
