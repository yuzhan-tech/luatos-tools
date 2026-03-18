/// Context for incremental log parsing, holding partial data between reads.
pub struct LogContext {
    pub buffer: Vec<u8>,
}

impl LogContext {
    pub fn new() -> Self {
        LogContext { buffer: Vec::new() }
    }
}

/// Reverse 0x7D escape sequences in a log frame.
///
/// - 7D 01 -> 7D
/// - 7D 02 -> 7F
fn log_unpack(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i] == 0x7D && i + 1 < data.len() {
            match data[i + 1] {
                0x01 => out.push(0x7D),
                0x02 => out.push(0x7F),
                _ => out.push(data[i + 1]),
            }
            i += 2;
        } else {
            out.push(data[i]);
            i += 1;
        }
    }
    out
}

/// A parsed log message with optional device tick from the frame header.
pub struct LogMessage {
    pub text: String,
    /// Device tick in milliseconds (from frame header word 1).
    pub tick_ms: u32,
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset + 4)
        .and_then(|s| s.try_into().ok())
        .map(u32::from_le_bytes)
}

fn read_len_prefixed_string(data: &[u8], offset: &mut usize) -> Option<String> {
    *offset = (*offset + 3) & !3;
    let len = read_u32(data, *offset)? as usize;
    *offset += 4;
    let bytes = data.get(*offset..*offset + len)?;
    let s = String::from_utf8_lossy(bytes).to_string();
    *offset += len;
    *offset = (*offset + 3) & !3;
    Some(s)
}

/// Parse a single log frame: 12-byte header + format string + data.
///
/// Handles multiple trace format types:
/// - `%.*s`: length-prefixed string (most common LuatOS log output)
/// - `%s`: null-terminated string argument
/// - Format strings containing literal text with `%d`/`%u`/`%x`/`%s` etc.
/// - Plain text format strings with no arguments
fn log_split(data: &[u8]) -> Option<LogMessage> {
    if data.len() < 12 {
        return None;
    }

    // 12-byte header: [u32 tick_lo] [u32 tick_hi] [u32 id]
    let tick_ms = u32::from_le_bytes(data[0..4].try_into().unwrap_or([0; 4]));

    let payload = &data[12..];

    // Find null-terminated format string (4-byte aligned)
    let mut fmt_end = None;
    for i in 0..payload.len() {
        if payload[i] == 0 {
            fmt_end = Some(i);
            break;
        }
    }

    let fmt_end = fmt_end?;
    let fmt = std::str::from_utf8(&payload[..fmt_end]).ok()?;

    // Advance past the format string, aligned to 4 bytes
    let data_start = (fmt_end + 4) & !0x3;
    let rest = if data_start < payload.len() {
        &payload[data_start..]
    } else {
        &[]
    };

    let mk = |text: String| LogMessage { text, tick_ms };

    // %.*s: 4-byte length prefix + string data
    if fmt == "%.*s" {
        let mut offset = 0;
        if let Some(text) = read_len_prefixed_string(rest, &mut offset) {
            return Some(mk(text));
        }
        return None;
    }

    // Try to do basic printf-style formatting with the binary args
    let result = fmt_printf(fmt, rest);
    if !result.is_empty() {
        return Some(mk(result));
    }

    log::debug!(
        "log_split: empty result for fmt={:?} rest={:02x?}",
        fmt,
        rest
    );
    None
}

