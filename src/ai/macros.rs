// === AI 工具定义宏 t0..t4 ===
// 从 ai/mod.rs 提取, 通过 #[macro_use] 导入
// json! 宏在展开点 (mod.rs) 解析, 此处无需导入

macro_rules! t0 { ($name:literal, $desc:literal) => {
    json!({"type":"function","function":{"name":$name,"description":$desc,"parameters":{"type":"object","properties":{},"required":[]}}})
}}
macro_rules! t1 { ($name:literal, $desc:literal, $p1:literal, $t1:literal, $d1:literal) => {
    json!({"type":"function","function":{"name":$name,"description":$desc,"parameters":{"type":"object","properties":{$p1:{"type":$t1,"description":$d1}},"required":[$p1]}}})
}}
macro_rules! t2 { ($name:literal, $desc:literal, $p1:literal, $t1:literal, $d1:literal, $p2:literal, $t2:literal, $d2:literal) => {
    json!({"type":"function","function":{"name":$name,"description":$desc,"parameters":{"type":"object","properties":{$p1:{"type":$t1,"description":$d1},$p2:{"type":$t2,"description":$d2}},"required":[$p1,$p2]}}})
}}
macro_rules! t3 { ($name:literal, $desc:literal, $p1:literal, $t1:literal, $d1:literal, $p2:literal, $t2:literal, $d2:literal, $p3:literal, $t3:literal, $d3:literal) => {
    json!({"type":"function","function":{"name":$name,"description":$desc,"parameters":{"type":"object","properties":{$p1:{"type":$t1,"description":$d1},$p2:{"type":$t2,"description":$d2},$p3:{"type":$t3,"description":$d3}},"required":[$p1,$p2,$p3]}}})
}}
macro_rules! t4 { ($name:literal, $desc:literal, $p1:literal, $ty1:literal, $d1:literal, $p2:literal, $ty2:literal, $d2:literal, $p3:literal, $ty3:literal, $d3:literal, $p4:literal, $ty4:literal, $d4:literal) => {
    json!({"type":"function","function":{"name":$name,"description":$desc,"parameters":{"type":"object","properties":{$p1:{"type":$ty1,"description":$d1},$p2:{"type":$ty2,"description":$d2},$p3:{"type":$ty3,"description":$d3},$p4:{"type":$ty4,"description":$d4}},"required":[$p1]}}})
}}

