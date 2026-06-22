# far-reach

Rust CLI for using a Raspberry Pi or similar bench host as a remote embedded debugging station.

The bench host owns the local USB/JTAG/UART connections. Your workstation builds the firmware locally, connects to the bench host over Iroh, uploads the built artifact, asks the bench host to run a configured flashing command, and streams flash output or target logs back over the encrypted QUIC session.

## Current shape

- Transport: Iroh endpoint identity, relay/hole-punching, custom ALPN.
- Agent: runs on the Raspberry Pi next to the ESP/STM32/Nordic device.
- Client: runs on your workstation.
- Flash/log backend: command templates configured per target, so existing tools do chip-specific work.
- Build policy: firmware is always built on the workstation. The agent receives bytes and flashes; it does not run project builds.

This keeps the first implementation practical:

- ESP: `espflash`
- STM32: `probe-rs` or `openocd`
- Nordic: `probe-rs`, `nrfutil`, or `west flash`

## Agent config

Create `far-reach.toml` on the Raspberry Pi:

```toml
# Optional. If non-empty, only these Iroh endpoint IDs can connect.
authorized_peers = []

# Optional. Firmware uploads are staged here while flash commands run.
work_dir = ".far-reach/work"

[targets.esp32c3]
serial = "/dev/ttyUSB0"
baud = 115200
chip = "esp32c3"
flash = ["espflash", "flash", "--port", "{serial}", "{firmware}"]
monitor = ["espflash", "monitor", "--port", "{serial}", "--baud", "{baud}"]

[targets.stm32]
chip = "STM32F407VGTx"
flash = ["probe-rs", "download", "--chip", "{chip}", "{firmware}"]
monitor = ["probe-rs", "attach", "--chip", "{chip}"]

[targets.nrf52]
chip = "nRF52840_xxAA"
flash = ["probe-rs", "download", "--chip", "{chip}", "{firmware}"]
monitor = ["probe-rs", "attach", "--chip", "{chip}"]
```

Supported template values are `{target}`, `{firmware}`, `{firmware_name}`, `{serial}`, `{baud}`, and `{chip}`.

## Usage

Generate stable identities once:

```sh
far-reach keygen --identity .far-reach/agent.key
far-reach keygen --identity .far-reach/client.key
```

Run the agent on the Raspberry Pi:

```sh
far-reach agent --identity .far-reach/agent.key --config far-reach.toml
```

The agent prints its endpoint ID, relay URL, and local addresses. From your workstation:

```sh
far-reach flash \
  --identity .far-reach/client.key \
  --peer <agent-endpoint-id> \
  --relay-url <agent-relay-url> \
  --target esp32c3 \
  --firmware target/riscv32imc-unknown-none-elf/release/app.bin
```

Or run a local build first and then upload the resulting artifact:

```sh
far-reach flash \
  --identity .far-reach/client.key \
  --peer <agent-endpoint-id> \
  --relay-url <agent-relay-url> \
  --target stm32 \
  --build-command "cargo build --release --target thumbv7em-none-eabihf" \
  --firmware target/thumbv7em-none-eabihf/release/app
```

Monitor logs:

```sh
far-reach monitor \
  --identity .far-reach/client.key \
  --peer <agent-endpoint-id> \
  --relay-url <agent-relay-url> \
  --target esp32c3
```

Flash, then immediately start the target monitor:

```sh
far-reach flash-monitor \
  --identity .far-reach/client.key \
  --peer <agent-endpoint-id> \
  --relay-url <agent-relay-url> \
  --target esp32c3 \
  --firmware app.bin
```

You can override configured values from the client:

```sh
far-reach flash --target esp32c3 --serial /dev/ttyACM0 --baud 921600 --chip esp32c6 ...
```
