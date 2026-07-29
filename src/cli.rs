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

    /// Erase flash partitions or an explicit flash range
    Erase {
        /// Base SOC/binpkg image for chip and named partition lookup
        #[arg(short = 'i', long)]
        base_image: Option<PathBuf>,

        /// Named partitions to erase (comma-separated: fs,kv,platconfig)
        #[arg(long, value_delimiter = ',')]
        partition: Vec<String>,

        /// Raw flash erase start address, e.g. 0x3cc000
        #[arg(long)]
        addr: Option<String>,

        /// Erase size in bytes, e.g. 0x10000
        #[arg(long)]
        size: Option<String>,

        /// Serial port (or "auto" for auto-detection)
        #[arg(short, long, default_value = "auto")]
        port: String,

        /// Port type: usb or uart
        #[arg(short = 't', long, default_value = "usb")]
        port_type: String,

        /// Chip name for agent boot selection (e.g. ec618, ec718m)
        #[arg(short, long)]
        chip: Option<String>,
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

    /// Reboot the module normally (diag reboot over the log port, like Dev's Ctrl-R)
    Reboot {
        /// Serial port (or "auto" for auto-detection)
        #[arg(short, long, default_value = "auto")]
        port: String,
    },

    /// Capture, replay, and decode the low-level EigenComm UniLog stream
    Unilog {
        /// Serial port (or "auto" for LuatOS USB interface 5)
        #[arg(short, long, default_value = "auto")]
        port: String,

        /// comdb.txt produced by the EC7xx SDK PrePass tool
        #[arg(short, long)]
        comdb: Option<PathBuf>,

        /// LuatOS SOC image or extracted directory containing comdb.txt
        #[arg(short = 'i', long)]
        base_image: Option<PathBuf>,

        /// Print raw records without a comdb
        #[arg(long)]
        raw: bool,

        /// Show undecodable PHY records
        #[arg(long)]
        phy: bool,

        /// Filter owners by name substring or numeric ID
        #[arg(long, value_delimiter = ',')]
        owner: Vec<String>,

        /// Filter modules by name substring or numeric ID
        #[arg(long, value_delimiter = ',')]
        module: Vec<String>,

        /// Filter sites by name substring or numeric ID
        #[arg(long, value_delimiter = ',')]
        sub: Vec<String>,

        /// Filter levels (DBG, INF, VAL, SIG, WRN, ERR)
        #[arg(long, value_delimiter = ',')]
        level: Vec<String>,

        /// Replay a raw capture or EPAT RecvDump file
        #[arg(short = 'f', long)]
        file: Option<PathBuf>,

        /// Save raw captured bytes for later replay
        #[arg(short, long)]
        out: Option<PathBuf>,

        /// Append to --out instead of refusing an existing file
        #[arg(long)]
        append: bool,

        /// Compare the device log version with the selected comdb
        #[arg(long)]
        version_check: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_unilog_adapter_options() {
        let cli = Cli::try_parse_from([
            "luatos-tools",
            "unilog",
            "--base-image",
            "firmware.soc",
            "--owner",
            "RRC,3",
            "--level",
            "WRN,ERR",
            "--out",
            "capture.bin",
        ])
        .unwrap();

        match cli.command {
            Commands::Unilog {
                port,
                base_image,
                owner,
                level,
                out,
                ..
            } => {
                assert_eq!(port, "auto");
                assert_eq!(base_image, Some(PathBuf::from("firmware.soc")));
                assert_eq!(owner, ["RRC", "3"]);
                assert_eq!(level, ["WRN", "ERR"]);
                assert_eq!(out, Some(PathBuf::from("capture.bin")));
            }
            _ => panic!("expected unilog command"),
        }
    }
}
