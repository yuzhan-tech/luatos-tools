use regex::Regex;

/// EARFCN-to-band lookup entry: (band_name, dl_earfcn_low, dl_earfcn_high)
static EARFCN_BANDS: &[(&str, u32, u32)] = &[
    ("B1", 0, 599),
    ("B2", 600, 1199),
    ("B3", 1200, 1949),
    ("B4", 1950, 2399),
    ("B5", 2400, 2649),
    ("B7", 2750, 3449),
    ("B8", 3450, 3799),
    ("B9", 3800, 4149),
    ("B10", 4150, 4749),
    ("B11", 4750, 4949),
    ("B12", 5010, 5179),
    ("B13", 5180, 5279),
    ("B14", 5280, 5379),
    ("B17", 5730, 5849),
    ("B18", 5850, 5999),
    ("B19", 6000, 6149),
    ("B20", 6150, 6449),
    ("B21", 6450, 6599),
    ("B22", 6600, 7399),
    ("B24", 7700, 8039),
    ("B25", 8040, 8689),
    ("B26", 8690, 9039),
    ("B27", 9040, 9209),
    ("B28", 9210, 9659),
    ("B29", 9660, 9769),
    ("B30", 9770, 9869),
    ("B31", 9870, 9919),
    ("B32", 9920, 10359),
    ("B33", 36000, 36199),
    ("B34", 36200, 36349),
    ("B35", 36350, 36949),
    ("B36", 36950, 37549),
    ("B37", 37550, 37749),
    ("B38", 37750, 38249),
    ("B39", 38250, 38649),
    ("B40", 38650, 39649),
    ("B41", 39650, 41589),
    ("B42", 41590, 43589),
    ("B43", 43590, 45589),
    ("B44", 45590, 46589),
    ("B45", 46590, 46789),
    ("B46", 46790, 54539),
    ("B47", 54540, 55239),
    ("B48", 55240, 56739),
    ("B49", 56740, 58239),
    ("B50", 58240, 59089),
    ("B51", 59090, 59139),
    ("B52", 59140, 60139),
    ("B53", 60140, 60254),
    ("B65", 65536, 66435),
    ("B66", 66436, 67335),
    ("B67", 67336, 67535),
    ("B68", 67536, 67835),
    ("B69", 67836, 68335),
    ("B70", 68336, 68585),
    ("B71", 68586, 68935),
    ("B72", 68936, 68985),
    ("B73", 68986, 69035),
    ("B74", 69036, 69465),
    ("B75", 69466, 70315),
    ("B76", 70316, 70365),
    ("B85", 70366, 70545),
    ("B87", 70546, 70595),
    ("B88", 70596, 70645),
];

fn earfcn_to_band(earfcn: u32) -> Option<&'static str> {
    for &(band, lo, hi) in EARFCN_BANDS {
        if earfcn >= lo && earfcn <= hi {
            return Some(band);
        }
    }
    None
}

fn format_reg_status(stat: u8) -> &'static str {
    match stat {
        0 => "Not registered",
        1 => "Registered, home",
        2 => "Searching",
        3 => "Denied",
        4 => "Unknown",
        5 => "Registered, roaming",
        6 => "Registered, SMS only",
        7 => "Registered, SMS only, roaming",
        8 => "Emergency only",
        9 => "Registered (not recommended)",
        10 => "Registered, roaming (not recommended)",
        _ => "Unknown",
    }
}

fn decode_soccell_plmn_field(raw: &str) -> String {
    let trimmed_f = raw.trim_matches(|c| c == 'f' || c == 'F');
    let trimmed_zeros = trimmed_f.trim_start_matches('0');
    if trimmed_zeros.is_empty() {
        "0".to_string()
    } else {
        trimmed_zeros.to_string()
    }
}

fn format_poweron_reason(code: u8) -> &'static str {
    match code {
        0 => "Power key",
        1 => "Charging/AT",
        2 => "Alarm",
        3 => "Software restart",
        4 => "Unknown",
        5 => "Reset key",
        6 => "Abnormal restart",
        7 => "Tool restart",
        8 => "Watchdog",
        9 => "External reset",
        10 => "Charging",
        _ => "Unknown",
    }
}

