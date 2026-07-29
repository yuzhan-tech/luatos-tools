# luatos-tools

A CLI for building, flashing, and debugging LuatOS firmware on EC618 / EC7xx modules.

[简体中文文档](./README.zh-CN.md)

## Install

### macOS / Linux (Homebrew)

```bash
brew install yuzhan-tech/tap/luatos-tools
```

### Windows

Download the latest `x86_64-pc-windows-gnu.zip` from [Releases](https://github.com/yuzhan-tech/luatos-tools/releases), extract it, and put `luatos-tools.exe` somewhere on your `PATH`.

## Quick start

Plug in your device, then start the dev loop with your Lua project and a base SOC image:

```bash
luatos-tools dev ./lua -i base.soc
```

The tool streams logs from the device. Press `Ctrl+B` to rebuild and re-burn your script without disconnecting.

## Commands

All commands auto-detect the USB serial port by default. Pass `-p <port>` to override it.

### `dev` — fast iteration

Stream logs and reburn the script with `Ctrl+B`.

```bash
luatos-tools dev ./lua -i base.soc
```

### `script` — build `script.bin`

```bash
luatos-tools script ./lua -o script.bin
```

Build and burn in one shot:

```bash
luatos-tools script ./lua -i base.soc --burn
```

Production build (strips Lua debug info):

```bash
luatos-tools script ./lua -o script.bin -P
```

### `pack` — build a full firmware image

Repack a base SOC with your Lua project:

```bash
luatos-tools pack ./lua -i base.soc -o release.soc
```

Build a production `binpkg` for distribution:

```bash
luatos-tools pack ./lua -i base.soc -o release.binpkg -P
```

### `burn` — flash firmware

```bash
luatos-tools burn firmware.soc
```

Flash only specific zones (`bl`, `ap`, `cp`, `script`):

```bash
luatos-tools burn firmware.soc --only script
luatos-tools burn firmware.soc --only bl,ap
```

### `erase` — wipe flash partitions

Erase named partitions from a base SOC/binpkg:

```bash
luatos-tools erase -i firmware.soc --partition kv
luatos-tools erase -i firmware.soc --partition fs,platconfig
```

Or erase an explicit raw flash range:

```bash
luatos-tools erase --chip ec618 --addr 0x3cc000 --size 0x10000
```

### `logs` — capture serial logs

```bash
luatos-tools logs
```

Print raw frames as hex:

```bash
luatos-tools logs --hex
```

### `unilog` — low-level EigenComm logs

Decode the binary UniLog stream with an SDK `comdb.txt`, or load that file
directly from a LuatOS SOC image:

```bash
luatos-tools unilog --comdb comdb.txt
luatos-tools unilog -i firmware.soc
```

The live UniLog port is auto-detected on LuatOS USB interface 5. Capture raw
bytes for later replay, or inspect records without a dictionary:

```bash
luatos-tools unilog -i firmware.soc --out capture.bin
luatos-tools unilog --file capture.bin -i firmware.soc
luatos-tools unilog --raw
```

Use `--owner`, `--module`, `--sub`, and `--level` to filter decoded records.
This is the low-level EigenComm UniLog channel; `logs`, `monitor`, and `dev`
continue to use the separate LuatOS formatted-log channel.

### `monitor` — device status

Live dashboard:

```bash
luatos-tools monitor
```

Stream updates line by line:

```bash
luatos-tools monitor --stream
```

## Troubleshooting

**Port open failure.** Close any other serial monitor or flashing tool, then retry.

**Burn fails mid-transfer.** Use a direct USB connection (avoid hubs) and check power stability.

**Chip detection failure.** Pass `--chip` explicitly, e.g. `--chip ec618` or `--chip ec718m`.

**Auto-detect picks the wrong port.** Override with `-p`, e.g. `-p /dev/tty.usbserial-XXXX` (macOS/Linux) or `-p COM5` (Windows).

## Build from source

Requires the Rust stable toolchain. On Linux you also need `libudev-dev` and `pkg-config`.

```bash
cargo build --release
./target/release/luatos-tools --help
```

## License

[MIT](./LICENSE)
