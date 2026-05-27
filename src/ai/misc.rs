// === 小工具函数: parse_ports, is_code_tool ===
// 从 ai/mod.rs 提取

pub(super) fn parse_ports(ports_str: &str) -> Vec<u16> {
    crate::utils::parse_ports(ports_str)
}

/// 判断是否为代码读取类工具 — 这些工具结果必须保持准确，不能降敏
/// (降敏会破坏标识符如 netlink→n-e-t-l-i-n-k，导致后续 grep/find 失败)
pub(super) fn is_code_tool(tool_name: &str) -> bool {
    matches!(tool_name,
        "read_file" | "read_file_lines" | "grep" | "find_files" |
        "hex_dump" | "ls" | "stat" | "pwd" | "checksum_file" |
        "mime_detect" | "text_stats" | "word_frequency" | "du" |
        "deduplicate_lines" | "sort_lines" | "split_file"
    )
}
