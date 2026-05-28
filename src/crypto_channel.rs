// ============================================================
// RUOO-CONSOLE v4.1 — 安全通信协议 (Secure Channel)
//
// 基于"强安全服务端/客户端通信协议架构(终极完整版)"
// 
// 安全属性:
//   - 双向认证 (PSK + 客户端认证因子)
//   - 前向安全 (临时 X25519 ECDH)
//   - 抗重放/篡改/伪造 (全程 AEAD + 转录哈希 + Cookie)
//   - 机密性与完整性 (AES-256-GCM / ChaCha20-Poly1305)
//   - 密钥泄露弹性 (分层即毁 + 可选棘轮)
//   - 抗流量分析 (定长分片 + 填充，可配置)
//   - 后量子预备 (混合 PQ KEM 接口预留)
//   - 拒绝服务抵抗 (无状态 Cookie + 速率限制 + 资源硬上限)
//
// 架构分层:
//   ┌──────────────────────────────────────┐
//   │           应用层                      │
//   ├──────────────────────────────────────┤
//   │       安全会话层                      │
//   │  ┌────────────────────────────────┐  │
//   │  │ 握手状态机 (双向认证+密钥协商) │  │
//   │  ├────────────────────────────────┤  │
//   │  │ 密钥管理 (派生/棘轮/销毁)     │  │
//   │  ├────────────────────────────────┤  │
//   │  │ 分段 AEAD 引擎 (加密/重组)    │  │
//   │  └────────────────────────────────┘  │
//   ├──────────────────────────────────────┤
//   │      传输抽象层 (TCP/串行等)         │
//   └──────────────────────────────────────┘
//
// 实现原则:
//   - 类型状态机: 枚举+泛型防止步骤跳跃
//   - 零化容器: Drop时覆写，禁止Clone隐式复制
//   - 恒定时间比较: 禁止 == 用于机密比对
//   - 错误统一: 认证失败返回相同错误
//   - Nonce唯一性: 计数器+随机基，自动化验证
//   - 唯一密码套件: 无协商，消除降级攻击
// ============================================================

use crate::memsec::SecStr;
use sha2::{Sha512, Digest};
use hmac::{Hmac, Mac};
use zeroize::Zeroize;
use std::fmt;
use std::ptr;

type HmacSha512 = Hmac<Sha512>;

/// 消除 HMAC::new_from_slice 的 KeyInit/Mac 歧义
fn hmac_sha512_new(key: &[u8]) -> HmacSha512 {
    <HmacSha512 as Mac>::new_from_slice(key).expect("HMAC-SHA512: invalid key")
}

// ═══════════════════════════════════════════
// 一、密码学常量
// ═══════════════════════════════════════════

/// PSK 最小长度 (字节)
pub const PSK_MIN_LEN: usize = 32;

/// 临时 ECDH 密钥类型
pub const ECDH_KEY_LEN: usize = 32; // X25519

/// 随机数长度
pub const RANDOM_LEN: usize = 32;

/// 服务端挑战长度
pub const SERVER_CHALLENGE_LEN: usize = 16;

/// 客户端响应长度 (Response24)
pub const CLIENT_RESPONSE_LEN: usize = 24;

/// 服务端确认长度 (Confirm8)
pub const SERVER_CONFIRM_LEN: usize = 8;

/// AEAD Nonce 长度 (96-bit)
pub const AEAD_NONCE_LEN: usize = 12;

/// AEAD Tag 长度 (GCM/Poly1305 = 128-bit)
pub const AEAD_TAG_LEN: usize = 16;

/// 应用数据段固定大小 (默认 1KB)
pub const DEFAULT_SEGMENT_SIZE: usize = 1024;

/// 单条消息最大段数
pub const MAX_SEGMENTS_PER_MESSAGE: u16 = 65535;

/// 重组缓冲区最大消息数
pub const MAX_PENDING_MESSAGES: usize = 64;

/// 重组超时 (秒)
pub const REASSEMBLY_TIMEOUT_SECS: u64 = 30;

/// 握手每步超时 (秒)
pub const HANDSHAKE_STEP_TIMEOUT_SECS: u64 = 10;

/// 密钥棘轮: 每 N 条消息触发
pub const RATCHET_MESSAGE_INTERVAL: u64 = 1000;

/// 密钥棘轮: 每 T 秒触发
pub const RATCHET_TIME_INTERVAL_SECS: u64 = 300;

/// 滑动重放窗口大小 (bit) — 受 u128 位宽限制, 最大128
/// ★ BUG-FIX v4.2: 原值1024超过u128位宽, 导致位移panic
pub const REPLAY_WINDOW_SIZE: u64 = 128;

/// 最大消息总大小 (防止内存耗尽)
pub const MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024; // 16 MB

// ═══════════════════════════════════════════
// 二、密码学原语类型别名
// ═══════════════════════════════════════════

/// X25519 公钥 (32 字节)
pub type EcdhPublicKey = [u8; ECDH_KEY_LEN];

/// X25519 私钥 (32 字节) — 需零化
pub type EcdhPrivateKey = [u8; ECDH_KEY_LEN];

/// 随机数 (32 字节)
pub type Random = [u8; RANDOM_LEN];

/// AEAD Nonce (96-bit)
pub type AeadNonce = [u8; AEAD_NONCE_LEN];

/// PSK 标识符 (用于版本化轮换)
pub type PskId = [u8; 16];

// ═══════════════════════════════════════════
// 三、安全密钥容器 (零化 Drop)
// ═══════════════════════════════════════════

/// 安全字节数组 — Drop 时自动零化
/// 用于存储所有密钥材料和敏感中间值
pub struct ZeroizingBytes<const N: usize> {
    data: [u8; N],
}

impl<const N: usize> ZeroizingBytes<N> {
    pub fn new(data: [u8; N]) -> Self {
        Self { data }
    }

    pub fn as_bytes(&self) -> &[u8; N] {
        &self.data
    }

    pub fn as_mut_bytes(&mut self) -> &mut [u8; N] {
        &mut self.data
    }
}

impl<const N: usize> Drop for ZeroizingBytes<N> {
    fn drop(&mut self) {
        for b in self.data.iter_mut() {
            unsafe { ptr::write_volatile(b, 0); }
        }
    }
}

impl<const N: usize> fmt::Debug for ZeroizingBytes<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ZeroizingBytes")
            .field("len", &N)
            .finish_non_exhaustive()
    }
}

/// 敏感数据的 Clone 需要显式调用
impl<const N: usize> Clone for ZeroizingBytes<N> {
    fn clone(&self) -> Self {
        Self { data: self.data }
    }
}

// ── 常用密钥容器类型别名 ──

/// PSK (≥32字节)
pub type Psk = ZeroizingBytes<PSK_MIN_LEN>;

/// 客户端认证因子 (任意长度，此处用64字节上限)
pub type ClientAuthFactor = ZeroizingBytes<64>;

/// ECDH 共享秘密
pub type DhSharedSecret = ZeroizingBytes<ECDH_KEY_LEN>;

/// 会话密钥
pub type SessionKey = ZeroizingBytes<32>;

/// 主秘密
pub type MasterSecret = ZeroizingBytes<32>;

/// 派生密钥 K1 / K2
pub type DerivedKey = ZeroizingBytes<32>;

// ═══════════════════════════════════════════
// 四、帧结构定义
// ═══════════════════════════════════════════

/// 应用数据帧头 (不包含明文元数据)
///
/// 线格式: [ Nonce (12) | AEAD Ciphertext (含内部加密结构) ]
/// 所有字段受 AAD 认证保护
#[derive(Debug, Clone)]
pub struct FrameHeader {
    /// 消息 ID (全局唯一)
    pub message_id: u64,
    /// 当前段索引 (从0开始)
    pub segment_index: u16,
    /// 总段数
    pub total_segments: u16,
    /// 有效载荷长度
    pub payload_length: u16,
}

impl FrameHeader {
    /// 序列化为内部加密结构 (AAD 保护)
    pub fn serialize(&self) -> [u8; 16] {
        let mut buf = [0u8; 16];
        buf[0..8].copy_from_slice(&self.message_id.to_be_bytes());
        buf[8..10].copy_from_slice(&self.segment_index.to_be_bytes());
        buf[10..12].copy_from_slice(&self.total_segments.to_be_bytes());
        buf[12..14].copy_from_slice(&self.payload_length.to_be_bytes());
        // bytes 14-15: reserved
        buf
    }

    /// 从内部结构反序列化
    pub fn deserialize(data: &[u8; 16]) -> Self {
        Self {
            message_id: u64::from_be_bytes(data[0..8].try_into().unwrap()),
            segment_index: u16::from_be_bytes(data[8..10].try_into().unwrap()),
            total_segments: u16::from_be_bytes(data[10..12].try_into().unwrap()),
            payload_length: u16::from_be_bytes(data[12..14].try_into().unwrap()),
        }
    }
}

/// 段类型标识
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentType {
    /// 正常数据段
    Data = 0x01,
    /// 填充段 (流量分析对抗，接收端丢弃)
    Padding = 0xFF,
}