#[derive(Default)]
pub struct DeviceStatus {
    pub version: Option<String>,
    pub imei: Option<String>,
    pub fw_info: Option<String>,
    pub hw_info: Option<String>,
    pub sim: Option<String>,
    pub reg_status: Option<String>,
    pub gprs: Option<String>,
    pub data: Option<String>,
    pub net_type: Option<String>,
    pub mcc: Option<String>,
    pub mnc: Option<String>,
    pub pci: Option<u32>,
    pub cell_id: Option<u32>,
    pub earfcn: Option<u32>,
    pub band: Option<String>,
    pub csq: Option<u8>,
    pub rsrp: Option<i32>,
    pub rsrq: Option<i32>,
    pub snr: Option<i32>,
    pub lua_total: Option<u32>,
    pub lua_used: Option<u32>,
    pub psram_total: Option<u32>,
    pub psram_used: Option<u32>,
    pub boot_reason: Option<String>,
    pub boot_version: Option<String>,
    pub error_count: Option<u32>,
    pub wifi_fw: Option<String>,
    pub uart_boot: bool,
}

impl DeviceStatus {
    pub fn display(&self) {
        // Clear screen and move cursor to top
        print!("\x1b[2J\x1b[H");

        println!("=== Device Monitor ===");
        println!();

        Self::print_field("Module", &self.hw_info);
        Self::print_field("Firmware", &self.version);
        Self::print_field("Poweron", &self.boot_reason);
        Self::print_field("SIM", &self.sim);
        Self::print_field("Reg", &self.reg_status);
        Self::print_field("Net Type", &self.net_type);

        if self.mcc.is_some() || self.cell_id.is_some() {
            let mcc = self.mcc.as_deref().unwrap_or("0");
            let mnc = self.mnc.as_deref().unwrap_or("0");
            let pci = self.pci.map(|v| v.to_string()).unwrap_or("0".into());
            let cid = self.cell_id.map(|v| v.to_string()).unwrap_or("0".into());
            let earfcn = self.earfcn.map(|v| v.to_string()).unwrap_or("0".into());
            let band = self.band.as_deref().unwrap_or("?");
            println!(
                "{:<12}{} {} {} {} earfcn:{} band:{}",
                "Cell ID:", mcc, mnc, pci, cid, earfcn, band
            );
        } else {
            Self::print_field("Cell ID", &None::<String>);
        }

        if let Some(csq) = self.csq {
            println!("{:<12}{}", "CSQ:", csq);
        } else {
            println!("{:<12}-", "CSQ:");
        }
        let rsrp = self
            .rsrp
            .map(|v| format!("{}dBm", v))
            .unwrap_or_else(|| "-".to_string());
        let rsrq = self
            .rsrq
            .map(|v| format!("{}dB", v))
            .unwrap_or_else(|| "-".to_string());
        let snr = self
            .snr
            .map(|v| format!("SNR {}dB", v))
            .unwrap_or_else(|| "SNR -".to_string());
        println!("{:<12}{}, {} {}", "Signal:", rsrp, rsrq, snr);

        let lua_mem = self
            .lua_total
            .map(|total| {
                format!(
                    "{}/{}KB(lua)",
                    self.lua_used.unwrap_or(0) / 1024,
                    total / 1024
                )
            })
            .unwrap_or_else(|| "-(lua)".to_string());
        let sys_mem = self
            .psram_total
            .map(|total| {
                format!(
                    "{}/{}KB(sys)",
                    self.psram_used.unwrap_or(0) / 1024,
                    total / 1024
                )
            })
            .unwrap_or_else(|| "-(sys)".to_string());
        println!("{:<12}{} {}", "Memory:", lua_mem, sys_mem);
        if let Some(cnt) = self.error_count {
            if cnt > 0 {
                println!("{:<12}{}", "Errors:", cnt);
            }
        }
        if self.uart_boot {
            println!("{:<12}Complete", "UART Boot:");
        }
    }

    fn print_field<T: std::fmt::Display>(label: &str, value: &Option<T>) {
        let label_fmt = format!("{}:", label);
        match value {
            Some(v) => println!("{:<12}{}", label_fmt, v),
            None => println!("{:<12}-", label_fmt),
        }
    }
}

pub struct StatusParser {
    re_baseinfo: Regex,
    re_socsq: Regex,
    re_csq: Regex,
    re_soccell: Regex,
    re_socreg: Regex,
    re_cereg: Regex,
    re_cgatt: Regex,
    re_mem: Regex,
    re_poweron: Regex,
    pub status: DeviceStatus,
}

