# farreach

Rust CLI for using a Raspberry Pi or similar bench host as a remote embedded debugging station.

The bench host owns the local USB/JTAG/UART connections. Your workstation builds the firmware locally, connects to the bench host over Iroh, uploads the built artifact with platform metadata, and the server selects the configured target, flashes it, and streams status/log output back over the encrypted QUIC session.

## Current shape

- Transport: Iroh endpoint identity, relay/hole-punching, custom ALPN.
- Same binary on both sides: run `farreach serve` on the Raspberry Pi, and `farreach flash` or `farreach monitor` from your workstation.
- Flash/log backend: command templates configured per target, so existing tools do chip-specific work.
- Build policy: firmware is always built on the workstation. The server receives bytes and flashes; it does not run project builds.
- Target selection: the request may specify `--platform esp|stm32|nordic` or `--target <name>`. If there is only one configured target, the server can select it automatically.

This keeps the first implementation practical:

- ESP: `espflash`
- STM32: `probe-rs` or `openocd`
- Nordic: `probe-rs`, `nrfutil`, or `west flash`

## Server config

Create `farreach.toml` on the Raspberry Pi:

```toml
# Optional. If non-empty, only these Iroh endpoint IDs can connect.
authorized_peers = []

# Optional. Firmware uploads are staged here while flash commands run.
work_dir = ".farreach/work"

[targets.esp32c3]
platform = "esp"
serial = "/dev/ttyUSB0"
baud = 115200
chip = "esp32c3"
flash = ["espflash", "flash", "--port", "{serial}", "{firmware}"]
monitor = ["espflash", "monitor", "--port", "{serial}", "--baud", "{baud}"]

[targets.stm32]
platform = "stm32"
chip = "STM32F407VGTx"
flash = ["probe-rs", "download", "--chip", "{chip}", "{firmware}"]
monitor = ["probe-rs", "attach", "--chip", "{chip}"]

[targets.nrf52]
platform = "nordic"
chip = "nRF52840_xxAA"
flash = ["probe-rs", "download", "--chip", "{chip}", "{firmware}"]
monitor = ["probe-rs", "attach", "--chip", "{chip}"]
```

Supported template values are `{target}`, `{platform}`, `{firmware}`, `{firmware_name}`, `{serial}`, `{baud}`, and `{chip}`.

## End-to-end setup

Install the same `farreach` binary on both machines:

```sh
cargo install --path .
```

On the Raspberry Pi, install the flashing tools needed for the devices physically connected to it:

```sh
cargo install probe-rs-tools
cargo install espflash
```

Use `probe-rs` for supported STM32/Nordic SWD/JTAG workflows, usually with `.elf` or `.hex` artifacts. Raw `.bin` files can work too, but the flash command normally needs a base address. Use `espflash` for ESP serial flashing workflows.

Start the server on the Pi:

```sh
farreach serve --identity .farreach/server.key --config farreach.toml
```

The Pi prints a server endpoint ID and relay URL. Keep that process running.

Build firmware on your workstation with whatever toolchain the project uses:

```sh
make
cmake --build build
cargo build --release
```

Then flash the built artifact remotely:

```sh
farreach flash \
  --identity .farreach/client.key \
  --peer <server-endpoint-id> \
  --relay-url <server-relay-url> \
  --platform stm32 \
  --firmware build/app.elf
```

The uploaded file can come from a C, C++, Rust, Zig, or other embedded codebase. `farreach` only transports the artifact and metadata; the configured flasher decides how to interpret the file.

If multiple connected boards share the same platform, specify the configured target name:

```sh
farreach flash \
  --peer <server-endpoint-id> \
  --relay-url <server-relay-url> \
  --target stm32 \
  --firmware build/app.elf
```

## Usage

Generate stable identities once:

```sh
farreach keygen --identity .farreach/server.key
farreach keygen --identity .farreach/client.key
```

Run the server on the Raspberry Pi:

```sh
farreach serve --identity .farreach/server.key --config farreach.toml
```

The server prints its endpoint ID, relay URL, and local addresses. From your workstation:

```sh
farreach flash \
  --identity .farreach/client.key \
  --peer <server-endpoint-id> \
  --relay-url <server-relay-url> \
  --platform esp \
  --firmware target/riscv32imc-unknown-none-elf/release/app.bin
```

Or run a local build first and then upload the resulting artifact:

```sh
farreach flash \
  --identity .farreach/client.key \
  --peer <server-endpoint-id> \
  --relay-url <server-relay-url> \
  --platform stm32 \
  --build-command "cargo build --release --target thumbv7em-none-eabihf" \
  --firmware target/thumbv7em-none-eabihf/release/app
```

Monitor logs:

```sh
farreach monitor \
  --identity .farreach/client.key \
  --peer <server-endpoint-id> \
  --relay-url <server-relay-url> \
  --platform esp
```

Flash, then immediately start the target monitor:

```sh
farreach flash-monitor \
  --identity .farreach/client.key \
  --peer <server-endpoint-id> \
  --relay-url <server-relay-url> \
  --platform esp \
  --firmware app.bin
```

You can override configured values from the client:

```sh
farreach flash --target esp32c3 --serial /dev/ttyACM0 --baud 921600 --chip esp32c6 ...
```