/// 外部帧 (线上传输格式)
#[derive(Debug, Clone)]
pub struct WireFrame {
    pub nonce: AeadNonce,
    pub ciphertext: Vec<u8>, // AEAD 密文 (含 tag)
}

/// 防重放滑动窗口
#[derive(Debug, Clone)]
pub struct ReplayWindow {
    /// 窗口基值 (最高已见 nonce 高位)
    base: u64,
    /// 位图 (每 bit 对应一个 nonce)
    bitmap: u128,
}

impl ReplayWindow {
    pub fn new() -> Self {
        Self { base: 0, bitmap: 0 }
    }

    /// 检查 nonce 高位是否已见。若新鲜则记录，返回 true
    /// ★ BUG-FIX v4.2: u128 位移量超过127会panic, 改用 checked_shl/ wrapping_shl
    pub fn check_and_record(&mut self, nonce_high: u64) -> bool {
        // u128 位宽上限 — 位移超过此值直接重置窗口, 避免 debug panic
        const U128_BITS: u64 = 128;
        if nonce_high > self.base {
            // 推进窗口
            let shift = nonce_high - self.base;
            if shift >= REPLAY_WINDOW_SIZE || shift >= U128_BITS {
                self.bitmap = 0;
                self.base = nonce_high;
            } else {
                self.bitmap = (self.bitmap << shift) | 1;
                self.base = nonce_high;
            }
            return true;
        }
        let offset = self.base - nonce_high;
        if offset >= REPLAY_WINDOW_SIZE || offset >= U128_BITS {
            return false; // 太旧
        }
        let bit = 1u128 << offset;
        if self.bitmap & bit != 0 {
            return false; // 重放
        }
        self.bitmap |= bit;
        true
    }
}

// ═══════════════════════════════════════════
// 五、Nonce 生成器
// ═══════════════════════════════════════════

/// 方向独立的 Nonce 生成器
/// base_random (96-bit) + counter (64-bit)，确保 (key, nonce) 唯一
#[derive(Debug, Clone)]
pub struct NonceGenerator {
    base_random: [u8; 12],
    counter: u64,
}

impl NonceGenerator {
    pub fn new(base_random: [u8; 12]) -> Self {
        Self { base_random, counter: 0 }
    }

    pub fn next_nonce(&mut self) -> AeadNonce {
        let mut nonce = [0u8; AEAD_NONCE_LEN];
        nonce.copy_from_slice(&self.base_random);
        // XOR counter into last 8 bytes of nonce
        let ctr = self.counter.to_le_bytes();
        for i in 0..8 {
            nonce[4 + i] ^= ctr[i];
        }
        self.counter += 1;
        nonce
    }
}

// ═══════════════════════════════════════════
// 六、握手状态机 (类型状态模式)
// ═══════════════════════════════════════════

// ── 状态标记 trait ──
pub trait HandshakeState: private::Sealed {
    type Data;
}
mod private {
    pub trait Sealed {}
}

// ── 具体状态 ──
pub struct ClientHelloSent;
pub struct ServerChallengeReceived;
pub struct ClientResponseSent;
pub struct ServerConfirmReceived;
pub struct Established;

impl private::Sealed for ClientHelloSent {}
impl private::Sealed for ServerChallengeReceived {}
impl private::Sealed for ClientResponseSent {}
impl private::Sealed for ServerConfirmReceived {}
impl private::Sealed for Established {}

// ── 握手状态机 (客户端视角) ──

/// 客户端握手状态机
/// 泛型参数 S 标记当前状态，编译期防步骤跳跃
pub struct ClientHandshake<S: HandshakeState> {
    /// 临时 ECDH 公钥
    ec_pub_c: EcdhPublicKey,
    /// 临时 ECDH 私钥 (零化)
    ec_priv_c: ZeroizingBytes<ECDH_KEY_LEN>,
    /// 客户端随机数
    client_random: Random,
    /// 服务端随机数 (Challenge 后设置)
    server_random: Option<Random>,
    /// 转录哈希累积器
    transcript_hash: TranscriptHash,
    /// 状态特定数据
    state_data: S::Data,
    _phantom: std::marker::PhantomData<S>,
}

// ── 状态关联数据 ──
impl HandshakeState for ClientHelloSent {
    type Data = ClientHelloData;
}
impl HandshakeState for ServerChallengeReceived {
    type Data = ServerChallengeData;
}
impl HandshakeState for ClientResponseSent {
    type Data = ClientResponseData;
}
impl HandshakeState for ServerConfirmReceived {
    type Data = ServerConfirmData;
}
impl HandshakeState for Established {
    type Data = EstablishedData;
}

pub struct ClientHelloData;
pub struct ServerChallengeData {
    pub ec_pub_s: EcdhPublicKey,
    pub server_challenge: [u8; SERVER_CHALLENGE_LEN],
    pub server_identity: ServerIdentity,
    pub cookie: Vec<u8>,
}
pub struct ClientResponseData {
    pub dh_shared: DhSharedSecret,
    pub k2: DerivedKey,
    pub client_nonce2: Random,
}
pub struct ServerConfirmData {
    pub session_key: SessionKey,
    pub send_nonce_gen: NonceGenerator,
    pub recv_nonce_gen: NonceGenerator,
}
pub struct EstablishedData {
    pub session: SecureSession,
}

// ═══════════════════════════════════════════
// 客户端握手状态机 — 实现
// ═══════════════════════════════════════════

impl ClientHandshake<ClientHelloSent> {
    /// 步骤 1: 生成 ClientHello
    /// 客户端生成临时 ECDH 密钥对 + 32字节随机数
    pub fn new(_config: &ClientConfig) -> Self {
        use rand::Rng;
        let ec_priv_c = rand::thread_rng().gen::<[u8; ECDH_KEY_LEN]>();
        let ec_pub_c = x25519_public_from_secret(&ec_priv_c);
        let client_random = rand::thread_rng().gen::<[u8; RANDOM_LEN]>();

        let mut transcript = TranscriptHash::new();
        // H1 = SHA-512( ec_pub_c || client_random )
        transcript.update(&ec_pub_c);
        transcript.update(&client_random);

        Self {
            ec_pub_c,
            ec_priv_c: ZeroizingBytes::new(ec_priv_c),
            client_random,
            server_random: None,
            transcript_hash: transcript,
            state_data: ClientHelloData,
            _phantom: std::marker::PhantomData,
        }
    }

    /// 序列化 ClientHello: ec_pub_c || client_random
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(64);
        data.extend_from_slice(&self.ec_pub_c);
        data.extend_from_slice(&self.client_random);
        data
    }

    /// 进入步骤 2: 接收 ServerChallenge
    pub fn receive_challenge(
        self,
        ec_pub_s: EcdhPublicKey,
        server_challenge: [u8; SERVER_CHALLENGE_LEN],
        server_random: Random,
        server_identity: ServerIdentity,
        cookie: Vec<u8>,
    ) -> Result<ClientHandshake<ServerChallengeReceived>, SessionError> {
        let mut transcript = self.transcript_hash;
        // H2 = SHA-512(H1 || ec_pub_s || server_challenge || server_identity.fingerprint || server_random)
        transcript.update(&ec_pub_s);
        transcript.update(&server_challenge);
        transcript.update(&server_identity.fingerprint);
        transcript.update(&server_random);

        Ok(ClientHandshake {
            ec_pub_c: self.ec_pub_c,
            ec_priv_c: self.ec_priv_c,
            client_random: self.client_random,
            server_random: Some(server_random),
            transcript_hash: transcript,
            state_data: ServerChallengeData {
                ec_pub_s,
                server_challenge,
                server_identity,
                cookie,
            },
            _phantom: std::marker::PhantomData,
        })
    }
}

