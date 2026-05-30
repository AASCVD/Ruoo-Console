// ============================================================
// RUOO-CONSOLE v4.0 — 工具函数
// ============================================================

/// 哈希函数 — 已迁移到 plugin_crypto_tools.dll
/// 此处保留最小桩实现 (使用 std::hash::DefaultHasher)，仅用于 dead_code 路径
#[allow(dead_code)]
pub fn hash_md5(text: &str) -> String {
    format!("{:032x}", std_seeded_hash(text, 0x4d443520)) // "MD5 "
}
#[allow(dead_code)]
pub fn hash_sha1(text: &str) -> String {
    format!("{:040x}", std_seeded_hash(text, 0x53484131)) // "SHA1"
}
#[allow(dead_code)]
pub fn hash_sha256(text: &str) -> String {
    format!("{:064x}", std_seeded_hash(text, 0x53484132)) // "SHA2"
}
#[allow(dead_code)]
pub fn hash_sha512(text: &str) -> String {
    format!("{:0128x}", std_seeded_hash(text, 0x35313221)) // "512!"
}

/// 使用 std::hash 生成确定性哈希 (不依赖外部 crate)
fn std_seeded_hash(text: &str, seed: u64) -> u128 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    seed.hash(&mut h);
    text.hash(&mut h);
    let lo = h.finish();
    // 生成第二个64位哈希作为高位
    let mut h2 = std::collections::hash_map::DefaultHasher::new();
    (seed ^ 0xDEADBEEFCAFE0000).hash(&mut h2);
    text.hash(&mut h2);
    let hi = h2.finish();
    ((hi as u128) << 64) | (lo as u128)
}

// ── Base64 ──
const BASE64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode_str(input: &str) -> String {
    base64_encode_bytes(input.as_bytes())
}

/// 原始字节 → Base64 (用于AES密文等二进制数据编码)
pub fn base64_encode_bytes(data: &[u8]) -> String {
    let mut result = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let buffer: u32 = match chunk.len() {
            3 => ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | (chunk[2] as u32),
            2 => ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8),
            1 => (chunk[0] as u32) << 16,
            _ => 0,
        };
        for i in (0..4).rev() {
            if i > chunk.len() {
                result.push('=');
            } else {
                result.push(BASE64_CHARS[((buffer >> (6 * i)) & 0x3F) as usize] as char);
            }
        }
    }
    result
}

/// Base64 解码最大输出字节数 (防压缩炸弹 OOM)
const BASE64_MAX_OUTPUT: usize = 16 * 1024 * 1024; // 16MB

pub fn base64_decode_str(input: &str) -> Result<String, String> {
    // ★ BUG-13 修复: 限制输入长度，防止压缩炸弹导致 OOM
    // Base64 编码膨胀率 ~4/3，16MB 输出 ≈ 21.3MB 输入
    if input.len() > BASE64_MAX_OUTPUT * 4 / 3 + 1024 {
        return Err(format!(
            "Base64 输入过大 ({} 字节), 超过安全限制 {} MB",
            input.len(),
            BASE64_MAX_OUTPUT / 1024 / 1024
        ));
    }
    let input = input.trim_end_matches('=');
    let mut bytes = Vec::with_capacity(input.len().min(BASE64_MAX_OUTPUT) * 3 / 4);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for c in input.chars() {
        if c.is_whitespace() { continue; }
        let val = match c {
            'A'..='Z' => c as u8 - b'A',
            'a'..='z' => c as u8 - b'a' + 26,
            '0'..='9' => c as u8 - b'0' + 52,
            '+' => 62, '/' => 63,
            _ => return Err(format!("无效 Base64 字符: '{}'", c)),
        } as u32;
        buffer = (buffer << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
        // ★ 额外防护: 解码过程中也检查大小
        if bytes.len() > BASE64_MAX_OUTPUT {
            return Err(format!("Base64 解码输出超过 {} MB 安全限制", BASE64_MAX_OUTPUT / 1024 / 1024));
        }
    }
    String::from_utf8(bytes).map_err(|e| format!("Base64 解码结果非有效 UTF-8: {}", e))
}

// ── URL 编解码 ──
pub fn url_encode_str(input: &str) -> String {
    input.bytes().map(|b| {
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' || b == b'~' {
            (b as char).to_string()
        } else {
            format!("%{:02X}", b)
        }
    }).collect()
}

pub fn url_decode_str(input: &str) -> String {
    let mut result = String::new();
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if hex.len() == 2 {
                if let Ok(b) = u8::from_str_radix(&hex, 16) {
                    result.push(b as char);
                    continue;
                }
            }
            result.push('%');
            result.push_str(&hex);
        } else if c == '+' {
            result.push(' ');
        } else {
            result.push(c);
        }
    }
    result
}

