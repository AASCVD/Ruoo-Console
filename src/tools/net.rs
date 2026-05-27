// ── 侦察工具 (子网计算, 系统信息, 网络接口, 磁盘, 进程, MIME) ──

// ── 子网计算 ──
pub struct SubnetInfo { pub network: String, pub broadcast: String, pub first_host: String, pub last_host: String, pub total_hosts: u64, pub usable_hosts: u64, pub mask: String, pub cidr: u8, }
pub fn subnet_calc(cidr: &str) -> Result<SubnetInfo, String> {
    let parts: Vec<&str> = cidr.split('/').collect();
    if parts.len()!=2 { return Err("需要 CIDR 格式如 192.168.1.0/24".into()); }
    let prefix: u8 = parts[1].parse().map_err(|_|"前缀无效")?;
    if prefix>32 { return Err("前缀 0-32".into()); }
    let octets: Vec<u32> = parts[0].split('.').filter_map(|o| o.parse().ok()).collect();
    if octets.len()!=4 { return Err("IP 格式无效".into()); }
    let ip_num: u32 = (octets[0]<<24)|(octets[1]<<16)|(octets[2]<<8)|octets[3];
    let mask_num: u32 = if prefix==0 {0} else {!0u32<<(32-prefix)};
    let net = ip_num & mask_num;
    let bcast = net | !mask_num;
    let to_ip = |n:u32| format!("{}.{}.{}.{}", (n>>24)&0xFF,(n>>16)&0xFF,(n>>8)&0xFF,n&0xFF);
    Ok(SubnetInfo{network:to_ip(net),broadcast:to_ip(bcast),first_host:if prefix>=31{to_ip(net)}else{to_ip(net+1)},last_host:if prefix>=31{to_ip(bcast)}else{to_ip(bcast-1)},total_hosts:1u64<<(32-prefix),usable_hosts:if prefix>=31{0}else{(1u64<<(32-prefix))-2},mask:to_ip(mask_num),cidr:prefix})
}

// ── 系统信息 ──
pub fn system_info() -> Vec<String> {
    let mut info = Vec::new();
    info.push(format!("  OS: {}", std::env::consts::OS));
    info.push(format!("  Arch: {}", std::env::consts::ARCH));
    info.push(format!("  Family: {}", std::env::consts::FAMILY));
    info.push(format!("  CPUs: {}", std::thread::available_parallelism().map(|n|n.get()).unwrap_or(0)));
    info.push(format!("  PID: {}", std::process::id()));
    #[cfg(windows)] { if let Ok(o)=std::process::Command::new("wmic").args(["OS","get","TotalVisibleMemorySize,FreePhysicalMemory","/Value"]).output() { for line in String::from_utf8_lossy(&o.stdout).lines() { if line.contains("TotalVisibleMemorySize=") { if let Ok(kb)=line.split('=').nth(1).unwrap_or("0").parse::<u64>() { info.push(format!("  Total RAM: {:.1} GB",kb as f64/1024./1024.)); } } if line.contains("FreePhysicalMemory=") { if let Ok(kb)=line.split('=').nth(1).unwrap_or("0").parse::<u64>() { info.push(format!("  Free RAM: {:.1} GB",kb as f64/1024./1024.)); } } } } }
    #[cfg(not(windows))] { if let Ok(c)=std::fs::read_to_string("/proc/meminfo") { for line in c.lines() { if line.starts_with("MemTotal:") { if let Some(k)=line.split_whitespace().nth(1) { if let Ok(kb)=k.parse::<u64>() { info.push(format!("  Total RAM: {:.1} GB",kb as f64/1024./1024.)); } } } if line.starts_with("MemAvailable:") { if let Some(k)=line.split_whitespace().nth(1) { if let Ok(kb)=k.parse::<u64>() { info.push(format!("  Avail RAM: {:.1} GB",kb as f64/1024./1024.)); } } } } } }
    #[cfg(windows)] { if let Ok(o)=std::process::Command::new("wmic").args(["logicaldisk","get","size,freespace,caption"]).output() { for line in String::from_utf8_lossy(&o.stdout).lines().skip(1) { let p:Vec<&str>=line.split_whitespace().collect(); if p.len()>=3 { if let(Ok(f),Ok(t))=(p[1].parse::<u64>(),p[2].parse::<u64>()) { if t>0 { info.push(format!("  Disk {}: {:.1}/{:.1} GB",p[0],f as f64/1e9,t as f64/1e9)); } } } } } }
    #[cfg(not(windows))] { if let Ok(o)=std::process::Command::new("df").args(["-h","--total"]).output() { for line in String::from_utf8_lossy(&o.stdout).lines().skip(1) { let p:Vec<&str>=line.split_whitespace().collect(); if p.len()>=4&&(p[0].starts_with("/dev/")||p[0]=="total") { info.push(format!("  Disk {}: {}/{} ({})",p[0],p[2],p[1],p.get(4).unwrap_or(&"?"))); } } } }
    info
}