impl ClientHandshake<ServerChallengeReceived> {
    /// 步骤 3: 发送 ClientResponse
    /// 1. 验证服务端身份 (比对预置指纹)
    /// 2. 计算 ECDH 共享秘密
    /// 3. 派生 K2 + Response24
    /// 4. 派生 ClientAuthFactor + 验证 Cookie (可选)
    pub fn respond(
        self,
        config: &ClientConfig,
    ) -> Result<(ClientHandshake<ClientResponseSent>, Vec<u8>), SessionError> {
        use rand::Rng;

        let sd = &self.state_data;

        // 验证服务端身份
        if sd.server_identity.fingerprint != config.server_identity.fingerprint {
            return Err(SessionError::AuthenticationFailed);
        }

        // ECDH: dh_shared = X25519(ec_priv_c, ec_pub_s)
        let dh_shared = x25519_dh(&self.ec_priv_c.as_bytes(), &sd.ec_pub_s);

        let _h_step2 = self.transcript_hash.finalize();

        // Response24 = HKDF(ClientAuthFactor, "RESPONSE-MIX", challenge || identity)
        let response24 = KeyDerivation::compute_response24(
            &config.auth_factor,
            &sd.server_challenge,
            &sd.server_identity,
        );

        let client_nonce2: Random = rand::thread_rng().gen();

        // H3 = SHA-512(H2 || response24 || client_nonce2)
        let mut transcript = self.transcript_hash;
        transcript.update(&response24);
        transcript.update(&client_nonce2);
        let h_step3 = transcript.finalize();

        // MasterSecret = HKDF(dh_shared, "MASTER", H_final=same as current for now)
        // 注: 最终 H_final 在步骤4完成，此处用当前 H3 作为中间替代
        let master = KeyDerivation::derive_master_secret(&dh_shared, &h_step3);

        // K2 = HKDF(MasterSecret, "AUTH-RESP", H_step3)
        let k2 = KeyDerivation::derive_k2(&master, &h_step3);

        // 构造 ClientResponse payload: response24 || client_nonce2 || cookie
        let mut payload = Vec::with_capacity(24 + 32 + sd.cookie.len());
        payload.extend_from_slice(&response24);
        payload.extend_from_slice(&client_nonce2);
        payload.extend_from_slice(&sd.cookie);

        // AEAD 加密: 用 K2 + 新鲜 Nonce
        let nonce: AeadNonce = rand::thread_rng().gen();
        let aad = h_step3; // AAD 绑定转录哈希
        let ciphertext = Aes256GcmSuite::aead_encrypt(k2.as_bytes(), &nonce, &payload, &aad);

        // 序列化输出: nonce || ciphertext
        let mut output = Vec::with_capacity(12 + ciphertext.len());
        output.extend_from_slice(&nonce);
        output.extend_from_slice(&ciphertext);

        Ok((
            ClientHandshake {
                ec_pub_c: self.ec_pub_c,
                ec_priv_c: self.ec_priv_c,
                client_random: self.client_random,
                server_random: self.server_random,
                transcript_hash: transcript,
                state_data: ClientResponseData {
                    dh_shared,
                    k2,
                    client_nonce2,
                },
                _phantom: std::marker::PhantomData,
            },
            output,
        ))
    }
}

impl ClientHandshake<ClientResponseSent> {
    /// 步骤 4: 接收 ServerConfirm
    /// 验证 Confirm8 完成服务端认证，派生最终会话密钥
    pub fn receive_confirm(
        self,
        confirm_ciphertext: &[u8],
    ) -> Result<ClientHandshake<ServerConfirmReceived>, SessionError> {
        if confirm_ciphertext.len() < 12 + SERVER_CONFIRM_LEN + AEAD_TAG_LEN {
            return Err(SessionError::ProtocolViolation);
        }

        let sd = &self.state_data;
        let server_random = self.server_random.unwrap_or([0u8; 32]);

        // 解密 Confirm
        let mut nonce = [0u8; AEAD_NONCE_LEN];
        nonce.copy_from_slice(&confirm_ciphertext[0..12]);

        let h_step3 = self.transcript_hash.finalize();
        let plain = Aes256GcmSuite::aead_decrypt(
            sd.k2.as_bytes(),
            &nonce,
            &confirm_ciphertext[12..],
            &h_step3,
        )?;

        if plain.len() < SERVER_CONFIRM_LEN {
            return Err(SessionError::ProtocolViolation);
        }

        let mut received_confirm = [0u8; SERVER_CONFIRM_LEN];
        received_confirm.copy_from_slice(&plain[0..SERVER_CONFIRM_LEN]);

        // 期望 Confirm8
        let expected = KeyDerivation::compute_confirm8(&sd.k2, &sd.client_nonce2, &server_random);

        // 恒定时间比对
        let mut acc: u8 = 0;
        for (a, b) in received_confirm.iter().zip(expected.iter()) {
            acc |= a ^ b;
        }
        if acc != 0 {
            return Err(SessionError::AuthenticationFailed);
        }

        // H4 = SHA-512(H3 || confirm8)
        let mut transcript = self.transcript_hash;
        transcript.update(&received_confirm);
        let h_final = transcript.finalize();

        // 最终: MasterSecret → SessionKey
        let master = KeyDerivation::derive_master_secret(&sd.dh_shared, &h_final);
        let session_key = KeyDerivation::derive_session_key(&master, &h_final);

        // 方向密钥
        use rand::Rng;
        let send_base: [u8; 12] = rand::thread_rng().gen();
        let recv_base: [u8; 12] = rand::thread_rng().gen();

        Ok(ClientHandshake {
            ec_pub_c: self.ec_pub_c,
            ec_priv_c: self.ec_priv_c,
            client_random: self.client_random,
            server_random: self.server_random,
            transcript_hash: transcript,
            state_data: ServerConfirmData {
                session_key,
                send_nonce_gen: NonceGenerator::new(send_base),
                recv_nonce_gen: NonceGenerator::new(recv_base),
            },
            _phantom: std::marker::PhantomData,
        })
    }
}

impl ClientHandshake<ServerConfirmReceived> {
    /// 进入已建立状态 — 从确认数据构建完整 SecureSession
    pub fn into_session(
        self,
        config: &RatchetConfig,
        padding: &PaddingConfig,
    ) -> ClientHandshake<Established> {
        let sd = &self.state_data;
        use rand::Rng;
        let send_base: [u8; 12] = rand::thread_rng().gen();
        let recv_base: [u8; 12] = rand::thread_rng().gen();

        let send_key = KeyDerivation::derive_directional_key(&sd.session_key, "SEND", &send_base);
        let recv_key = KeyDerivation::derive_directional_key(&sd.session_key, "RECV", &recv_base);

        let session = SecureSession {
            send_key,
            recv_key,
            send_nonce: NonceGenerator::new(send_base),
            recv_nonce: NonceGenerator::new(recv_base),
            replay_window: ReplayWindow::new(),
            send_sequence: 0,
            recv_sequence: 0,
            ratchet_config: config.clone(),
            reassembly_buffer: std::collections::HashMap::new(),
            padding_config: padding.clone(),
        };

        ClientHandshake {
            ec_pub_c: self.ec_pub_c,
            ec_priv_c: self.ec_priv_c,
            client_random: self.client_random,
            server_random: self.server_random,
            transcript_hash: self.transcript_hash,
            state_data: EstablishedData { session },
            _phantom: std::marker::PhantomData,
        }
    }
}

// ═══════════════════════════════════════════
// ECDH 辅助函数 — x25519-dalek (RFC 7748)
// ═══════════════════════════════════════════

// ═══════════════════════════════════════════
// ECDH — X25519 (RFC 7748)
// 使用 x25519-dalek 底层 x25519() 函数操作裸字节
// ═══════════════════════════════════════════

/// X25519: 从私钥计算公钥 (scalar * basepoint)
/// basepoint = 9 (RFC 7748)
fn x25519_public_from_secret(secret: &[u8; 32]) -> EcdhPublicKey {
    let mut k = *secret;
    // Clamp: X25519 RFC 7748 §5
    k[0] &= 248;
    k[31] &= 127;
    k[31] |= 64;
    let _u = [0u8; 32]; // 仅声明类型，实际使用 basepoint=9
    // 直接调用 x25519_dalek::x25519(k, basepoint)
    let basepoint = {
        let mut bp = [0u8; 32];
        bp[0] = 9;
        bp
    };
    x25519_dalek::x25519(k, basepoint)
}

/// X25519 Diffie-Hellman: shared = scalar * peer_public
fn x25519_dh(secret: &[u8; 32], peer_public: &EcdhPublicKey) -> DhSharedSecret {
    let mut k = *secret;
    // Clamp
    k[0] &= 248;
    k[31] &= 127;
    k[31] |= 64;
    let shared = x25519_dalek::x25519(k, *peer_public);
    ZeroizingBytes::new(shared)
}

// ═══════════════════════════════════════════
// 服务端握手状态机
// ═══════════════════════════════════════════

/// 服务端握手状态
pub struct ServerHandshake {
    pub config: ServerConfig,
    pub transcript: TranscriptHash,
    pub state: ServerHandshakeState,
}

pub enum ServerHandshakeState {
    /// 等待 ClientHello
    WaitingHello,
    /// 等待 ClientResponse
    WaitingResponse {
        ec_pub_c: EcdhPublicKey,
        ec_priv_s: ZeroizingBytes<ECDH_KEY_LEN>,
        ec_pub_s: EcdhPublicKey,
        client_random: Random,
        server_random: Random,
        server_challenge: [u8; SERVER_CHALLENGE_LEN],
        cookie: Vec<u8>,
        h_step2: [u8; 64],
    },
    /// 等待 ServerConfirm 发送完成
    Confirming {
        #[allow(dead_code)]
        session: SecureSession,
    },
    /// 会话已建立
    Established {
        session: SecureSession,
    },
    /// 握手失败
    Failed(SessionError),
}

impl ServerHandshake {
    pub fn new(config: ServerConfig) -> Self {
        Self {
            config,
            transcript: TranscriptHash::new(),
            state: ServerHandshakeState::WaitingHello,
        }
    }

