use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "luatos-tools", version, about = "CLI tools for LuatOS modules")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Enable debug logging
    #[arg(short, long, global = true)]
    pub debug: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Generate script.bin from Lua/resource files and directories
    Script {
        /// Files and/or directories containing Lua scripts and resource files
        paths: Vec<PathBuf>,

        /// Output file path
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Burn script.bin to device after generation
        #[arg(short, long)]
        burn: bool,

        /// Base SOC file for address/chip auto-detection (required with --burn)
        #[arg(short = 'i', long)]
        base_image: Option<PathBuf>,

        /// Strip debug info from compiled Lua bytecode
        #[arg(short = 'P', long)]
        production: bool,

        /// Lua bytecode bitness for standalone script generation (32 or 64).
        /// If a base image is provided, it must match the image's script.bitw.
        #[arg(long)]
        lua_bitw: Option<u32>,

        /// Serial port (or "auto" for auto-detection)
        #[arg(short, long, default_value = "auto")]
        port: String,

        /// Port type: usb or uart
        #[arg(short = 't', long, default_value = "usb")]
        port_type: String,
    },

    /// Pack a SOC or production binpkg from base image + Lua scripts
    Pack {
        /// Files and/or directories containing Lua scripts and resource files
        paths: Vec<PathBuf>,

        /// Base SOC image
        #[arg(short = 'i', long)]
        base_image: PathBuf,

        /// Output file path
        #[arg(short, long)]
        output: PathBuf,

        /// Generate production binpkg instead of SOC
        #[arg(short = 'P', long)]
        production: bool,
    },

    /// Burn firmware (SOC/binpkg) to EC618/EC7xx module
    Burn {
        /// Path to SOC or binpkg file
        file: PathBuf,

        /// Serial port (or "auto" for auto-detection)
        #[arg(short, long, default_value = "auto")]
        port: String,

        /// Port type: usb or uart
        #[arg(short = 't', long, default_value = "usb")]
        port_type: String,

        /// Chip name for agent boot selection (e.g. ec618, ec718m)
        #[arg(short, long)]
        chip: Option<String>,

        /// Burn only specific zones (comma-separated: bl,ap,cp,script). Default: all.
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
    },

    /// Capture device serial logs
    Logs {
        /// Serial port (or "auto" for auto-detection)
        #[arg(short, long, default_value = "auto")]
        port: String,

        /// Serial baud rate
        #[arg(short, long, default_value = "2000000")]
        baud: u32,

        /// Print raw log frames as hex instead of decoding them
        #[arg(long)]
        hex: bool,
    },

    /// Development mode: stream logs, press Ctrl+B to reburn script
    Dev {
        /// Files and/or directories containing Lua scripts and resource files
        paths: Vec<PathBuf>,

        /// Base SOC file for address/chip auto-detection
        #[arg(short = 'i', long)]
        base_image: PathBuf,

        /// Serial port (or "auto" for auto-detection)
        #[arg(short, long, default_value = "auto")]
        port: String,

        /// Port type: usb or uart
        #[arg(short = 't', long, default_value = "usb")]
        port_type: String,

        /// Serial baud rate for log capture
        #[arg(short, long, default_value = "2000000")]
        baud: u32,
    },

    /// Monitor device status (version, signal, cell info, etc.)
    Monitor {
        /// Serial port (or "auto" for auto-detection)
        #[arg(short, long, default_value = "auto")]
        port: String,

        /// Serial baud rate
        #[arg(short, long, default_value = "2000000")]
        baud: u32,

        /// Stream mode: print each status update as a line instead of dashboard
        #[arg(short, long)]
        stream: bool,
    },
}