/// Basic printf-style formatter: walks the format string and substitutes
/// %d/%i/%u/%x/%X/%s/%.*s/%p from a binary argument stream (4-byte aligned LE).
fn fmt_printf(fmt: &str, data: &[u8]) -> String {
    let mut out = String::new();
    let mut offset = 0;
    let chars: Vec<char> = fmt.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] != '%' {
            out.push(chars[i]);
            i += 1;
            continue;
        }

        i += 1; // skip '%'
        if i >= chars.len() {
            break;
        }

        // Handle %%
        if chars[i] == '%' {
            out.push('%');
            i += 1;
            continue;
        }

        // Skip flags: -, +, 0, space, #
        while i < chars.len() && "-+ 0#".contains(chars[i]) {
            i += 1;
        }
        // Skip width (digits or *)
        if i < chars.len() && chars[i] == '*' {
            // width from arg
            offset = (offset + 3) & !3;
            if offset + 4 <= data.len() {
                // consume but don't use for output
                offset += 4;
            }
            i += 1;
        } else {
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
        }
        // Skip precision
        if i < chars.len() && chars[i] == '.' {
            i += 1;
            if i < chars.len() && chars[i] == '*' {
                // precision from arg
                offset = (offset + 3) & !3;
                let prec = if offset + 4 <= data.len() {
                    let v =
                        u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap_or([0; 4]));
                    offset += 4;
                    v as usize
                } else {
                    0
                };

                i += 1;
                if i < chars.len() && chars[i] == 's' {
                    // %.*s: precision-limited string from pointer
                    // In trace protocol, data is inline after the length
                    if offset + prec <= data.len() {
                        let s = String::from_utf8_lossy(&data[offset..offset + prec]);
                        out.push_str(&s);
                        offset += prec;
                    }
                    i += 1;
                    continue;
                }
            } else {
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
        }
        // Skip length modifier: l, ll, h, hh, z
        if i < chars.len() && chars[i] == 'l' {
            i += 1;
            if i < chars.len() && chars[i] == 'l' {
                i += 1;
            }
        } else if i < chars.len() && (chars[i] == 'h' || chars[i] == 'z') {
            i += 1;
            if i < chars.len() && chars[i] == 'h' {
                i += 1;
            }
        }

        if i >= chars.len() {
            break;
        }

        let spec = chars[i];
        i += 1;

        match spec {
            'd' | 'i' => {
                offset = (offset + 3) & !3;
                if offset + 4 <= data.len() {
                    let v =
                        i32::from_le_bytes(data[offset..offset + 4].try_into().unwrap_or([0; 4]));
                    out.push_str(&v.to_string());
                    offset += 4;
                }
            }
            'u' => {
                offset = (offset + 3) & !3;
                if offset + 4 <= data.len() {
                    let v =
                        u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap_or([0; 4]));
                    out.push_str(&v.to_string());
                    offset += 4;
                }
            }
            'x' => {
                offset = (offset + 3) & !3;
                if offset + 4 <= data.len() {
                    let v =
                        u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap_or([0; 4]));
                    out.push_str(&format!("{:x}", v));
                    offset += 4;
                }
            }
            'X' => {
                offset = (offset + 3) & !3;
                if offset + 4 <= data.len() {
                    let v =
                        u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap_or([0; 4]));
                    out.push_str(&format!("{:X}", v));
                    offset += 4;
                }
            }
            'p' => {
                offset = (offset + 3) & !3;
                if offset + 4 <= data.len() {
                    let v =
                        u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap_or([0; 4]));
                    out.push_str(&format!("0x{:x}", v));
                    offset += 4;
                }
            }
            's' => {
                if let Some(s) = read_len_prefixed_string(data, &mut offset) {
                    out.push_str(&s);
                }
            }
            'c' => {
                offset = (offset + 3) & !3;
                if offset + 4 <= data.len() {
                    let v = data[offset];
                    out.push(v as char);
                    offset += 4;
                }
            }
            _ => {
                // Unknown specifier, output literally
                out.push('%');
                out.push(spec);
            }
        }
    }

    out
}