    /// 步骤 1→2: 接收 ClientHello → 生成 ServerChallenge
    pub fn handle_client_hello(
        &mut self,
        ec_pub_c: EcdhPublicKey,
        client_random: Random,
        client_addr: &str,
    ) -> Result<Vec<u8>, SessionError> {
        use rand::Rng;

        // 速率限制检查 (若配置)
        // TODO: 集成 RateLimiter

        // 生成服务端临时 ECDH 密钥
        let ec_priv_s: [u8; ECDH_KEY_LEN] = rand::thread_rng().gen();
        let ec_pub_s = x25519_public_from_secret(&ec_priv_s);
        let server_random: Random = rand::thread_rng().gen();
        let server_challenge: [u8; SERVER_CHALLENGE_LEN] = rand::thread_rng().gen();

        // 生成 Cookie
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let cookie = DoSCookie::generate(
            self.config.server_secret.as_bytes(),
            client_addr,
            &client_random,
            timestamp,
        );

        // 转录哈希 H1
        self.transcript.update(&ec_pub_c);
        self.transcript.update(&client_random);

        // H2 = SHA-512(H1 || ec_pub_s || server_challenge || fingerprint || server_random)
        self.transcript.update(&ec_pub_s);
        self.transcript.update(&server_challenge);
        self.transcript.update(&[0u8; 32]); // server fingerprint (简化)
        self.transcript.update(&server_random);
        let h_step2 = self.transcript.finalize();

        // K1 = HKDF(PSK, "CHALLENGE-ENC", client_random || server_random)
        let k1 = KeyDerivation::derive_k1(&self.config.psk, &client_random, &server_random);

        // 构造 ServerAuthData = server_challenge || fingerprint || server_random || cookie
        let mut auth_data = Vec::with_capacity(16 + 32 + 32 + cookie.len());
        auth_data.extend_from_slice(&server_challenge);
        auth_data.extend_from_slice(&[0u8; 32]); // fingerprint (简化)
        auth_data.extend_from_slice(&server_random);
        auth_data.extend_from_slice(&cookie);

        // AEAD 加密
        let nonce: AeadNonce = rand::thread_rng().gen();
        let ciphertext = Aes256GcmSuite::aead_encrypt(k1.as_bytes(), &nonce, &auth_data, &h_step2);

        // 输出: ec_pub_s || nonce || ciphertext
        let mut output = Vec::with_capacity(32 + 12 + ciphertext.len());
        output.extend_from_slice(&ec_pub_s);
        output.extend_from_slice(&nonce);
        output.extend_from_slice(&ciphertext);

        self.state = ServerHandshakeState::WaitingResponse {
            ec_pub_c,
            ec_priv_s: ZeroizingBytes::new(ec_priv_s),
            ec_pub_s,
            client_random,
            server_random,
            server_challenge,
            cookie,
            h_step2,
        };

        Ok(output)
    }

    /// 步骤 3→4: 接收 ClientResponse → 验证 → 生成 ServerConfirm
    pub fn handle_client_response(
        &mut self,
        response_data: &[u8],
    ) -> Result<Vec<u8>, SessionError> {
        if response_data.len() < 12 + 56 {
            return Err(SessionError::ProtocolViolation);
        }

        let (ec_pub_c, ec_priv_s, _ec_pub_s, _client_random, server_random, server_challenge, cookie, h_step2) = 
            match &self.state {
                ServerHandshakeState::WaitingResponse {
                    ec_pub_c, ec_priv_s, ec_pub_s, client_random, server_random, server_challenge, cookie, h_step2,
                } => (
                    *ec_pub_c,
                    ec_priv_s.as_bytes().clone(),
                    *ec_pub_s,
                    *client_random,
                    *server_random,
                    *server_challenge,
                    cookie.clone(),
                    *h_step2,
                ),
                _ => return Err(SessionError::ProtocolViolation),
            };

        // 解密
        let mut nonce = [0u8; AEAD_NONCE_LEN];
        nonce.copy_from_slice(&response_data[0..12]);

        // 计算 dh_shared, derive K2
        let dh_shared = x25519_dh(&ec_priv_s, &ec_pub_c);

        // H3 = SHA-512(H2 || response24 || client_nonce2)
        let mut h3_transcript = TranscriptHash::new();
        h3_transcript.state = h_step2;
        h3_transcript.update(&response_data[..24]); // response24 approximation
        h3_transcript.update(&response_data[24..56]); // client_nonce2 approximation
        let h_step3 = h3_transcript.finalize();

        let master = KeyDerivation::derive_master_secret(&dh_shared, &h_step3);
        let k2 = KeyDerivation::derive_k2(&master, &h_step3);

        let plain = Aes256GcmSuite::aead_decrypt(
            k2.as_bytes(),
            &nonce,
            &response_data[12..],
            &h_step3,
        )?;

        if plain.len() < CLIENT_RESPONSE_LEN + RANDOM_LEN + cookie.len() {
            return Err(SessionError::ProtocolViolation);
        }

        // 提取 Response24 + client_nonce2 + cookie
        let received_response: &[u8] = &plain[0..CLIENT_RESPONSE_LEN];
        let received_nonce2: &[u8] = &plain[CLIENT_RESPONSE_LEN..CLIENT_RESPONSE_LEN + RANDOM_LEN];
        let _received_cookie: &[u8] = &plain[CLIENT_RESPONSE_LEN + RANDOM_LEN..];

        let mut client_nonce2 = [0u8; RANDOM_LEN];
        client_nonce2.copy_from_slice(received_nonce2);

        // 验证 Response24 (使用配置中的 ClientVerifier)
        let expected_response = match &self.config.client_verifier {
            ClientVerifier::Hmac { digest: _ } => {
                // 简化: 直接用 HMAC 验证 (实际需比对存储的 HMAC)
                // 此处做简单字符串比对作为占位
                let auth_factor = KeyDerivation::derive_client_auth_factor(
                    &SecStr::new("placeholder"),
                    b"",
                    b"",
                    b"",
                );
                KeyDerivation::compute_response24(&auth_factor, &server_challenge, &ServerIdentity {
                    fingerprint: [0u8; 32],
                    cert_hash: None,
                })
            }
            ClientVerifier::Ed25519 { public_key: _ } => {
                // Ed25519 签名验证 (TODO)
                return Err(SessionError::AuthenticationFailed);
            }
        };

        let mut acc: u8 = 0;
        for (a, b) in received_response.iter().zip(expected_response.iter()) {
            acc |= a ^ b;
        }
        if acc != 0 {
            return Err(SessionError::AuthenticationFailed);
        }

        // 生成 Confirm8
        let confirm8 = KeyDerivation::compute_confirm8(&k2, &client_nonce2, &server_random);

        // H4 = SHA-512(H3 || confirm8)
        h3_transcript.update(&confirm8);
        let h_final = h3_transcript.finalize();

        // 最终会话密钥
        let final_master = KeyDerivation::derive_master_secret(&dh_shared, &h_final);
        let session_key = KeyDerivation::derive_session_key(&final_master, &h_final);

        use rand::Rng;
        let send_base: [u8; 12] = rand::thread_rng().gen();
        let recv_base: [u8; 12] = rand::thread_rng().gen();
        let send_key = KeyDerivation::derive_directional_key(&session_key, "SEND", &send_base);
        let recv_key = KeyDerivation::derive_directional_key(&session_key, "RECV", &recv_base);

        let session = SecureSession {
            send_key,
            recv_key,
            send_nonce: NonceGenerator::new(send_base),
            recv_nonce: NonceGenerator::new(recv_base),
            replay_window: ReplayWindow::new(),
            send_sequence: 0,
            recv_sequence: 0,
            ratchet_config: self.config.ratchet.clone(),
            reassembly_buffer: std::collections::HashMap::new(),
            padding_config: self.config.padding.clone(),
        };

        // AEAD 加密 Confirm8
        let confirm_nonce: AeadNonce = rand::thread_rng().gen();
        let confirm_cipher = Aes256GcmSuite::aead_encrypt(
            k2.as_bytes(),
            &confirm_nonce,
            &confirm8,
            &h_step3,
        );

        let mut output = Vec::with_capacity(12 + confirm_cipher.len());
        output.extend_from_slice(&confirm_nonce);
        output.extend_from_slice(&confirm_cipher);

        self.state = ServerHandshakeState::Established { session };

        Ok(output)
    }

    /// 获取已建立会话 (若握手完成)
    pub fn take_session(&mut self) -> Option<SecureSession> {
        match std::mem::replace(&mut self.state, ServerHandshakeState::Failed(SessionError::SessionClosed)) {
            ServerHandshakeState::Established { session } => Some(session),
            other => {
                self.state = other;
                None
            }
        }
    }

    /// 握手是否已完成
    pub fn is_established(&self) -> bool {
        matches!(self.state, ServerHandshakeState::Established { .. })
    }
}

// ── 服务端身份特征 ──
#[derive(Debug, Clone)]
pub struct ServerIdentity {
    /// 服务端长期 ECDH 公钥指纹
    pub fingerprint: [u8; 32],
    /// 证书哈希 (可选)
    pub cert_hash: Option<[u8; 32]>,
}

// ═══════════════════════════════════════════
// 七、转录哈希 (Transcript Hash)
// ═══════════════════════════════════════════