// ── 摩尔斯电码 ──
pub fn morse_encode(text: &str) -> String {
    let map = [
        ("A",".-"),("B","-..."),("C","-.-."),("D","-.."),("E","."),
        ("F","..-."),("G","--."),("H","...."),("I",".."),("J",".---"),
        ("K","-.-"),("L",".-.."),("M","--"),("N","-."),("O","---"),
        ("P",".--."),("Q","--.-"),("R",".-."),("S","..."),("T","-"),
        ("U","..-"),("V","...-"),("W",".--"),("X","-..-"),("Y","-.--"),
        ("Z","--.."),
        ("0","-----"),("1",".----"),("2","..---"),("3","...--"),("4","....-"),
        ("5","....."),("6","-...."),("7","--..."),("8","---.."),("9","----."),
        (".",".-.-.-"),(",","--..--"),("?","..--.."),(" ","/"),
    ];
    text.to_uppercase().chars()
        .map(|c| map.iter().find(|&&(ch,_)| ch == c.to_string().as_str()).map(|&(_,m)| m).unwrap_or("?"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn morse_decode(text: &str) -> String {
    let map = [
        (".-","A"),("-...","B"),("-.-.","C"),("-..","D"),(".","E"),
        ("..-.","F"),("--.","G"),("....","H"),("..","I"),(".---","J"),
        ("-.-","K"),(".-..","L"),("--","M"),("-.","N"),("---","O"),
        (".--.","P"),("--.-","Q"),(".-.","R"),("...","S"),("-","T"),
        ("..-","U"),("...-","V"),(".--","W"),("-..-","X"),("-.--","Y"),
        ("--..","Z"),
        ("-----","0"),(".----","1"),("..---","2"),("...--","3"),("....-","4"),
        (".....","5"),("-....","6"),("--...","7"),("---..","8"),("----.","9"),
        (".-.-.-","."),("--..--",","),("..--..","?"),("/"," "),
    ];
    text.split_whitespace()
        .map(|code| map.iter().find(|&&(m,_)| m == code).map(|&(_,c)| c).unwrap_or("?"))
        .collect()
}

// ── Base64 → 原始字节 (用于 AES 解密等) ──
pub fn base64_decode_bytes(input: &str) -> Result<Vec<u8>, String> {
    // ★ BUG-13 修复: 同样限制 base64_decode_bytes
    if input.len() > BASE64_MAX_OUTPUT * 4 / 3 + 1024 {
        return Err(format!(
            "Base64 输入过大 ({} 字节), 超过安全限制 {} MB",
            input.len(),
            BASE64_MAX_OUTPUT / 1024 / 1024
        ));
    }
    let input = input.trim_end_matches('=');
    let mut bytes = Vec::with_capacity(input.len().min(BASE64_MAX_OUTPUT) * 3 / 4);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for c in input.chars() {
        if c.is_whitespace() { continue; }
        let val = match c {
            'A'..='Z' => c as u8 - b'A',
            'a'..='z' => c as u8 - b'a' + 26,
            '0'..='9' => c as u8 - b'0' + 52,
            '+' => 62, '/' => 63,
            _ => return Err(format!("无效 Base64 字符: '{}'", c)),
        } as u32;
        buffer = (buffer << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
        if bytes.len() > BASE64_MAX_OUTPUT {
            return Err(format!("Base64 解码输出超过 {} MB 安全限制", BASE64_MAX_OUTPUT / 1024 / 1024));
        }
    }
    Ok(bytes)
}

// ── XOR 字符串 ──
#[allow(dead_code)]
pub fn xor_str(text: &str, key: &str) -> String {
    let key_bytes = key.as_bytes();
    if key_bytes.is_empty() { return text.to_string(); }
    text.bytes()
        .enumerate()
        .map(|(i, b)| (b ^ key_bytes[i % key_bytes.len()]) as char)
        .collect()
}

// ── 模糊匹配建议 ──
pub fn fuzzy_match(input: &str) -> Vec<String> {
    // v10.0: 动态读取 mod_meta 作为唯一真相源，不再硬编码过时列表
    use std::sync::LazyLock;
    use std::collections::HashSet;
    static FUZZY_CMDS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
        let mut set: HashSet<&str> = HashSet::new();
        for (name, _syntax, _desc, _cat) in crate::commands::mod_meta::all_commands() {
            let base = name.split(|c: char| c == '/' || c == ' ').next().unwrap_or(name).trim();
            if !base.is_empty() && !base.starts_with('&') {
                set.insert(base);
            }
        }
        let mut v: Vec<&str> = set.into_iter().collect();
        v.sort();
        v
    });
    let commands: &[&str] = &FUZZY_CMDS;
    commands.iter()
        .filter(|c| c.starts_with(input) || (input.len() >= 2 && c.contains(input)))
        .take(3)
        .map(|s| s.to_string())
        .collect()
}

// ── 统一端口解析 (v4.1: ai.rs 与 commands.rs 共用) ──
pub const TOP_100_PORTS: &[u16] = &[
    21, 22, 23, 25, 53, 67, 69, 80, 110, 111, 123, 135, 137, 139, 143,
    161, 162, 389, 443, 445, 465, 514, 587, 636, 873, 993, 995, 1080,
    1194, 1352, 1433, 1521, 1723, 2049, 2082, 2083, 2181, 2222, 2375,
    2376, 3128, 3306, 3389, 4444, 4848, 5000, 5060, 5353, 5432, 5632,
    5672, 5900, 5901, 5985, 5986, 6379, 6443, 7077, 7443, 8000, 8009,
    8080, 8081, 8088, 8181, 8443, 8888, 8983, 9000, 9042, 9090, 9092,
    9100, 9200, 9300, 9443, 9600, 9999, 10000, 11211, 15672, 16010,
    18080, 27017, 27018, 27019, 28015, 28017, 32768, 50000, 50030,
    50070, 50075, 50090, 54321,
];

/// 统一端口解析: "1-1000" / "80,443,8080" / "22,443,8000-9000"
///
/// ★ BUG-9 修复: 非法字符不再静默降级为 TOP_100, 而是发出警告
///   合法格式: 逗号分隔数字 或 范围 (1-1000, 80,443)
///   包含非法字符 → eprintln 警告 + 返回空 vec, 由调用方决定行为
pub fn parse_ports(spec: &str) -> Vec<u16> {
    if spec.is_empty() {
        return TOP_100_PORTS.to_vec();
    }
    // 检查是否包含非法字符(非数字/逗号/短横线/空格)
    let invalid_chars: String = spec.chars()
        .filter(|c| !c.is_ascii_digit() && *c != ',' && *c != '-' && *c != ' ')
        .collect();
    if !invalid_chars.is_empty() {
        eprintln!(
            "[!] parse_ports: 端口格式无效 \"{}\" — 包含非法字符 \"{}\". \
             期望格式: \"1-1000\" / \"80,443,8080\" / \"22,443,8000-9000\"",
            spec, invalid_chars
        );
        return Vec::new(); // 返回空而非静默替换 — 让用户知晓格式错误
    }
    let mut ports = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() { continue; }
        if part.contains('-') {
            let range: Vec<&str> = part.splitn(2, '-').collect();
            if range.len() == 2 {
                if let (Ok(start), Ok(end)) = (range[0].trim().parse::<u16>(), range[1].trim().parse::<u16>()) {
                    let lo = start.min(end);
                    let hi = start.max(end);
                    for p in lo..=hi {
                        if ports.len() < 65535 { ports.push(p); }
                    }
                } else {
                    eprintln!("[!] parse_ports: 范围解析失败 \"{}\"", part);
                }
            }
        } else if let Ok(p) = part.parse::<u16>() {
            if ports.len() < 65535 { ports.push(p); }
        } else {
            eprintln!("[!] parse_ports: 端口值无效 \"{}\"", part);
        }
    }
    if ports.is_empty() {
        eprintln!("[!] parse_ports: 未解析到有效端口, 降级为 TOP_100");
        TOP_100_PORTS.to_vec()
    } else {
        ports
    }
}

/// 旧别名，保持兼容
#[allow(dead_code)]
pub fn simulated_hash(text: &str, seed: u64) -> Vec<u8> {
    // 现在使用真实哈希
    let hash_str = match seed {
        0 => hash_md5(text),
        1 => hash_sha1(text),
        2 => hash_sha256(text),
        _ => hash_sha512(text),
    };
    // 转换为字节数组 (保持和原来返回类型兼容)
    let mut out = Vec::new();
    for i in (0..hash_str.len()).step_by(2) {
        if i + 2 <= hash_str.len() {
            if let Ok(b) = u8::from_str_radix(&hash_str[i..i+2], 16) {
                out.push(b);
            }
        }
    }
    // 补齐到期望长度
    let expected = match seed { 0 => 16, 1 => 20, 2 => 32, _ => 64 };
    while out.len() < expected { out.push(0); }
    out.truncate(expected);
    out
}

/// JWT 解码 (简化版 — 不验证签名)
pub fn jwt_decode_simple(token: &str) -> Result<(String, String), String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err("JWT 格式错误: 需要 header.payload.signature 三段".into());
    }
    let header = base64_decode_json(parts[0])
        .map_err(|e| format!("Header 解码失败: {}", e))?;
    let payload = base64_decode_json(parts[1])
        .map_err(|e| format!("Payload 解码失败: {}", e))?;
    Ok((header, payload))
}

fn base64_decode_json(b64: &str) -> Result<String, String> {
    let bytes = base64_decode_bytes(b64)?;
    let text = String::from_utf8(bytes).map_err(|e| format!("UTF-8: {}", e))?;
    // JSON 美化
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
        Ok(serde_json::to_string_pretty(&v).unwrap_or(text))
    } else {
        Ok(text)
    }
}

/// 随机密码生成
pub fn random_password_str(len: usize) -> String {
    let charset = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*";
    let mut rng = rand::thread_rng();
    (0..len).map(|_| {
        let idx = rand::Rng::gen_range(&mut rng, 0..charset.len());
        charset[idx] as char
    }).collect()
}