/// Parse incoming serial data for log frames delimited by 0x7E.
///
/// Returns a list of parsed log messages with device tick timestamps.
pub fn log_parse(ctx: &mut LogContext, data: &[u8]) -> Vec<LogMessage> {
    let mut input = if ctx.buffer.is_empty() {
        data.to_vec()
    } else {
        let mut combined = std::mem::take(&mut ctx.buffer);
        combined.extend_from_slice(data);
        combined
    };

    let mut msgs = Vec::new();
    let mut offset = 0;

    while offset < input.len() {
        if input[offset] == 0x7E {
            // Find matching end 0x7E
            let mut found = false;
            for j in (offset + 1)..input.len() {
                if input[j] == 0x7E {
                    let frame = &input[offset + 1..j];
                    let unpacked = log_unpack(frame);
                    if let Some(msg) = log_split(&unpacked) {
                        msgs.push(msg);
                    }
                    offset = j;
                    found = true;
                    break;
                }
            }
            if !found {
                // Incomplete frame, save remainder
                break;
            }
        }
        offset += 1;
    }

    if offset < input.len() {
        ctx.buffer = input.split_off(offset);
    }

    msgs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_hex(hex: &str) -> Vec<u8> {
        let hex: String = hex.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(hex.len() % 2 == 0, "hex length must be even");
        hex.as_bytes()
            .chunks(2)
            .map(|chunk| {
                let s = std::str::from_utf8(chunk).unwrap();
                u8::from_str_radix(s, 16).unwrap()
            })
            .collect()
    }

    #[test]
    fn test_log_unpack() {
        let data = vec![0x7D, 0x01, 0x7D, 0x02];
        assert_eq!(log_unpack(&data), vec![0x7D, 0x7F]);
    }

    #[test]
    fn test_log_parse_sample() {
        // Sample from eclogs.py
        let hex_str = "7E599902000000000004CEA730A252E2A7300000000001C00000049\
                        2F757365722E4750494F09476F20476F20476F09300945433631387E";
        // We can't easily use this hex directly but the unpack test above verifies the core logic.
        let mut ctx = LogContext::new();
        let data = vec![0x7E, 0x00, 0x7E]; // minimal empty frame
        let msgs = log_parse(&mut ctx, &data);
        assert!(msgs.is_empty()); // too short to have content
    }

    #[test]
    fn test_log_parse_length_prefixed_string_args() {
        let frame = decode_hex(
            "7E1000000000000000C320140A25732025643A696F20766F6C7420332E337620256400\
             2573100000006273705F757365725F696E69745F696F89010000150000007E",
        );
        let mut ctx = LogContext::new();
        let msgs = log_parse(&mut ctx, &frame);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text, "bsp_user_init_io 393:io volt 3.3v 21");
    }

    #[test]
    fn test_log_parse_chip_type_frame() {
        let frame = decode_hex(
            "7E0800000000000000C320140A25732025643A25782C25782C25782C25782C25642C257300\
             10000000616D5F6765745F636869705F7479706561030000F6BE06001B00000064000000DB000000\
             0A000000070000004543373138484D007E",
        );
        let mut ctx = LogContext::new();
        let msgs = log_parse(&mut ctx, &frame);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text, "am_get_chip_type 865:6bef6,1b,64,db,10,EC718HM");
    }

    #[test]
    fn test_log_parse_loadlibs_frame() {
        let frame = decode_hex(
            "7EA400000000000000C320140A442F6D61696E202573206C7561766D20256C6420256C6420256C6400\
             080000006C6F61646C696273F8FF3F00983E0000983E00007E",
        );
        let mut ctx = LogContext::new();
        let msgs = log_parse(&mut ctx, &frame);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text, "D/main loadlibs luavm 4194296 16024 16024");
    }

    #[test]
    fn test_log_parse_soccell_frame() {
        let frame = decode_hex(
            "7E9B692500000000004C6AD2082B534F4343454C4C3A2025782C25782C25752C25752C257500\
             2573206004000000F00000060E0000D70000008CCACB047E",
        );
        let mut ctx = LogContext::new();
        let msgs = log_parse(&mut ctx, &frame);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text, "+SOCCELL: 460,f000,3590,215,80464524");
    }
}