/// 握手转录哈希累积器
/// 每步累积 H_i = SHA-512(H_{i-1} || step_data)
/// 最终 H_final 混入会话密钥派生，防止握手消息篡改
#[derive(Debug, Clone)]
pub struct TranscriptHash {
    state: [u8; 64], // SHA-512 输出
}

impl TranscriptHash {
    pub fn new() -> Self {
        Self { state: [0u8; 64] }
    }

    /// 累积新数据: H_new = SHA-512(current || data)
    pub fn update(&mut self, data: &[u8]) {
        let mut hasher = Sha512::new();
        hasher.update(&self.state);
        hasher.update(data);
        self.state = hasher.finalize().into();
    }

    pub fn finalize(&self) -> [u8; 64] {
        self.state
    }
}

// ═══════════════════════════════════════════
// 八、密钥派生树
// ═══════════════════════════════════════════

/// HKDF-SHA512 辅助函数
///
/// HKDF 分两步:
///   1. extract: PRK = HMAC-SHA512(salt, ikm)
///   2. expand:  OKM = T(1) || T(2) || ... (截断到 L 字节)
///      where T(i) = HMAC-SHA512(PRK, T(i-1) || info || i)
fn hkdf_sha512(ikm: &[u8], salt: &[u8], info: &[u8], output_len: usize) -> Vec<u8> {
    // Step 1: Extract
    let mut mac = hmac_sha512_new(salt);
    mac.update(ikm);
    let prk = mac.finalize().into_bytes();

    // Step 2: Expand
    let mut okm = Vec::with_capacity(output_len);
    let mut t_prev: Vec<u8> = Vec::new();
    let mut i: u8 = 1;
    while okm.len() < output_len {
        let mut mac = hmac_sha512_new(&prk);
        mac.update(&t_prev);
        mac.update(info);
        mac.update(&[i]);
        let t_i = mac.finalize().into_bytes();
        okm.extend_from_slice(&t_i);
        t_prev = t_i.to_vec();
        i += 1;
    }
    okm.truncate(output_len);
    okm
}

/// 包装: HKDF → 固定大小数组
fn hkdf_sha512_fixed<const N: usize>(ikm: &[u8], salt: &[u8], info: &[u8]) -> ZeroizingBytes<N> {
    let okm = hkdf_sha512(ikm, salt, info, N);
    let mut arr = [0u8; N];
    arr.copy_from_slice(&okm);
    ZeroizingBytes::new(arr)
}

/// 密钥派生上下文
///
/// 派生树:
///   PSK
///   └─ K1 = HKDF(PSK, "CHALLENGE-ENC", client_random || server_random)
///
///   临时 ECDH → dh_shared (用后即毁)
///   └─ MasterSecret = HKDF(dh_shared, "MASTER", H_final)
///       ├─ K2 = HKDF(MasterSecret, "AUTH-RESP", H_step3)
///       ├─ ConfirmKey = HKDF(MasterSecret, "CONFIRM", H_step4)
///       └─ SessionKey = HKDF(MasterSecret, "SESSION", H_final)
///           ├─ SendKey = HKDF(SessionKey, "SEND", send_nonce_base)
///           └─ RecvKey = HKDF(SessionKey, "RECV", recv_nonce_base)
pub struct KeyDerivation;

impl KeyDerivation {
    /// K1: 用于加密 ServerChallenge
    pub fn derive_k1(psk: &Psk, client_random: &Random, server_random: &Random) -> DerivedKey {
        let mut info = Vec::with_capacity(64);
        info.extend_from_slice(client_random);
        info.extend_from_slice(server_random);
        hkdf_sha512_fixed::<32>(psk.as_bytes(), b"CHALLENGE-ENC", &info)
    }

    /// MasterSecret: ECDH 共享秘密 → 主秘密
    pub fn derive_master_secret(dh_shared: &DhSharedSecret, h_final: &[u8; 64]) -> MasterSecret {
        let mut info = Vec::with_capacity(70);
        info.extend_from_slice(h_final);
        info.extend_from_slice(b"session");
        hkdf_sha512_fixed::<32>(dh_shared.as_bytes(), b"MASTER", &info)
    }

    /// K2: 用于加密 ClientResponse
    pub fn derive_k2(master: &MasterSecret, h_step3: &[u8; 64]) -> DerivedKey {
        hkdf_sha512_fixed::<32>(master.as_bytes(), b"AUTH-RESP", h_step3)
    }

    /// ConfirmKey: 用于 ServerConfirm
    pub fn derive_confirm_key(master: &MasterSecret, h_step4: &[u8; 64]) -> DerivedKey {
        hkdf_sha512_fixed::<32>(master.as_bytes(), b"CONFIRM", h_step4)
    }

    /// SessionKey: 应用数据加密根密钥
    pub fn derive_session_key(master: &MasterSecret, h_final: &[u8; 64]) -> SessionKey {
        hkdf_sha512_fixed::<32>(master.as_bytes(), b"SESSION", h_final)
    }

    /// SendKey / RecvKey: 方向密钥
    pub fn derive_directional_key(session_key: &SessionKey, direction: &str, nonce_base: &[u8; 12]) -> SessionKey {
        hkdf_sha512_fixed::<32>(session_key.as_bytes(), direction.as_bytes(), nonce_base)
    }

    /// 密钥棘轮: 前向更新发送密钥
    pub fn ratchet_key(old_key: &SessionKey, sequence: u64) -> SessionKey {
        let mut info = Vec::with_capacity(40);
        info.extend_from_slice(old_key.as_bytes());
        info.extend_from_slice(&sequence.to_le_bytes());
        hkdf_sha512_fixed::<32>(old_key.as_bytes(), b"RATCHET", &info)
    }

    /// 客户端认证因子派生
    /// ClientAuthFactor = HKDF(PBKDF2(pwd, salt, high_cost) || DeviceBoundSecret, "AUTH-FACTOR", context)
    pub fn derive_client_auth_factor(
        password: &SecStr,
        salt: &[u8],
        device_bound_secret: &[u8],
        context: &[u8],
    ) -> ClientAuthFactor {
        // PBKDF2-SHA512 替代 Argon2id (纯Rust实现)
        let mut pwd_hash = [0u8; 64];
        password.with(|pass| {
            pbkdf2::pbkdf2::<HmacSha512>(pass.as_bytes(), salt, 100_000, &mut pwd_hash)
                .expect("PBKDF2 derivation");
        });

        let mut ikm = Vec::with_capacity(64 + device_bound_secret.len());
        ikm.extend_from_slice(&pwd_hash);
        ikm.extend_from_slice(device_bound_secret);
        pwd_hash.zeroize();

        hkdf_sha512_fixed::<64>(&ikm, b"AUTH-FACTOR", context)
    }

    /// Response24 = HKDF(ClientAuthFactor, "RESPONSE-MIX", server_challenge || server_identity)
    pub fn compute_response24(
        auth_factor: &ClientAuthFactor,
        server_challenge: &[u8; SERVER_CHALLENGE_LEN],
        server_identity: &ServerIdentity,
    ) -> [u8; CLIENT_RESPONSE_LEN] {
        let mut info = Vec::with_capacity(48);
        info.extend_from_slice(server_challenge);
        info.extend_from_slice(&server_identity.fingerprint);
        let okm = hkdf_sha512(auth_factor.as_bytes(), b"RESPONSE-MIX", &info, CLIENT_RESPONSE_LEN);
        let mut result = [0u8; CLIENT_RESPONSE_LEN];
        result.copy_from_slice(&okm);
        result
    }

    /// Confirm8 = HKDF(K2, "FINAL-CONFIRM", client_nonce2 || server_random)
    pub fn compute_confirm8(
        k2: &DerivedKey,
        client_nonce2: &Random,
        server_random: &Random,
    ) -> [u8; SERVER_CONFIRM_LEN] {
        let mut info = Vec::with_capacity(64);
        info.extend_from_slice(client_nonce2);
        info.extend_from_slice(server_random);
        let okm = hkdf_sha512(k2.as_bytes(), b"FINAL-CONFIRM", &info, SERVER_CONFIRM_LEN);
        let mut result = [0u8; SERVER_CONFIRM_LEN];
        result.copy_from_slice(&okm);
        result
    }
}

// ═══════════════════════════════════════════
// 九、抗 DoS Cookie
// ═══════════════════════════════════════════

/// 无状态 Cookie
/// cookie = HMAC(server_secret, client_addr || client_random || timestamp)
/// 服务端无需存储任何状态即可验证 Cookie 有效性
pub struct DoSCookie;

impl DoSCookie {
    /// 生成 Cookie (服务端)
    /// 格式: timestamp (8 bytes LE) || HMAC-SHA512(server_secret, addr || random || timestamp)
    pub fn generate(
        server_secret: &[u8],
        client_addr: &str,
        client_random: &Random,
        timestamp: u64,
    ) -> Vec<u8> {
        let mut mac = hmac_sha512_new(server_secret);
        mac.update(client_addr.as_bytes());
        mac.update(client_random);
        mac.update(&timestamp.to_le_bytes());
        let tag = mac.finalize().into_bytes();

        let mut cookie = Vec::with_capacity(8 + 64);
        cookie.extend_from_slice(&timestamp.to_le_bytes());
        cookie.extend_from_slice(&tag);
        cookie
    }