// ── 网络接口 ──
pub fn network_interfaces() -> Vec<String> {
    let mut r = Vec::new();
    #[cfg(windows)] { if let Ok(o)=std::process::Command::new("ipconfig").output() { let stdout=String::from_utf8_lossy(&o.stdout); let mut cur=String::new(); for line in stdout.lines() { let t=line.trim(); if t.is_empty(){continue;} if !t.starts_with(' ')&&t.contains(':') { if !cur.is_empty(){r.push(cur.clone());} cur=t.to_string(); } else if t.contains("IPv4")||t.contains("IP Address") { cur.push_str(&format!("\n    {}",t)); } else if t.contains("Subnet")||t.contains("子网") { cur.push_str(&format!(" | {}",t)); } else if t.contains("Gateway")||t.contains("网关") { cur.push_str(&format!(" | {}",t)); } else if t.contains("Physical")||t.contains("物理") { cur.push_str(&format!(" | MAC:{}",t.split(':').last().unwrap_or("").trim())); } } if !cur.is_empty(){r.push(cur);} } }
    #[cfg(not(windows))] { let cmd=if std::process::Command::new("ip").arg("addr").output().is_ok(){("ip",vec!["addr"])}else{("ifconfig",vec![])}; if let Ok(o)=std::process::Command::new(cmd.0).args(&cmd.1).output() { let stdout=String::from_utf8_lossy(&o.stdout); let mut cur=String::new(); for line in stdout.lines() { let t=line.trim(); if t.is_empty(){continue;} if !t.starts_with(' ')&&(t.chars().next().unwrap_or(' ').is_ascii_digit()||t.contains(": ")) { if !cur.is_empty(){r.push(cur.clone());} cur=t.to_string(); } else if t.starts_with("inet ")||t.starts_with("inet6 ") { cur.push_str(&format!("\n    {}",t)); } else if t.starts_with("link/")||t.contains("ether ") { cur.push_str(&format!(" | {}",t)); } } if !cur.is_empty(){r.push(cur);} } }
    if r.is_empty() { r.push("  无法获取网络接口信息".into()); }
    r
}

// ── 磁盘使用 ──
pub fn disk_usage() -> Vec<String> {
    let mut r = Vec::new();
    #[cfg(windows)] { if let Ok(o)=std::process::Command::new("wmic").args(["logicaldisk","get","caption,freespace,size"]).output() { for line in String::from_utf8_lossy(&o.stdout).lines().skip(1) { let p:Vec<&str>=line.split_whitespace().collect(); if p.len()>=3 { if let(Ok(f),Ok(t))=(p[1].parse::<u64>(),p[2].parse::<u64>()) { if t>0 { let u=t-f; r.push(format!("  {} {:.1}/{:.1} GB ({:.0}%)",p[0],u as f64/1e9,t as f64/1e9,(u as f64/t as f64)*100.)); } } } } } }
    #[cfg(not(windows))] { if let Ok(o)=std::process::Command::new("df").args(["-h"]).output() { for line in String::from_utf8_lossy(&o.stdout).lines().skip(1) { let p:Vec<&str>=line.split_whitespace().collect(); if p.len()>=5&&p[0].starts_with("/dev/") { r.push(format!("  {} {} / {} ({})",p[0],p[2],p[1],p.get(4).unwrap_or(&"?"))); } } } }
    if r.is_empty() { r.push("  无法获取磁盘信息".into()); }
    r
}

