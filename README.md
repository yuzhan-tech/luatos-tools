# luatos-tools

`luatos-tools` is a Rust CLI for LuatOS firmware packaging, script deployment, flashing, and serial monitoring on EC618 and EC7xx modules.

[简体中文文档](./README.zh-CN.md)

## Features

- Build `script.bin` from Lua files and bundled resources.
- Compile `.lua` sources to Lua bytecode automatically and optionally strip debug info for production.
- Infer Lua bytecode bitness from a base SOC image when one is provided.
- Repack a base `.soc` image with a new `script.bin`.
- Generate production `.binpkg` output from a base SOC image.
- Burn `.soc` and `.binpkg` images to supported modules.
- Burn only selected zones with `--only bl,ap,cp,script`.
- Auto-detect serial ports with `-p auto`.
- Capture serial logs in decoded or raw hex form.
- Show a live device status dashboard or stream status events.
- Run a fast development loop that streams logs and re-burns scripts with `Ctrl+B`.

## Requirements

- Rust stable toolchain
- USB access to a supported LuatOS device
- A base SOC image when using `pack`, `dev`, or `script --burn`

## Usage

Build and run with Cargo:

```bash
cargo run -- <command> [args]
```

Or use the compiled binary:

```bash
./target/debug/luatos-tools <command> [args]
```

## Commands

### `script`

Generate `script.bin` from one or more files or directories.

```bash
luatos-tools script ./lua -o script.bin
```

Compile standalone 64-bit Lua bytecode:

```bash
luatos-tools script ./lua -o script.bin --lua-bitw 64
```

Build a production script bundle with stripped Lua debug info:

```bash
luatos-tools script ./lua -o script.bin -P
```

Build and immediately burn the generated script:

```bash
luatos-tools script ./lua --burn --base-image ./base.soc --port auto --port-type usb
```

Behavior:

- `.lua` files are compiled to `.luac`.
- Non-Lua files are packed as-is into the luadb payload.
- Without `--base-image`, script generation defaults to standalone bytecode rules and can be forced with `--lua-bitw 32|64`.
- With `--base-image`, the tool reads the image metadata and requires any explicit `--lua-bitw` value to match the image.
- `--burn` requires `--base-image`.

### `pack`

Create a new firmware package from a base SOC image plus your Lua/resources.

Create a repacked SOC:

```bash
luatos-tools pack ./lua --base-image ./base.soc --output ./out.soc
```

Create a production `binpkg`:

```bash
luatos-tools pack ./lua --base-image ./base.soc --output ./out.binpkg -P
```

Notes:

- `pack` reads script metadata from the base image and compiles Lua with matching bitness automatically.
- Use SOC output for a repacked firmware image.
- Use `-P` for production `binpkg` output.

### `burn`

Burn a SOC or `binpkg` image to a device.

Burn the full image:

```bash
luatos-tools burn ./firmware.soc --port auto --port-type usb
```

Burn only selected zones:

```bash
luatos-tools burn ./firmware.soc --only bl,ap
luatos-tools burn ./firmware.soc --only script
```

Specify the chip explicitly when needed:

```bash
luatos-tools burn ./firmware.soc --chip ec718m
```

Notes:

- Supported zone names are `bl`, `ap`, `cp`, and `script`.
- If chip detection is ambiguous, pass `--chip` instead of relying on inference.

### `logs`

Capture serial logs from the device.

```bash
luatos-tools logs --port auto --baud 2000000
```

Print raw frames as hex:

```bash
luatos-tools logs --port auto --baud 2000000 --hex
```

### `monitor`

Show device status information such as firmware, signal, and cell status.

Dashboard mode:

```bash
luatos-tools monitor --port auto --baud 2000000
```

Stream updates line by line:

```bash
luatos-tools monitor --port auto --baud 2000000 --stream
```

### `dev`

Run a serial log session with fast script reburn support.

```bash
luatos-tools dev ./lua --base-image ./base.soc --port auto --port-type usb --baud 2000000
```

During `dev` mode:

- The tool streams logs continuously.
- Press `Ctrl+B` to rebuild the script bundle and burn the updated script.
- The base image is used for script layout and device burn metadata.

## Common Workflows

Fast script iteration:

```bash
luatos-tools dev ./lua --base-image ./base.soc --port auto --port-type usb --baud 2000000
```

One-shot script build and burn:

```bash
luatos-tools script ./lua --burn --base-image ./base.soc --port auto --port-type usb
```

Produce a distributable SOC:

```bash
luatos-tools pack ./lua --base-image ./base.soc --output ./release.soc
```

Produce a distributable production `binpkg`:

```bash
luatos-tools pack ./lua --base-image ./base.soc --output ./release.binpkg -P
```

## Troubleshooting

`Timeout waiting for USB device`:

- Reconnect the device and retry with `--port auto`.
- Check that the cable carries data, not only power.
- Put the module into boot or download mode manually if auto-switching fails.

Port open failures:

- Close any other serial monitors or flashing tools.
- Re-run with the correct `--port-type` if the wrong interface was chosen.

Burn failures mid-transfer:

- Retry with a direct USB connection.
- Check power stability and avoid flaky hubs.

Chip detection failures:

- Pass `--chip ec618`, `--chip ec718m`, or the correct target explicitly.

## Build

Build the project:

```bash
cargo build
```

Build an optimized release binary:

```bash
cargo build --release
```

## Developer Notes

Run tests:

```bash
cargo test
```

Run clippy:

```bash
cargo clippy --all-targets --all-features
```

Check command help while editing the CLI:

```bash
cargo run -- --help
```

Implementation notes:

- The project vendors Lua 5.3.6 sources and builds the Lua compiler helper through `build.rs`.
- Agent boot binaries used during flashing are stored in `agentboot/`.