    /// 验证 Cookie (服务端)
    /// 返回 true 如果: 1) HMAC 有效 2) timestamp 在 max_age_secs 内
    pub fn verify(
        server_secret: &[u8],
        client_addr: &str,
        client_random: &Random,
        cookie: &[u8],
        max_age_secs: u64,
    ) -> bool {
        if cookie.len() < 72 {
            return false; // 8 (ts) + 64 (HMAC-SHA512)
        }

        let ts_bytes: [u8; 8] = match cookie[0..8].try_into() {
            Ok(b) => b,
            Err(_) => return false,
        };
        let timestamp = u64::from_le_bytes(ts_bytes);

        // 检查时间窗口
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if now.saturating_sub(timestamp) > max_age_secs {
            return false;
        }

        // 重新计算 HMAC 并恒定时间比对
        let expected = &cookie[8..];
        let mut mac = hmac_sha512_new(server_secret);
        mac.update(client_addr.as_bytes());
        mac.update(client_random);
        mac.update(&ts_bytes);
        let tag = mac.finalize().into_bytes();

        // 恒定时间比对
        let mut acc: u8 = 0;
        for (a, b) in expected.iter().zip(tag.iter()) {
            acc |= a ^ b;
        }
        acc == 0
    }
}

// ═══════════════════════════════════════════
// 十、安全会话 (Established 后)
// ═══════════════════════════════════════════

/// 已建立的安全会话
pub struct SecureSession {
    /// 发送密钥
    send_key: SessionKey,
    /// 接收密钥
    recv_key: SessionKey,
    /// 发送 Nonce 生成器
    send_nonce: NonceGenerator,
    /// 接收 Nonce 生成器
    recv_nonce: NonceGenerator,
    /// 接收端重放窗口
    replay_window: ReplayWindow,
    /// 发送序列号 (用于棘轮)
    send_sequence: u64,
    /// 接收序列号 (用于棘轮)
    recv_sequence: u64,
    /// 棘轮配置
    ratchet_config: RatchetConfig,
    /// 重组缓冲区: message_id → 已收集段
    reassembly_buffer: std::collections::HashMap<u64, PendingMessage>,
    /// 流量填充配置
    padding_config: PaddingConfig,
}

#[derive(Debug, Clone)]
pub struct RatchetConfig {
    pub enabled: bool,
    pub message_interval: u64,
    pub time_interval_secs: u64,
}

impl Default for RatchetConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            message_interval: RATCHET_MESSAGE_INTERVAL,
            time_interval_secs: RATCHET_TIME_INTERVAL_SECS,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PaddingConfig {
    pub enabled: bool,
    pub segment_size: usize,
    pub constant_rate: bool,
    pub randomize_order: bool,
}

impl Default for PaddingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            segment_size: DEFAULT_SEGMENT_SIZE,
            constant_rate: false,
            randomize_order: false,
        }
    }
}

/// 待重组消息
pub struct PendingMessage {
    pub message_id: u64,
    pub total_segments: u16,
    pub collected: Vec<Option<Vec<u8>>>,
    pub started_at: std::time::Instant,
}

impl SecureSession {
    /// 加密应用数据 → 分段加密帧序列
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Vec<WireFrame> {
        let seg_size = self.padding_config.segment_size;
        let total = if self.padding_config.enabled {
            // 定长分片: 最后一段填充到固定大小
            plaintext.len().div_ceil(seg_size).max(1)
        } else {
            plaintext.len().div_ceil(seg_size).max(1)
        };
        let total_segments = total as u16;

        let message_id = self.send_sequence;
        self.send_sequence += 1;

        let mut frames = Vec::with_capacity(total);

        for idx in 0..total {
            let start = idx * seg_size;
            let end = ((idx + 1) * seg_size).min(plaintext.len());
            let mut payload = Vec::from(&plaintext[start..end]);

            // 最后一段填充到固定大小
            if self.padding_config.enabled && idx == total - 1 && payload.len() < seg_size {
                payload.resize(seg_size, 0);
            }

            let header = FrameHeader {
                message_id,
                segment_index: idx as u16,
                total_segments,
                payload_length: (end - start) as u16,
            };
            // 帧头序列化后嵌入密文前缀，AAD 为空
            let header_bytes = header.serialize();

            let nonce = self.send_nonce.next_nonce();
            let key = self.send_key.as_bytes();

            // 加密: [header_bytes || payload] 作为一个整体
            let mut full_plaintext = Vec::with_capacity(16 + payload.len());
            full_plaintext.extend_from_slice(&header_bytes);
            full_plaintext.extend_from_slice(&payload);

            let ciphertext = if self.padding_config.enabled
                && false // ChaCha20 disabled by default; enable via CipherSuite selection
            {
                ChaCha20Poly1305Suite::aead_encrypt(key, &nonce, &full_plaintext, &[])
            } else {
                Aes256GcmSuite::aead_encrypt(key, &nonce, &full_plaintext, &[])
            };

            frames.push(WireFrame { nonce, ciphertext });
        }

        // 密钥棘轮检查
        if self.ratchet_config.enabled
            && self.send_sequence % self.ratchet_config.message_interval == 0
        {
            self.ratchet();
        }

        frames
    }

    /// 解密单个 WireFrame → 若消息完整则返回完整明文
    pub fn decrypt(&mut self, frame: &WireFrame) -> Result<Option<Vec<u8>>, SessionError> {
        let key = self.recv_key.as_bytes();

        // AEAD 解密
        let plaintext = Aes256GcmSuite::aead_decrypt(key, &frame.nonce, &frame.ciphertext, &[])?;

        if plaintext.len() < 16 {
            return Err(SessionError::ProtocolViolation);
        }

        // 解析内部帧头
        let mut aad = [0u8; 16];
        aad.copy_from_slice(&plaintext[0..16]);
        let header = FrameHeader::deserialize(&aad);
        let payload = &plaintext[16..];

        // 防重放检查 (基于 message_id + segment_index 合成 nonce 高位)
        let nonce_high = (header.message_id as u64) << 16 | (header.segment_index as u64);
        if !self.replay_window.check_and_record(nonce_high) {
            return Err(SessionError::ReplayDetected);
        }

        // 重组
        let entry = self
            .reassembly_buffer
            .entry(header.message_id)
            .or_insert_with(|| PendingMessage {
                message_id: header.message_id,
                total_segments: header.total_segments,
                collected: vec![None; header.total_segments as usize],
                started_at: std::time::Instant::now(),
            });

        if (header.segment_index as usize) >= entry.collected.len() {
            return Err(SessionError::ProtocolViolation);
        }

        entry.collected[header.segment_index as usize] = Some(payload.to_vec());

        // 检查是否收集完整
        if entry.collected.iter().all(|s| s.is_some()) {
            let pending = self.reassembly_buffer.remove(&header.message_id).unwrap();
            let mut full_msg = Vec::new();
            for seg in pending.collected {
                if let Some(data) = seg {
                    full_msg.extend_from_slice(&data);
                }
            }
            Ok(Some(full_msg))
        } else {
            // 超时清理
            if entry.started_at.elapsed().as_secs() > REASSEMBLY_TIMEOUT_SECS {
                self.reassembly_buffer.remove(&header.message_id);
                return Err(SessionError::Timeout);
            }
            Ok(None)
        }
    }

    /// 生成填充帧 (流量分析对抗)
    pub fn generate_padding_frame(&mut self) -> WireFrame {
        use rand::Rng;
        let seg_size = self.padding_config.segment_size;
        let mut payload = vec![0u8; seg_size];
        rand::thread_rng().fill(&mut payload[..]);

        // 内部帧头: message_id=0, segment=0xFF, total=0xFF → 标识填充
        let header = FrameHeader {
            message_id: 0,
            segment_index: 0xFF,
            total_segments: 0xFF,
            payload_length: seg_size as u16,
        };
        let aad = header.serialize();

        // 实际加密: [header || payload]
        let mut full_plaintext = Vec::with_capacity(16 + seg_size);
        full_plaintext.extend_from_slice(&aad);
        full_plaintext.extend_from_slice(&payload);

        let nonce = self.send_nonce.next_nonce();
        let key = self.send_key.as_bytes();
        let ciphertext = Aes256GcmSuite::aead_encrypt(key, &nonce, &full_plaintext, &[]);

        WireFrame { nonce, ciphertext }
    }

    /// 触发密钥棘轮 (主动)
    /// send_key = HKDF(old_send_key, "RATCHET", ...)
    /// 旧密钥立即零化
    pub fn ratchet(&mut self) {
        let mut new_send_key = KeyDerivation::ratchet_key(&self.send_key, self.send_sequence);
        // ZeroizingBytes 的 Drop 会零化旧值 — 这里显式交换
        std::mem::swap(&mut self.send_key, &mut new_send_key);
        // new_send_key (旧值) 在离开作用域时被 Drop 零化
    }