// ── 进程列表 ──
pub fn process_list(filter: Option<&str>) -> Vec<String> {
    let mut r = Vec::new();
    #[cfg(windows)] { if let Ok(o)=std::process::Command::new("tasklist").args(["/FO","CSV","/NH"]).output() { for line in String::from_utf8_lossy(&o.stdout).lines() { let l=line.trim_matches('"').replace("\",\""," | "); if let Some(f)=filter { if l.to_lowercase().contains(&f.to_lowercase()){r.push(l);} } else { r.push(l); } if r.len()>=100&&filter.is_none(){break;} } } }
    #[cfg(not(windows))] { if let Ok(o)=std::process::Command::new("ps").args(["aux"]).output() { for line in String::from_utf8_lossy(&o.stdout).lines().skip(1) { if let Some(f)=filter { if line.to_lowercase().contains(&f.to_lowercase()){r.push(line.to_string());} } else { r.push(line.to_string()); } if r.len()>=50&&filter.is_none(){break;} } } }
    if r.is_empty() { r.push("  未能获取进程列表".into()); }
    r
}

// ── MIME 检测 ──
pub fn mime_detect(filename: &str) -> Result<String, String> {
    let data = std::fs::read(filename).map_err(|e| format!("读取失败: {}", e))?;
    if data.is_empty() { return Ok("空文件".into()); }
    let magic = &data[..data.len().min(16)];
    match magic {
        [0xFF,0xD8,0xFF,..]=>Ok("JPEG (image/jpeg)".into()),[0x89,b'P',b'N',b'G',..]=>Ok("PNG (image/png)".into()),
        [b'G',b'I',b'F',b'8',..]=>Ok("GIF (image/gif)".into()),[0x25,0x50,0x44,0x46,..]=>Ok("PDF".into()),
        [0x50,0x4B,0x03,0x04,..]=>Ok("ZIP".into()),[0x1F,0x8B,..]=>Ok("GZIP".into()),
        [0x7F,b'E',b'L',b'F',..]=>Ok("ELF".into()),[0x4D,0x5A,..]=>Ok("PE (.exe/.dll)".into()),
        [0xCA,0xFE,0xBA,0xBE,..]=>Ok("Java Class".into()),[b'R',b'a',b'r',b'!',..]=>Ok("RAR".into()),
        [0x42,0x4D,..]=>Ok("BMP".into()),[0x00,0x00,0x01,0xBA,..]=>Ok("MPEG".into()),
        [b'O',b'g',b'g',b'S',..]=>Ok("OGG".into()),[0x1A,0x45,0xDF,0xA3,..]=>Ok("WebM/MKV".into()),
        [b'<',b'?',b'x',b'm',b'l',..]=>Ok("XML".into()),[b'<',b'h',b't',b'm',b'l',..]|[b'<',b'H',b'T',b'M',b'L',..]=>Ok("HTML".into()),
        _ => if data.iter().all(|&b| b>=0x20||b==b'\n'||b==b'\r'||b==b'\t') { Ok("text/plain".into()) } else { let h: String=magic.iter().take(8).map(|b|format!("{:02X}",b)).collect::<Vec<_>>().join(" "); Ok(format!("未知 — 魔术字节: {}",h)) }
    }
}
