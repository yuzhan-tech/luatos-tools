# luatos-tools

LuatOS 在 EC618 / EC7xx 模组上的固件构建、烧录与调试 CLI 工具。

[English](./README.md)

## 安装

### macOS / Linux（Homebrew）

```bash
brew install yuzhan-tech/tap/luatos-tools
```

### Windows

从 [Releases](https://github.com/yuzhan-tech/luatos-tools/releases) 下载最新的 `x86_64-pc-windows-gnu.zip`，解压后将 `luatos-tools.exe` 放入 `PATH` 即可使用。

## 快速开始

接好设备，使用基础 SOC 镜像启动开发循环：

```bash
luatos-tools dev ./lua -i base.soc
```

工具会持续输出设备日志。按 `Ctrl+B` 即可重新构建并烧录脚本，无需重连设备。

## 命令

所有命令默认通过 USB 自动识别串口。如需手动指定，使用 `-p <port>` 参数。

### `dev` — 快速迭代

实时日志 + `Ctrl+B` 重烧脚本：

```bash
luatos-tools dev ./lua -i base.soc
```

### `script` — 构建 `script.bin`

```bash
luatos-tools script ./lua -o script.bin
```

构建后立即烧录：

```bash
luatos-tools script ./lua -i base.soc --burn
```

生产模式（去除 Lua 调试信息）：

```bash
luatos-tools script ./lua -o script.bin -P
```

### `pack` — 打包完整固件镜像

基于基础 SOC 重打包：

```bash
luatos-tools pack ./lua -i base.soc -o release.soc
```

生成发布用 `binpkg`：

```bash
luatos-tools pack ./lua -i base.soc -o release.binpkg -P
```

### `burn` — 烧录固件

```bash
luatos-tools burn firmware.soc
```

只烧录指定分区（可选 `bl`、`ap`、`cp`、`script`）：

```bash
luatos-tools burn firmware.soc --only script
luatos-tools burn firmware.soc --only bl,ap
```

### `logs` — 串口日志

```bash
luatos-tools logs
```

十六进制原始帧输出：

```bash
luatos-tools logs --hex
```

### `monitor` — 设备状态

实时面板：

```bash
luatos-tools monitor
```

事件流模式：

```bash
luatos-tools monitor --stream
```

## 常见问题

**串口被占用。** 关闭其他串口监控或烧录工具后重试。

**烧录中途失败。** 使用直连 USB（避免使用 USB Hub），并确认供电稳定。

**芯片识别失败。** 显式指定 `--chip`，例如 `--chip ec618` 或 `--chip ec718m`。

**自动识别选错串口。** 使用 `-p` 手动指定，例如 `-p /dev/tty.usbserial-XXXX`（macOS/Linux）或 `-p COM5`（Windows）。

## 从源码构建

需要 Rust stable 工具链。Linux 上还需安装 `libudev-dev` 和 `pkg-config`。

```bash
cargo build --release
./target/release/luatos-tools --help
```

## License

[MIT](./LICENSE)