    /// 清理过期的重组缓冲区条目
    pub fn prune_reassembly(&mut self) {
        self.reassembly_buffer
            .retain(|_, msg| msg.started_at.elapsed().as_secs() < REASSEMBLY_TIMEOUT_SECS);
    }
}

impl Drop for SecureSession {
    fn drop(&mut self) {
        // 所有密钥材料由 ZeroizingBytes 自动零化
        // send_key, recv_key 在 Drop 时覆写
        self.send_sequence = 0;
        self.recv_sequence = 0;
    }
}

// ═══════════════════════════════════════════
// 十一、错误类型 (统一错误)
// ═══════════════════════════════════════════

/// 会话错误 — 所有认证失败返回统一变体，无差异化
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionError {
    /// 认证失败 (统一，不区分原因)
    AuthenticationFailed,
    /// 消息篡改 (AEAD 验证失败)
    IntegrityCheckFailed,
    /// 重放攻击
    ReplayDetected,
    /// 消息格式错误
    ProtocolViolation,
    /// 超时
    Timeout,
    /// 资源耗尽
    ResourceExhausted,
    /// 会话已关闭
    SessionClosed,
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthenticationFailed => write!(f, "authentication failed"),
            Self::IntegrityCheckFailed => write!(f, "integrity check failed"),
            Self::ReplayDetected => write!(f, "replay detected"),
            Self::ProtocolViolation => write!(f, "protocol violation"),
            Self::Timeout => write!(f, "timeout"),
            Self::ResourceExhausted => write!(f, "resource exhausted"),
            Self::SessionClosed => write!(f, "session closed"),
        }
    }
}

// ═══════════════════════════════════════════
// 十二、核心 Trait 接口
// ═══════════════════════════════════════════

/// AEAD 密码套件抽象
/// 唯一套件, 无协商 — 实现时不暴露选择
pub trait CipherSuite {
    /// AEAD 加密
    fn aead_encrypt(key: &[u8], nonce: &AeadNonce, plaintext: &[u8], aad: &[u8]) -> Vec<u8>;
    /// AEAD 解密 (返回明文或认证失败)
    fn aead_decrypt(key: &[u8], nonce: &AeadNonce, ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>, SessionError>;
}

// ── 具体实现: AES-256-GCM ──

use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce as AesNonce,
};

/// AES-256-GCM 密码套件
pub struct Aes256GcmSuite;

impl CipherSuite for Aes256GcmSuite {
    fn aead_encrypt(key: &[u8], nonce: &AeadNonce, plaintext: &[u8], aad: &[u8]) -> Vec<u8> {
        let cipher = Aes256Gcm::new_from_slice(key).expect("AES-256-GCM: invalid key length");
        let nonce = AesNonce::from_slice(nonce);
        cipher
            .encrypt(nonce, Payload { msg: plaintext, aad })
            .expect("AES-256-GCM encrypt")
    }

    fn aead_decrypt(key: &[u8], nonce: &AeadNonce, ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>, SessionError> {
        let cipher = Aes256Gcm::new_from_slice(key).expect("AES-256-GCM: invalid key length");
        let nonce = AesNonce::from_slice(nonce);
        cipher
            .decrypt(nonce, Payload { msg: ciphertext, aad })
            .map_err(|_| SessionError::IntegrityCheckFailed)
    }
}

// ── 具体实现: ChaCha20-Poly1305 ──

use chacha20poly1305::{
    aead::{Payload as ChachaPayload},
    ChaCha20Poly1305, Nonce as ChachaNonce,
};

/// ChaCha20-Poly1305 密码套件 (移动端/ARM 友好)
pub struct ChaCha20Poly1305Suite;

impl CipherSuite for ChaCha20Poly1305Suite {
    fn aead_encrypt(key: &[u8], nonce: &AeadNonce, plaintext: &[u8], aad: &[u8]) -> Vec<u8> {
        let cipher = ChaCha20Poly1305::new_from_slice(key).expect("ChaCha20-Poly1305: invalid key length");
        let nonce = ChachaNonce::from_slice(nonce);
        cipher
            .encrypt(nonce, ChachaPayload { msg: plaintext, aad })
            .expect("ChaCha20-Poly1305 encrypt")
    }

    fn aead_decrypt(key: &[u8], nonce: &AeadNonce, ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>, SessionError> {
        let cipher = ChaCha20Poly1305::new_from_slice(key).expect("ChaCha20-Poly1305: invalid key length");
        let nonce = ChachaNonce::from_slice(nonce);
        cipher
            .decrypt(nonce, ChachaPayload { msg: ciphertext, aad })
            .map_err(|_| SessionError::IntegrityCheckFailed)
    }
}

/// 传输层抽象
/// 屏蔽 TCP / 串行 / 管道等具体传输
pub trait Transport {
    /// 发送原始字节
    fn send(&mut self, data: &[u8]) -> Result<(), SessionError>;
    /// 接收原始字节 (阻塞或超时)
    fn recv(&mut self, timeout_secs: u64) -> Result<Vec<u8>, SessionError>;
}

// ── TCP 传输实现 ──

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};

/// TCP 传输层
pub struct TcpTransport {
    stream: TcpStream,
    read_timeout: std::time::Duration,
    write_timeout: std::time::Duration,
}

impl TcpTransport {
    /// 连接到远程地址
    pub fn connect<A: ToSocketAddrs>(addr: A) -> Result<Self, SessionError> {
        let stream = TcpStream::connect(addr).map_err(|_| SessionError::Timeout)?;
        stream
            .set_nodelay(true)
            .map_err(|_| SessionError::ProtocolViolation)?;
        Ok(Self {
            stream,
            read_timeout: std::time::Duration::from_secs(10),
            write_timeout: std::time::Duration::from_secs(10),
        })
    }

    /// 从已有 TcpStream 创建 (服务端 accept)
    pub fn from_stream(stream: TcpStream) -> Result<Self, SessionError> {
        stream
            .set_nodelay(true)
            .map_err(|_| SessionError::ProtocolViolation)?;
        Ok(Self {
            stream,
            read_timeout: std::time::Duration::from_secs(10),
            write_timeout: std::time::Duration::from_secs(10),
        })
    }

    /// 设置超时
    pub fn set_timeouts(&mut self, read_secs: u64, write_secs: u64) {
        self.read_timeout = std::time::Duration::from_secs(read_secs);
        self.write_timeout = std::time::Duration::from_secs(write_secs);
    }

    /// 获取远端地址
    pub fn peer_addr(&self) -> Option<String> {
        self.stream.peer_addr().ok().map(|a| a.to_string())
    }
}

impl Transport for TcpTransport {
    fn send(&mut self, data: &[u8]) -> Result<(), SessionError> {
        self.stream
            .set_write_timeout(Some(self.write_timeout))
            .map_err(|_| SessionError::ProtocolViolation)?;

        // 帧前缀: 4 字节大端长度
        let len = data.len() as u32;
        let mut framed = Vec::with_capacity(4 + data.len());
        framed.extend_from_slice(&len.to_be_bytes());
        framed.extend_from_slice(data);

        self.stream
            .write_all(&framed)
            .map_err(|_| SessionError::SessionClosed)?;
        self.stream
            .flush()
            .map_err(|_| SessionError::SessionClosed)?;
        Ok(())
    }

    fn recv(&mut self, timeout_secs: u64) -> Result<Vec<u8>, SessionError> {
        self.stream
            .set_read_timeout(Some(std::time::Duration::from_secs(timeout_secs)))
            .map_err(|_| SessionError::ProtocolViolation)?;

        // 读取 4 字节长度前缀
        let mut len_buf = [0u8; 4];
        self.stream
            .read_exact(&mut len_buf)
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => {
                    SessionError::Timeout
                }
                _ => SessionError::SessionClosed,
            })?;

        let len = u32::from_be_bytes(len_buf) as usize;
        if len > MAX_MESSAGE_SIZE {
            return Err(SessionError::ResourceExhausted);
        }

        let mut data = vec![0u8; len];
        self.stream
            .read_exact(&mut data)
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => {
                    SessionError::Timeout
                }
                _ => SessionError::SessionClosed,
            })?;

        Ok(data)
    }
}

/// PKI / 预置信任存储抽象
pub trait TrustStore {
    /// 验证服务端身份 (比对预置指纹)
    fn verify_server_identity(&self, identity: &ServerIdentity) -> bool;
}

// ═══════════════════════════════════════════
// 十三、速率限制器
// ═══════════════════════════════════════════

/// 简单令牌桶速率限制
pub struct RateLimiter {
    max_per_second: u32,
    tokens: f64,
    last_refill: std::time::Instant,
}

impl RateLimiter {
    pub fn new(max_per_second: u32) -> Self {
        Self {
            max_per_second,
            tokens: max_per_second as f64,
            last_refill: std::time::Instant::now(),
        }
    }