impl StatusParser {
    pub fn new() -> Self {
        StatusParser {
            // BASEINFO:<IMEI>,<FIRMWARE> or BASEINFO:<IMEI>
            re_baseinfo: Regex::new(r"BASEINFO:(\S*?),(\S+)").unwrap(),
            re_socsq: Regex::new(r"\+SOCSQ: (\S+),(\S+),(\S+)").unwrap(),
            re_csq: Regex::new(r"\+CSQ:.* (\d+)").unwrap(),
            re_soccell: Regex::new(r"\+SOCCELL: ([[:xdigit:]]+),([[:xdigit:]]+),(\d+),(\d+),(\d+)")
                .unwrap(),
            re_socreg: Regex::new(r"\+SOCREG: (\d+),(\S+)").unwrap(),
            re_cereg: Regex::new(r"\+CEREG: \d+,(\d+)").unwrap(),
            re_cgatt: Regex::new(r"\+CGATT: (\d)").unwrap(),
            // +MEM: <type> <timestamp> <unknown> <total> <used>
            re_mem: Regex::new(r"\+MEM: (\S+)\s+\d+\s+\d+\s+(\d+)\s+(\d+)").unwrap(),
            // soc poweron: <reason> [<version> <error_count>]
            // With 3 values: reason, version, error_count
            // With 2 values: reason, error_count (no version)
            re_poweron: Regex::new(r"soc poweron:\D*(\d+)(?:\s+(\S+)\s+(\d+))?").unwrap(),
            status: DeviceStatus::default(),
        }
    }

    /// Check if a message is a status message (without mutating state).
    pub fn is_status(msg: &str) -> bool {
        const PREFIXES: &[&str] = &[
            "BASEINFO:",
            "+SOCSQ:",
            "+CSQ:",
            "+SOCCELL:",
            "+SOCREG:",
            "+CEREG:",
            "+CPIN:",
            "+CGATT:",
            "+PDP:",
            "+MEM:",
            "+FW:",
            "+HW:",
            "+WIFI_FW:",
            "+CGEV: ME PDN ACT",
            "+NETSTAT:",
            "+NETDRV:",
        ];
        const CONTAINS: &[&str] = &[
            "soc poweron:",
            "UART Boot Completed",
            "net.NET_UPD_NET_MODE",
            "+E_UTRAN",
        ];
        for p in PREFIXES {
            if msg.starts_with(p) {
                return true;
            }
        }
        for c in CONTAINS {
            if msg.contains(c) {
                return true;
            }
        }
        false
    }

