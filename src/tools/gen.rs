// ── 工具函数 (模式提取, 正则测试, Markdown转换, 单位转换, 颜色转换) ──

pub fn extract_pattern(text: &str, pattern_type: &str) -> String {
    use regex::Regex;
    let mut r = format!("模式提取 ({})\n", pattern_type);
    let re = match pattern_type {
        "email" => Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").ok(),
        "url" => Regex::new(r#"https?://[^\s<>"{}|\\^`]+"#).ok(),
        "ip" => Regex::new(r"\b(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\b").ok(),
        "phone" => Regex::new(r"1[3-9][0-9]\d{8}").ok(),
        "all" => {
            r.push_str("\n-- 邮箱 --\n");
            if let Some(re)=Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").ok() { for m in re.find_iter(text){r.push_str(&format!("  {}\n",m.as_str()));} }
            r.push_str("\n-- URL --\n");
            if let Some(re)=Regex::new(r#"https?://[^\s<>"{}|\\^`]+"#).ok() { for m in re.find_iter(text){r.push_str(&format!("  {}\n",m.as_str()));} }
            r.push_str("\n-- IP --\n");
            if let Some(re)=Regex::new(r"\b(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\b").ok() { for m in re.find_iter(text){r.push_str(&format!("  {}\n",m.as_str()));} }
            r.push_str("\n-- 手机号 --\n");
            if let Some(re)=Regex::new(r"1[3-9][0-9]\d{8}").ok() { for m in re.find_iter(text){r.push_str(&format!("  {}\n",m.as_str()));} }
            return r;
        }
        _ => return format!("不支持: {} (email/url/ip/phone/all)", pattern_type),
    };
    match re { Some(re)=>{let mut c=0;for m in re.find_iter(text){r.push_str(&format!("  {}\n",m.as_str()));c+=1;if c>=50{r.push_str("  ...\n");break;}}if c==0{r.push_str("  (未找到)\n");}else{r.push_str(&format!("\n  共 {} 个匹配\n",c));}}None=>r.push_str("  正则编译失败\n"), }
    r
}

pub fn regex_test(pattern: &str, text: &str) -> String {
    use regex::Regex;
    match Regex::new(pattern) {
        Ok(re) => {
            let mut r = format!("正则: /{}/\n目标: \"{}\"\n\n", pattern, &text[..text.len().min(200)]);
            let matches: Vec<_> = re.find_iter(text).collect();
            if matches.is_empty() { r.push_str("  无匹配\n"); }
            else { r.push_str(&format!("  {} 个匹配:\n", matches.len())); for (i,m) in matches.iter().enumerate() { let p: String = text[m.start()..m.end()].chars().take(80).collect(); r.push_str(&format!("    [{}] {}..{}: \"{}\"\n", i+1, m.start(), m.end(), p)); if i>=20 { r.push_str("    ...\n"); break; } }
                if let Some(caps) = re.captures(text) { if caps.len()>1 { r.push_str("\n  捕获组:\n"); for (i,cap) in caps.iter().enumerate().skip(1) { if let Some(c)=cap { let s:String=c.as_str().chars().take(60).collect(); r.push_str(&format!("    ${}: \"{}\"\n", i, s)); } } } } }
            r
        }
        Err(e) => format!("正则语法错误: {}", e),
    }
}

pub fn markdown_to_html(md: &str) -> String {
    let mut html=String::from("<!DOCTYPE html>\n<html>\n<body>\n"); let mut in_cb=false; let mut in_ul=false;
    for line in md.lines() { let t=line.trim();
        if t.starts_with("```") { if in_cb{html.push_str("</pre>\n");in_cb=false;}else{html.push_str("<pre><code>\n");in_cb=true;} continue; }
        if in_cb { html.push_str(&html_escape(line)); html.push('\n'); continue; }
        if t.is_empty() { if in_ul{html.push_str("</ul>\n");in_ul=false;} html.push_str("<br>\n"); continue; }
        if t.starts_with("###### "){html.push_str(&format!("<h6>{}</h6>\n",&t[7..]));continue;}
        if t.starts_with("##### "){html.push_str(&format!("<h5>{}</h5>\n",&t[6..]));continue;}
        if t.starts_with("#### "){html.push_str(&format!("<h4>{}</h4>\n",&t[5..]));continue;}
        if t.starts_with("### "){html.push_str(&format!("<h3>{}</h3>\n",&t[4..]));continue;}
        if t.starts_with("## "){html.push_str(&format!("<h2>{}</h2>\n",&t[3..]));continue;}
        if t.starts_with("# "){html.push_str(&format!("<h1>{}</h1>\n",&t[2..]));continue;}
        if t.starts_with("* ")||t.starts_with("- "){if !in_ul{html.push_str("<ul>\n");in_ul=true;}html.push_str(&format!("  <li>{}</li>\n",parse_inline_md(&t[2..])));continue;}
        if in_ul{html.push_str("</ul>\n");in_ul=false;}
        if t=="---"||t=="***"||t=="___"{html.push_str("<hr>\n");continue;}
        if t.starts_with("> "){html.push_str(&format!("<blockquote>{}</blockquote>\n",parse_inline_md(&t[2..])));continue;}
        html.push_str(&format!("<p>{}</p>\n",parse_inline_md(t)));
    }
    if in_ul{html.push_str("</ul>\n");} if in_cb{html.push_str("</pre>\n");}
    html.push_str("</body>\n</html>"); html
}
fn parse_inline_md(text: &str) -> String {
    let mut r = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // Bold: **text**
        if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
            i += 2;
            r.push_str("<strong>");
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '*') {
                r.push(chars[i]);
                i += 1;
            }
            r.push_str("</strong>");
            if i + 1 < chars.len() { i += 2; }
            continue;
        }
        // Italic: *text*
        if chars[i] == '*' {
            i += 1;
            r.push_str("<em>");
            while i < chars.len() && chars[i] != '*' {
                r.push(chars[i]);
                i += 1;
            }
            r.push_str("</em>");
            if i < chars.len() { i += 1; }
            continue;
        }
        // Inline code: `text`
        if chars[i] == '`' {
            i += 1;
            r.push_str("<code>");
            while i < chars.len() && chars[i] != '`' {
                r.push(chars[i]);
                i += 1;
            }
            r.push_str("</code>");
            if i < chars.len() { i += 1; }
            continue;
        }
        // Link: [text](url)
        if chars[i] == '[' {
            i += 1;
            let mut lt = String::new();
            while i < chars.len() && chars[i] != ']' {
                lt.push(chars[i]);
                i += 1;
            }
            if i < chars.len() { i += 1; } // skip ']'
            if i < chars.len() && chars[i] == '(' {
                i += 1;
                let mut lu = String::new();
                while i < chars.len() && chars[i] != ')' {
                    lu.push(chars[i]);
                    i += 1;
                }
                if i < chars.len() { i += 1; } // skip ')'
                r.push_str(&format!("<a href=\"{}\">{}</a>", html_escape(&lu), html_escape(&lt)));
            } else {
                r.push('[');
                r.push_str(&html_escape(&lt));
            }
            continue;
        }
        r.push(chars[i]);
        i += 1;
    }
    r
}
fn html_escape(text: &str) -> String { text.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;") }

pub fn unit_convert(value: f64, from: &str, to: &str) -> String {
    let to_base: f64 = match from.to_lowercase().as_str() {
        "km"=>1000.,"m"=>1.,"cm"=>0.01,"mm"=>0.001,"mi"|"mile"=>1609.344,"ft"|"foot"=>0.3048,"in"|"inch"=>0.0254,"yd"|"yard"=>0.9144,
        "kg"=>1.,"g"=>0.001,"mg"=>0.000001,"lb"|"lbs"=>0.453592,"oz"=>0.0283495,"t"|"tonne"=>1000.,"st"|"stone"=>6.35029,
        "c"|"celsius"=>return convert_temperature(value,"c",to),"f"|"fahrenheit"=>return convert_temperature(value,"f",to),"k"|"kelvin"=>return convert_temperature(value,"k",to),
        "b"|"byte"=>1.,"kb"=>1024.,"mb"=>1024_f64.powi(2),"gb"=>1024_f64.powi(3),"tb"=>1024_f64.powi(4),"pb"=>1024_f64.powi(5),
        "m/s"|"m_s"=>1.,"km/h"|"km_h"=>0.277778,"mph"=>0.44704,"knot"|"kn"=>0.514444,
        _=>return format!("未知单位: {} (km/m/cm/mm/mi/ft/in,kg/g/mg/lb/oz,C/F/K,B/KB/MB/GB/TB,m/s/km/h/mph)",from),
    };
    let base=value*to_base;
    let from_base: f64 = match to.to_lowercase().as_str() {
        "km"=>0.001,"m"=>1.,"cm"=>100.,"mm"=>1000.,"mi"|"mile"=>1./1609.344,"ft"|"foot"=>1./0.3048,"in"|"inch"=>1./0.0254,"yd"|"yard"=>1./0.9144,
        "kg"=>1.,"g"=>1000.,"mg"=>1_000_000.,"lb"|"lbs"=>1./0.453592,"oz"=>1./0.0283495,"t"|"tonne"=>0.001,"st"|"stone"=>1./6.35029,
        "b"|"byte"=>1.,"kb"=>1./1024.,"mb"=>1./1024_f64.powi(2),"gb"=>1./1024_f64.powi(3),"tb"=>1./1024_f64.powi(4),"pb"=>1./1024_f64.powi(5),
        "m/s"|"m_s"=>1.,"km/h"|"km_h"=>3.6,"mph"=>1./0.44704,"knot"|"kn"=>1./0.514444,
        _=>return format!("未知目标单位: {}",to),
    };
    format!("{} {} = {:.6} {}",value,from,base*from_base,to)
}
fn convert_temperature(value: f64, from: &str, to: &str) -> String {
    let tl=to.to_lowercase();
    match from {
        "c"|"celsius"=>{let(f,k)=(value*9./5.+32.,value+273.15);match tl.as_str(){"f"|"fahrenheit"=>format!("{}°C = {:.2}°F",value,f),"k"|"kelvin"=>format!("{}°C = {:.2}K",value,k),"c"|"celsius"=>format!("{}°C = {}°C",value,value),_=>format!("{}°C = {:.2}°F | {:.2}K",value,f,k)}}
        "f"|"fahrenheit"=>{let(c,k)=((value-32.)*5./9.,(value-32.)*5./9.+273.15);match tl.as_str(){"c"|"celsius"=>format!("{}°F = {:.2}°C",value,c),"k"|"kelvin"=>format!("{}°F = {:.2}K",value,k),"f"|"fahrenheit"=>format!("{}°F = {}°F",value,value),_=>format!("{}°F = {:.2}°C | {:.2}K",value,c,k)}}
        "k"|"kelvin"=>{let(c,f)=(value-273.15,(value-273.15)*9./5.+32.);match tl.as_str(){"c"|"celsius"=>format!("{}K = {:.2}°C",value,c),"f"|"fahrenheit"=>format!("{}K = {:.2}°F",value,f),"k"|"kelvin"=>format!("{}K = {}K",value,value),_=>format!("{}K = {:.2}°C | {:.2}°F",value,c,f)}}
        _=>"未知温度单位".into(),
    }
}

pub fn color_convert(input: &str) -> String {
    let input=input.trim();
    if input.starts_with('#'){let hex=input.trim_start_matches('#');if hex.len()==6||hex.len()==3{let(r,g,b)=if hex.len()==6{(u8::from_str_radix(&hex[0..2],16).unwrap_or(0),u8::from_str_radix(&hex[2..4],16).unwrap_or(0),u8::from_str_radix(&hex[4..6],16).unwrap_or(0))}else{(u8::from_str_radix(&hex[0..1],16).unwrap_or(0)*17,u8::from_str_radix(&hex[1..2],16).unwrap_or(0)*17,u8::from_str_radix(&hex[2..3],16).unwrap_or(0)*17)};let(h,s,l)=rgb_to_hsl(r,g,b);return format!("HEX: #{:02X}{:02X}{:02X}\nRGB: rgb({},{},{})\nHSL: hsl({:.0}°,{:.0}%,{:.0}%)",r,g,b,r,g,b,h,s*100.,l*100.);}}
    if input.to_lowercase().starts_with("rgb"){let nums:Vec<&str>=input.trim_start_matches(|c:char|!c.is_ascii_digit()).trim_end_matches(|c:char|!c.is_ascii_digit()).split(',').collect();if nums.len()==3{if let(Ok(r),Ok(g),Ok(b))=(nums[0].trim().parse::<u8>(),nums[1].trim().parse::<u8>(),nums[2].trim().parse::<u8>()){let(h,s,l)=rgb_to_hsl(r,g,b);return format!("HEX: #{:02X}{:02X}{:02X}\nRGB: rgb({},{},{})\nHSL: hsl({:.0}°,{:.0}%,{:.0}%)",r,g,b,r,g,b,h,s*100.,l*100.);}}}
    if input.to_lowercase().starts_with("hsl"){let nums:Vec<&str>=input.trim_start_matches(|c:char|!c.is_ascii_digit()).trim_end_matches(|c:char|!c.is_ascii_digit()).split(',').collect();if nums.len()==3{if let(Ok(h),Ok(s),Ok(l))=(nums[0].trim().parse::<f64>(),nums[1].trim().parse::<f64>(),nums[2].trim().parse::<f64>()){let(r,g,b)=hsl_to_rgb(h%360.,s/100.,l/100.);return format!("HEX: #{:02X}{:02X}{:02X}\nRGB: rgb({},{},{})\nHSL: hsl({:.0}°,{:.0}%,{:.0}%)",r,g,b,r,g,b,h,s,l);}}}
    format!("无法解析颜色: {} (支持: #FF5733 / rgb(255,87,51) / hsl(9,100%,60%))",input)
}
fn rgb_to_hsl(r:u8,g:u8,b:u8)->(f64,f64,f64){let(rf,gf,bf)=(r as f64/255.,g as f64/255.,b as f64/255.);let max=rf.max(gf).max(bf);let min=rf.min(gf).min(bf);let l=(max+min)/2.;if(max-min).abs()<1e-10{return(0.,0.,l);}let d=max-min;let s=if l>0.5{d/(2.-max-min)}else{d/(max+min)};let h=if max==rf{((gf-bf)/d)%6.}else if max==gf{(bf-rf)/d+2.}else{(rf-gf)/d+4.}*60.;(if h<0.{h+360.}else{h},s,l)}
fn hsl_to_rgb(h:f64,s:f64,l:f64)->(u8,u8,u8){if s.abs()<1e-10{let v=(l*255.)as u8;return(v,v,v);}let q=if l<0.5{l*(1.+s)}else{l+s-l*s};let p=2.*l-q;let r=hue_to_rgb(p,q,h/360.+1./3.);let g=hue_to_rgb(p,q,h/360.);let b=hue_to_rgb(p,q,h/360.-1./3.);((r*255.)as u8,(g*255.)as u8,(b*255.)as u8)}
fn hue_to_rgb(p:f64,q:f64,t:f64)->f64{let t=if t<0.{t+1.}else if t>1.{t-1.}else{t};if t<1./6.{p+(q-p)*6.*t}else if t<0.5{q}else if t<2./3.{p+(q-p)*(2./3.-t)*6.}else{p}}

// ═══════════════════════════════════════════
// v9.0 — 扩展WebShell + 编码载荷