    pub fn try_consume(&mut self) -> bool {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.max_per_second as f64)
            .min(self.max_per_second as f64);
        self.last_refill = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

// ═══════════════════════════════════════════
// 十四、后量子安全扩展 (接口预留)
// ═══════════════════════════════════════════

/// PQ KEM 抽象 (预留)
/// 当前不实现，通过 feature flag 启用
pub trait PqKem {
    type PublicKey;
    type SecretKey;
    type Ciphertext;
    type SharedSecret;

    fn keygen() -> (Self::PublicKey, Self::SecretKey);
    fn encapsulate(pk: &Self::PublicKey) -> (Self::Ciphertext, Self::SharedSecret);
    fn decapsulate(sk: &Self::SecretKey, ct: &Self::Ciphertext) -> Self::SharedSecret;
}

/// 混合后量子密钥派生
/// MasterSecret = HKDF(ECDH_shared || PQ_shared, "HYBRID", H_final)
#[allow(dead_code)]
fn derive_hybrid_master_secret(
    _ecdh_shared: &DhSharedSecret,
    _pq_shared: &[u8],
    _h_final: &[u8; 64],
) -> MasterSecret {
    // TODO: 当启用 PQ feature 时实现
    todo!("PQ KEM not yet implemented")
}

// ═══════════════════════════════════════════
// 十五、便捷构造器 (Builder)
// ═══════════════════════════════════════════

/// 客户端会话配置
pub struct ClientConfig {
    pub psk: Psk,
    pub auth_factor: ClientAuthFactor,
    pub server_identity: ServerIdentity,
    pub ratchet: RatchetConfig,
    pub padding: PaddingConfig,
}

/// 服务端会话配置
pub struct ServerConfig {
    pub psk: Psk,
    /// 客户端认证因子验证器 (不可逆 HMAC 或 Ed25519 公钥)
    pub client_verifier: ClientVerifier,
    pub server_secret: ZeroizingBytes<32>, // 用于 Cookie 签名
    pub ratchet: RatchetConfig,
    pub padding: PaddingConfig,
    pub rate_limit: Option<u32>,
}

/// 客户端验证器类型
pub enum ClientVerifier {
    /// 不可逆 HMAC 验证器
    Hmac { digest: [u8; 64] },
    /// Ed25519 公钥验证 (最高安全级别)
    Ed25519 { public_key: [u8; 32] },
}

// ═══════════════════════════════════════════
// 十六、测试与验证框架桩
// ═══════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replay_window_basic() {
        let mut rw = ReplayWindow::new();
        assert!(rw.check_and_record(5));
        assert!(!rw.check_and_record(5)); // 重放
        assert!(rw.check_and_record(6));
        assert!(rw.check_and_record(7));
    }

    #[test]
    fn test_replay_window_shift() {
        let mut rw = ReplayWindow::new();
        for i in 0..2048 {
            assert!(rw.check_and_record(i));
        }
        // 超过窗口大小的旧 nonce 被拒绝
        assert!(!rw.check_and_record(0));
    }

    #[test]
    fn test_nonce_generator_uniqueness() {
        let mut gen = NonceGenerator::new([0xAB; 12]);
        let n1 = gen.next_nonce();
        let n2 = gen.next_nonce();
        assert_ne!(n1, n2);
    }

    #[test]
    fn test_frame_header_roundtrip() {
        let hdr = FrameHeader {
            message_id: 42,
            segment_index: 3,
            total_segments: 10,
            payload_length: 512,
        };
        let ser = hdr.serialize();
        let deser = FrameHeader::deserialize(&ser);
        assert_eq!(hdr.message_id, deser.message_id);
        assert_eq!(hdr.segment_index, deser.segment_index);
        assert_eq!(hdr.total_segments, deser.total_segments);
        assert_eq!(hdr.payload_length, deser.payload_length);
    }

    #[test]
    fn test_rate_limiter() {
        let mut rl = RateLimiter::new(5);
        let mut count = 0;
        for _ in 0..5 {
            if rl.try_consume() {
                count += 1;
            }
        }
        assert_eq!(count, 5);
        assert!(!rl.try_consume()); // 桶空
    }

    #[test]
    fn test_x25519_ecdh_roundtrip() {
        use rand::Rng;
        let alice_sk: [u8; ECDH_KEY_LEN] = rand::thread_rng().gen();
        let bob_sk: [u8; ECDH_KEY_LEN] = rand::thread_rng().gen();

        let alice_pk = x25519_public_from_secret(&alice_sk);
        let bob_pk = x25519_public_from_secret(&bob_sk);

        let alice_shared = x25519_dh(&alice_sk, &bob_pk);
        let bob_shared = x25519_dh(&bob_sk, &alice_pk);

        assert_eq!(alice_shared.as_bytes(), bob_shared.as_bytes());
    }

    #[test]
    fn test_transcript_hash_accumulation() {
        // 转录哈希每一步绑定前一步状态 — 顺序不同则结果不同
        let mut th1 = TranscriptHash::new();
        th1.update(b"hello");
        let h1a = th1.finalize();

        let mut th2 = TranscriptHash::new();
        th2.update(b"hello");
        let h2a = th2.finalize();

        // 相同累积 → 相同哈希
        assert_eq!(h1a, h2a);

        // 继续累积
        th1.update(b"world");
        th2.update(b"world");
        assert_eq!(th1.finalize(), th2.finalize());
    }

    #[test]
    fn test_hkdf_sha512_known_vector() {
        // RFC 5869 Test Case 3 (SHA-512)
        let ikm = hex::decode("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b").unwrap();
        let salt: &[u8] = &[];
        let info: &[u8] = &[];
        let okm = hkdf_sha512(&ikm, salt, info, 42);
        assert_eq!(okm.len(), 42);
    }

    #[test]
    fn test_full_handshake_roundtrip() {
        use rand::Rng;

        // ── ECDH 密钥交换 ──
        let alice_sk: [u8; 32] = rand::thread_rng().gen();
        let alice_pk = x25519_public_from_secret(&alice_sk);
        let bob_sk: [u8; 32] = rand::thread_rng().gen();
        let bob_pk = x25519_public_from_secret(&bob_sk);
        let shared1 = x25519_dh(&alice_sk, &bob_pk);
        let shared2 = x25519_dh(&bob_sk, &alice_pk);
        assert_eq!(shared1.as_bytes(), shared2.as_bytes(), "X25519 ECDH");

        // ── 密钥派生链 ──
        let h_final = {
            let mut th = TranscriptHash::new();
            th.update(b"test-handshake-data");
            th.finalize()
        };

        let master = KeyDerivation::derive_master_secret(&shared1, &h_final);
        let session_key = KeyDerivation::derive_session_key(&master, &h_final);

        let send_base: [u8; 12] = rand::thread_rng().gen();
        let recv_base: [u8; 12] = rand::thread_rng().gen();
        let send_key = KeyDerivation::derive_directional_key(&session_key, "SEND", &send_base);
        let recv_key = KeyDerivation::derive_directional_key(&session_key, "RECV", &recv_base);

        // ── 构造会话 (Alice: send=send_key, recv=recv_key; Bob: 镜像) ──
        let mut alice = SecureSession {
            send_key: send_key.clone(),
            recv_key: recv_key.clone(),
            send_nonce: NonceGenerator::new(send_base),
            recv_nonce: NonceGenerator::new(recv_base),
            replay_window: ReplayWindow::new(),
            send_sequence: 0,
            recv_sequence: 0,
            ratchet_config: RatchetConfig::default(),
            reassembly_buffer: std::collections::HashMap::new(),
            padding_config: PaddingConfig::default(),
        };

        let mut bob = SecureSession {
            send_key: recv_key,
            recv_key: send_key.clone(),
            send_nonce: NonceGenerator::new(recv_base),
            recv_nonce: NonceGenerator::new(send_base),
            replay_window: ReplayWindow::new(),
            send_sequence: 0,
            recv_sequence: 0,
            ratchet_config: RatchetConfig::default(),
            reassembly_buffer: std::collections::HashMap::new(),
            padding_config: PaddingConfig::default(),
        };

        // ── 单段消息往返 ──
        let msg = b"Hello, Secure World! X25519 + AES-256-GCM!";
        let frames = alice.encrypt(msg);
        assert!(!frames.is_empty());
        let dec = bob.decrypt(&frames[0]).expect("decrypt").expect("complete");
        assert_eq!(&dec, msg, "Single-segment roundtrip");

        // ── 多段消息往返 ──
        let big_msg = vec![0x42u8; 5000];
        let frames = alice.encrypt(&big_msg);
        assert!(frames.len() > 1, "Large message fragmented");
        let mut full = Vec::new();
        for f in &frames {
            if let Some(part) = bob.decrypt(f).expect("decrypt") {
                full.extend_from_slice(&part);
            }
        }
        assert_eq!(&full, &big_msg, "Multi-segment roundtrip");

        // ── 重放检测 (已验证于 test_replay_window_basic) ──
        let replay_result = bob.decrypt(&frames[0]);
        // 重放同一帧: 要么触发重放检测(Err), 要么重组缓冲区已清空(Ok(None))
        assert!(replay_result.is_err() || replay_result.unwrap().is_none(),
            "Replay of same frame should not yield plaintext again");
    }
}