    /// Feed a text message from the trace stream. Returns true if status changed.
    pub fn feed(&mut self, msg: &str) -> bool {
        let mut changed = false;

        // BASEINFO:<IMEI>,<FIRMWARE> or BASEINFO:<IMEI> (no comma)
        if msg.starts_with("BASEINFO:") {
            let payload = &msg[9..];
            if let Some(caps) = self.re_baseinfo.captures(msg) {
                // Has comma: BASEINFO:<IMEI>,<FIRMWARE>
                let field1 = &caps[1];
                let field2 = &caps[2];
                if !field1.is_empty() {
                    self.status.imei = Some(field1.to_string());
                }
                self.status.version = Some(field2.to_string());
            } else if !payload.is_empty() {
                // No comma: BASEINFO:<IMEI>
                self.status.imei = Some(payload.trim().to_string());
            }
            changed = true;
        }

        if let Some(caps) = self.re_socsq.captures(msg) {
            self.status.rsrp = caps[1].parse().ok();
            self.status.rsrq = caps[2].parse().ok();
            self.status.snr = caps[3].parse().ok();
            changed = true;
        }

        if msg.contains("+CSQ:") {
            if let Some(caps) = self.re_csq.captures(msg) {
                if let Ok(v) = caps[1].parse::<u8>() {
                    if v != 99 {
                        self.status.csq = Some(v);
                    } else {
                        self.status.csq = None; // 99 = unknown
                    }
                    changed = true;
                }
            }
        }

        if let Some(caps) = self.re_soccell.captures(msg) {
            let mcc = decode_soccell_plmn_field(&caps[1]);
            let mnc = decode_soccell_plmn_field(&caps[2]);
            let earfcn: u32 = caps[3].parse().unwrap_or(0);
            let pci: u32 = caps[4].parse().unwrap_or(0);
            let cid: u32 = caps[5].parse().unwrap_or(0);

            self.status.mcc = Some(mcc);
            self.status.mnc = Some(mnc);
            self.status.earfcn = Some(earfcn);
            self.status.pci = Some(pci);
            self.status.cell_id = Some(cid);
            self.status.band = earfcn_to_band(earfcn).map(|s| s.to_string());
            changed = true;
        }

        if let Some(caps) = self.re_socreg.captures(msg) {
            if let Ok(stat) = caps[1].parse::<u8>() {
                self.status.reg_status = Some(format_reg_status(stat).to_string());
                changed = true;
            }
        } else if let Some(caps) = self.re_cereg.captures(msg) {
            if let Ok(stat) = caps[1].parse::<u8>() {
                self.status.reg_status = Some(format_reg_status(stat).to_string());
                changed = true;
            }
        }

        if msg.contains("+CPIN: READY") {
            self.status.sim = Some("Ready".to_string());
            changed = true;
        } else if msg.contains("+CPIN:") {
            self.status.sim = Some("Not ready".to_string());
            changed = true;
        }

        if let Some(caps) = self.re_cgatt.captures(msg) {
            self.status.gprs = Some(if &caps[1] == "1" {
                "Attached".to_string()
            } else {
                "Detached".to_string()
            });
            changed = true;
        }

        if msg.contains("+PDP: OK") {
            self.status.data = Some("Connected".to_string());
            changed = true;
        } else if msg.contains("+PDP:") {
            self.status.data = Some("Disconnected".to_string());
            changed = true;
        }

        // +MEM: <type> <timestamp> <unknown> <total> <used>
        if let Some(caps) = self.re_mem.captures(msg) {
            let mem_type = &caps[1];
            let total: Option<u32> = caps[2].parse().ok();
            let used: Option<u32> = caps[3].parse().ok();
            match mem_type {
                "LUA" => {
                    self.status.lua_total = total;
                    self.status.lua_used = used;
                }
                "PSRAM" => {
                    self.status.psram_total = total;
                    self.status.psram_used = used;
                }
                _ => {}
            }
            changed = true;
        }

        // soc poweron: <reason> [<version> <error_count>]
        if let Some(caps) = self.re_poweron.captures(msg) {
            if let Ok(code) = caps[1].parse::<u8>() {
                self.status.boot_reason = Some(format_poweron_reason(code).to_string());
            }
            if let Some(ver) = caps.get(2) {
                let v = ver.as_str();
                self.status.boot_version = Some(v.to_string());
                self.status.version = Some(v.to_string());
            }
            if let Some(ec) = caps.get(3) {
                self.status.error_count = ec.as_str().parse().ok();
            }
            changed = true;
        }

        if msg.starts_with("+FW:") {
            let parts: Vec<&str> = msg.split(' ').collect();
            if parts.len() > 3 {
                let fw_ver = parts[3];
                self.status.fw_info = Some(fw_ver.to_string());
                // Append to existing version (from soc poweron), or use fw_ver alone
                match self.status.version {
                    Some(ref mut ver) if !ver.is_empty() => {
                        if ver.ends_with('_') {
                            ver.push_str(fw_ver);
                        } else {
                            ver.push('_');
                            ver.push_str(fw_ver);
                        }
                    }
                    _ => {
                        self.status.version = Some(fw_ver.to_string());
                    }
                }
            } else {
                self.status.fw_info = Some(msg[4..].trim_start().to_string());
            }
            changed = true;
        }

        if msg.starts_with("+HW:") {
            let parts: Vec<&str> = msg.split(' ').collect();
            if parts.len() > 1 && !parts[1].is_empty() {
                self.status.hw_info = Some(parts[1].to_string());
            }
            changed = true;
        }

        // +WIFI_FW: handler
        if msg.starts_with("+WIFI_FW:") {
            self.status.wifi_fw = Some(msg[9..].trim_start().to_string());
            changed = true;
        }

        // UART Boot Completed
        if msg.contains("UART Boot Completed") {
            self.status.uart_boot = true;
            changed = true;
        }

        // +CGEV: ME PDN ACT — data connected
        if msg.contains("+CGEV: ME PDN ACT") {
            self.status.data = Some("Connected".to_string());
            changed = true;
        }

        // Network type: net.NET_UPD_NET_MODE.<n> or +E_UTRAN Service
        if msg.contains("net.NET_UPD_NET_MODE") {
            if msg.ends_with(".4") {
                self.status.net_type = Some("4G".to_string());
            } else if msg.ends_with(".3") {
                self.status.net_type = Some("3G".to_string());
            } else if msg.ends_with(".2") {
                self.status.net_type = Some("2G".to_string());
            }
            changed = true;
        } else if msg.contains("+E_UTRAN") {
            self.status.net_type = Some("4G".to_string());
            changed = true;
        }

        if !changed {
            log::debug!("unmatched: {}", msg);
        }

        changed
    }
}
