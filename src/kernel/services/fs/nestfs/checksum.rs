use crate::kernel::services::fs::nestfs::bp::NestCksumType;

pub const HV_CKSUM_FLETCHER2: usize = 1;
pub const HV_CKSUM_FLETCHER4: usize = 2;
pub const HV_CKSUM_SHA256: usize = 3;

// I-04: 引入 `Checksum` trait, 让 spa/dedup 等调用方依赖抽象而非具体类型.
// 这样单元测试可注入 mock 实现, 验证 DMU 在不真实存储上的逻辑.

// SAFETY: 该 trait 在 no_std 内核环境下使用, 方法均无内存分配 / 阻塞,
// 可在中断上下文调用. 实现方必须保证 `compute` 与 `verify` 对同一输入
// 返回稳定结果 (无内部可变状态).
pub trait Checksum: Send + Sync {
    /// 给定算法类型与数据, 计算校验和
    fn compute(&self, kind: NestCksumType, data: &[u8]) -> [u64; 4];
    /// 验证 `expected` 与 `data` 在同一算法下结果是否一致
    fn verify(&self, kind: NestCksumType, data: &[u8], expected: &[u64; 4]) -> bool;
}

#[derive(Debug, Clone, Copy)]
pub struct NestChecksum {
    pub kind: NestCksumType,
    pub value: [u64; 4],
}

impl NestChecksum {
    pub fn new(kind: NestCksumType) -> Self {
        Self {
            kind,
            value: [0; 4],
        }
    }

    #[expect(
        clippy::match_same_arms,
        reason = "match_same_arms: match arm 重复是为可读性/调试断点; 当前优先 expect"
    )]
    pub fn compute(kind: NestCksumType, data: &[u8]) -> Self {
        let mut ck = Self::new(kind);
        match kind {
            NestCksumType::Off => {}
            NestCksumType::Fletcher2 => ck.fletcher2(data),
            NestCksumType::Fletcher4 => ck.fletcher4(data),
            NestCksumType::SHA256 => ck.sha256(data),
            NestCksumType::EdonR => ck.fletcher4(data),
        }
        ck
    }

    pub fn verify(&self, data: &[u8]) -> bool {
        let computed = Self::compute(self.kind, data);
        self.value == computed.value
    }

    fn fletcher2(&mut self, data: &[u8]) {
        let words = data.chunks_exact(8);
        let mut a: u64 = 0;
        let mut b: u64 = 0;
        for chunk in words {
            let w = u64::from_le_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
            ]);
            a = a.wrapping_add(w);
            b = b.wrapping_add(a);
        }
        let rem = data.len() % 8;
        if rem > 0 {
            let start = data.len() - rem;
            let mut last = [0u8; 8];
            last[..rem].copy_from_slice(&data[start..]);
            let w = u64::from_le_bytes(last);
            a = a.wrapping_add(w);
            b = b.wrapping_add(a);
        }
        self.value[0] = a;
        self.value[1] = b;
        self.value[2] = 0;
        self.value[3] = 0;
    }

    #[expect(
        clippy::many_single_char_names,
        reason = "DECISION-043 pedantic 兜底: 当前批量 expect 兑底; 后续可逐处手工重构 (改 .cast() / let-else / 命名等)"
    )]
    fn fletcher4(&mut self, data: &[u8]) {
        let words = data.chunks_exact(8);
        let mut a: u64 = 0;
        let mut b: u64 = 0;
        let mut c: u64 = 0;
        let mut d: u64 = 0;
        for chunk in words {
            let w = u64::from_le_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
            ]);
            a = a.wrapping_add(w);
            b = b.wrapping_add(a);
            c = c.wrapping_add(b);
            d = d.wrapping_add(c);
        }
        let rem = data.len() % 8;
        if rem > 0 {
            let start = data.len() - rem;
            let mut last = [0u8; 8];
            last[..rem].copy_from_slice(&data[start..]);
            let w = u64::from_le_bytes(last);
            a = a.wrapping_add(w);
            b = b.wrapping_add(a);
            c = c.wrapping_add(b);
            d = d.wrapping_add(c);
        }
        self.value[0] = a;
        self.value[1] = b;
        self.value[2] = c;
        self.value[3] = d;
    }

    fn sha256(&mut self, data: &[u8]) {
        let hash = crate::kernel::framework::credo::sha256::sha256(data);
        self.value[0] = u64::from_be_bytes(hash[0..8].try_into().unwrap_or_else(|_| [0u8; 8]));
        self.value[1] = u64::from_be_bytes(hash[8..16].try_into().unwrap_or_else(|_| [0u8; 8]));
        self.value[2] = u64::from_be_bytes(hash[16..24].try_into().unwrap_or_else(|_| [0u8; 8]));
        self.value[3] = u64::from_be_bytes(hash[24..32].try_into().unwrap_or_else(|_| [0u8; 8]));
    }
}

// I-04: 为 NestChecksum 实现 Checksum trait, 使其成为 trait object / 泛型可注入.
impl Checksum for NestChecksum {
    fn compute(&self, kind: NestCksumType, data: &[u8]) -> [u64; 4] {
        Self::compute(kind, data).value
    }

    fn verify(&self, kind: NestCksumType, data: &[u8], expected: &[u64; 4]) -> bool {
        let computed = Self::compute(kind, data);
        &computed.value == expected
    }
}
