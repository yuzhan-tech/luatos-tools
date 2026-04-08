# luatos-tools

`luatos-tools` 是一个 Rust 编写的 CLI 工具，用于 LuatOS 在 EC618/EC7xx 模组上的固件打包、脚本部署、烧录和串口监控。

## 功能特性

- 将 Lua 脚本与资源文件打包为 `script.bin`
- 自动将 `.lua` 编译为字节码，并支持生产模式去除调试信息
- 在提供基础 SOC 镜像时，自动推断并匹配 Lua 字节码位宽
- 基于基础 `.soc` 镜像重打包新的 `script.bin`
- 从基础 SOC 生成生产发布用 `.binpkg`
- 烧录 `.soc` 和 `.binpkg` 到支持的模组
- 支持按分区烧录（`--only bl,ap,cp,script`）
- 支持 `-p auto` 自动识别串口
- 支持串口日志解码输出或原始十六进制输出
- 提供实时设备状态面板或事件流输出
- 开发模式下可持续看日志，并通过 `Ctrl+B` 快速重烧脚本

## 环境要求

- Rust stable 工具链
- 可访问的 USB 设备（目标 LuatOS 模组）
- 使用 `pack`、`dev` 或 `script --burn` 时需要基础 SOC 镜像

## 快速开始

使用 Cargo 运行：

```bash
cargo run -- <command> [args]
```

或使用编译后的二进制：

```bash
./target/debug/luatos-tools <command> [args]
```

## 主要命令

### `script`

从一个或多个文件/目录生成 `script.bin`：

```bash
luatos-tools script ./lua -o script.bin
```

编译独立 64 位 Lua 字节码：

```bash
luatos-tools script ./lua -o script.bin --lua-bitw 64
```

生成生产包（去除 Lua 调试信息）：

```bash
luatos-tools script ./lua -o script.bin -P
```

生成后立即烧录：

```bash
luatos-tools script ./lua --burn --base-image ./base.soc --port auto --port-type usb
```

### `pack`

基于基础 SOC 镜像和 Lua 资源打包：

```bash
luatos-tools pack ./lua --base-image ./base.soc --output ./out.soc
```

输出生产用 `binpkg`：

```bash
luatos-tools pack ./lua --base-image ./base.soc --output ./out.binpkg -P
```

### `burn`

烧录 SOC 或 `binpkg`：

```bash
luatos-tools burn ./firmware.soc --port auto --port-type usb
```

只烧录指定分区：

```bash
luatos-tools burn ./firmware.soc --only bl,ap
luatos-tools burn ./firmware.soc --only script
```

### `logs`

串口日志采集：

```bash
luatos-tools logs --port auto --baud 2000000
```

十六进制原始输出：

```bash
luatos-tools logs --port auto --baud 2000000 --hex
```

### `monitor`

设备状态监控：

```bash
luatos-tools monitor --port auto --baud 2000000
```

事件流输出：

```bash
luatos-tools monitor --port auto --baud 2000000 --stream
```

### `dev`

开发循环（日志 + 快速重烧）：

```bash
luatos-tools dev ./lua --base-image ./base.soc --port auto --port-type usb --baud 2000000
```

运行 `dev` 时：

- 持续输出设备日志
- 按 `Ctrl+B` 重新构建并烧录脚本
- 使用基础镜像中的脚本布局和烧录元数据

## 常见流程

快速迭代脚本：

```bash
luatos-tools dev ./lua --base-image ./base.soc --port auto --port-type usb --baud 2000000
```

一次性构建并烧录脚本：

```bash
luatos-tools script ./lua --burn --base-image ./base.soc --port auto --port-type usb
```

打包发布 SOC：

```bash
luatos-tools pack ./lua --base-image ./base.soc --output ./release.soc
```

打包发布生产 `binpkg`：

```bash
luatos-tools pack ./lua --base-image ./base.soc --output ./release.binpkg -P
```

## 构建与开发

构建项目：

```bash
cargo build
```

发布构建：

```bash
cargo build --release
```

运行测试：

```bash
cargo test
```

运行 clippy：

```bash
cargo clippy --all-targets --all-features
```

查看命令帮助：

```bash
cargo run -- --help
```

## License

本项目使用 [MIT License](./LICENSE)。
