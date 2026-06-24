//! CRC 和校验和计算模块
//!
//! 基于 lrzsz-0.12.20 `crctab.c` 实现，为 X/Y/ZModem 协议提供统一校验基础设施。
//!
//! - `crc16_ccitt`: CRC-16/CCITT (XMODEM-CRC, YMODEM)
//! - `crc32_zmodem`: CRC-32 (ZMODEM ZBIN32 帧)
//! - `checksum`: 算术和 mod 256 (标准 XMODEM)

/// CRC-16/CCITT 查找表（多项式 0x1021，对齐 lrzsz crctab）
const CRC16_TABLE: [u16; 256] = [
    0x0000, 0x1021, 0x2042, 0x3063, 0x4084, 0x50a5, 0x60c6, 0x70e7,
    0x8108, 0x9129, 0xa14a, 0xb16b, 0xc18c, 0xd1ad, 0xe1ce, 0xf1ef,
    0x1231, 0x0210, 0x3273, 0x2252, 0x52b5, 0x4294, 0x72f7, 0x62d6,
    0x9339, 0x8318, 0xb37b, 0xa35a, 0xd3bd, 0xc39c, 0xf3ff, 0xe3de,
    0x2462, 0x3443, 0x0420, 0x1401, 0x64e6, 0x74c7, 0x44a4, 0x5485,
    0xa56a, 0xb54b, 0x8528, 0x9509, 0xe5ee, 0xf5cf, 0xc5ac, 0xd58d,
    0x3653, 0x2672, 0x1611, 0x0630, 0x76d7, 0x66f6, 0x5695, 0x46b4,
    0xb75b, 0xa77a, 0x9719, 0x8738, 0xf7df, 0xe7fe, 0xd79d, 0xc7bc,
    0x48c4, 0x58e5, 0x6886, 0x78a7, 0x0840, 0x1861, 0x2802, 0x3823,
    0xc9cc, 0xd9ed, 0xe98e, 0xf9af, 0x8948, 0x9969, 0xa90a, 0xb92b,
    0x5af5, 0x4ad4, 0x7ab7, 0x6a96, 0x1a71, 0x0a50, 0x3a33, 0x2a12,
    0xdbfd, 0xcbdc, 0xfbbf, 0xeb9e, 0x9b79, 0x8b58, 0xbb3b, 0xab1a,
    0x6ca6, 0x7c87, 0x4ce4, 0x5cc5, 0x2c22, 0x3c03, 0x0c60, 0x1c41,
    0xedae, 0xfd8f, 0xcdec, 0xddcd, 0xad2a, 0xbd0b, 0x8d68, 0x9d49,
    0x7e97, 0x6eb6, 0x5ed5, 0x4ef4, 0x3e13, 0x2e32, 0x1e51, 0x0e70,
    0xff9f, 0xefbe, 0xdfdd, 0xcffc, 0xbf1b, 0xaf3a, 0x9f59, 0x8f78,
    0x9188, 0x81a9, 0xb1ca, 0xa1eb, 0xd10c, 0xc12d, 0xf14e, 0xe16f,
    0x1080, 0x00a1, 0x30c2, 0x20e3, 0x5004, 0x4025, 0x7046, 0x6067,
    0x83b9, 0x9398, 0xa3fb, 0xb3da, 0xc33d, 0xd31c, 0xe37f, 0xf35e,
    0x02b1, 0x1290, 0x22f3, 0x32d2, 0x4235, 0x5214, 0x6277, 0x7256,
    0xb5ea, 0xa5cb, 0x95a8, 0x8589, 0xf56e, 0xe54f, 0xd52c, 0xc50d,
    0x34e2, 0x24c3, 0x14a0, 0x0481, 0x7466, 0x6447, 0x5424, 0x4405,
    0xa7db, 0xb7fa, 0x8799, 0x97b8, 0xe75f, 0xf77e, 0xc71d, 0xd73c,
    0x26d3, 0x36f2, 0x0691, 0x16b0, 0x6657, 0x7676, 0x4615, 0x5634,
    0xd94c, 0xc96d, 0xf90e, 0xe92f, 0x99c8, 0x89e9, 0xb98a, 0xa9ab,
    0x5844, 0x4865, 0x7806, 0x6827, 0x18c0, 0x08e1, 0x3882, 0x28a3,
    0xcb7d, 0xdb5c, 0xeb3f, 0xfb1e, 0x8bf9, 0x9bd8, 0xabbb, 0xbb9a,
    0x4a75, 0x5a54, 0x6a37, 0x7a16, 0x0af1, 0x1ad0, 0x2ab3, 0x3a92,
    0xfd2e, 0xed0f, 0xdd6c, 0xcd4d, 0xbdaa, 0xad8b, 0x9de8, 0x8dc9,
    0x7c26, 0x6c07, 0x5c64, 0x4c45, 0x3ca2, 0x2c83, 0x1ce0, 0x0cc1,
    0xef1f, 0xff3e, 0xcf5d, 0xdf7c, 0xaf9b, 0xbfba, 0x8fd9, 0x9ff8,
    0x6e17, 0x7e36, 0x4e55, 0x5e74, 0x2e93, 0x3eb2, 0x0ed1, 0x1ef0,
];

/// CRC-32 查找表（多项式 0xEDB88320，对齐 lrzsz cr3tab）
const CRC32_TABLE: [u32; 256] = [
    0x00000000, 0x77073096, 0xee0e612c, 0x990951ba, 0x076dc419, 0x706af48f, 0xe963a535, 0x9e6495a3,
    0x0edb8832, 0x79dcb8a4, 0xe0d5e91e, 0x97d2d988, 0x09b64c2b, 0x7eb17cbd, 0xe7b82d07, 0x90bf1d91,
    0x1db71064, 0x6ab020f2, 0xf3b97148, 0x84be41de, 0x1adad47d, 0x6ddde4eb, 0xf4d4b551, 0x83d385c7,
    0x136c9856, 0x646ba8c0, 0xfd62f97a, 0x8a65c9ec, 0x14015c4f, 0x63066cd9, 0xfa0f3d63, 0x8d080df5,
    0x3b6e20c8, 0x4c69105e, 0xd56041e4, 0xa2677172, 0x3c03e4d1, 0x4b04d447, 0xd20d85fd, 0xa50ab56b,
    0x35b5a8fa, 0x42b2986c, 0xdbbbc9d6, 0xacbcf940, 0x32d86ce3, 0x45df5c75, 0xdcd60dcf, 0xabd13d59,
    0x26d930ac, 0x51de003a, 0xc8d75180, 0xbfd06116, 0x21b4f4b5, 0x56b3c423, 0xcfba9599, 0xb8bda50f,
    0x2802b89e, 0x5f058808, 0xc60cd9b2, 0xb10be924, 0x2f6f7c87, 0x58684c11, 0xc1611dab, 0xb6662d3d,
    0x76dc4190, 0x01db7106, 0x98d220bc, 0xefd5102a, 0x71b18589, 0x06b6b51f, 0x9fbfe4a5, 0xe8b8d433,
    0x7807c9a2, 0x0f00f934, 0x9609a88e, 0xe10e9818, 0x7f6a0dbb, 0x086d3d2d, 0x91646c97, 0xe6635c01,
    0x6b6b51f4, 0x1c6c6162, 0x856530d8, 0xf262004e, 0x6c0695ed, 0x1b01a57b, 0x8208f4c1, 0xf50fc457,
    0x65b0d9c6, 0x12b7e950, 0x8bbeb8ea, 0xfcb9887c, 0x62dd1ddf, 0x15da2d49, 0x8cd37cf3, 0xfbd44c65,
    0x4db26158, 0x3ab551ce, 0xa3bc0074, 0xd4bb30e2, 0x4adfa541, 0x3dd895d7, 0xa4d1c46d, 0xd3d6f4fb,
    0x4369e96a, 0x346ed9fc, 0xad678846, 0xda60b8d0, 0x44042d73, 0x33031de5, 0xaa0a4c5f, 0xdd0d7cc9,
    0x5005713c, 0x270241aa, 0xbe0b1010, 0xc90c2086, 0x5768b525, 0x206f85b3, 0xb966d409, 0xce61e49f,
    0x5edef90e, 0x29d9c998, 0xb0d09822, 0xc7d7a8b4, 0x59b33d17, 0x2eb40d81, 0xb7bd5c3b, 0xc0ba6cad,
    0xedb88320, 0x9abfb3b6, 0x03b6e20c, 0x74b1d29a, 0xead54739, 0x9dd277af, 0x04db2615, 0x73dc1683,
    0xe3630b12, 0x94643b84, 0x0d6d6a3e, 0x7a6a5aa8, 0xe40ecf0b, 0x9309ff9d, 0x0a00ae27, 0x7d079eb1,
    0xf00f9344, 0x8708a3d2, 0x1e01f268, 0x6906c2fe, 0xf762575d, 0x806567cb, 0x196c3671, 0x6e6b06e7,
    0xfed41b76, 0x89d32be0, 0x10da7a5a, 0x67dd4acc, 0xf9b9df6f, 0x8ebeeff9, 0x17b7be43, 0x60b08ed5,
    0xd6d6a3e8, 0xa1d1937e, 0x38d8c2c4, 0x4fdff252, 0xd1bb67f1, 0xa6bc5767, 0x3fb506dd, 0x48b2364b,
    0xd80d2bda, 0xaf0a1b4c, 0x36034af6, 0x41047a60, 0xdf60efc3, 0xa867df55, 0x316e8eef, 0x4669be79,
    0xcb61b38c, 0xbc66831a, 0x256fd2a0, 0x5268e236, 0xcc0c7795, 0xbb0b4703, 0x220216b9, 0x5505262f,
    0xc5ba3bbe, 0xb2bd0b28, 0x2bb45a92, 0x5cb36a04, 0xc2d7ffa7, 0xb5d0cf31, 0x2cd99e8b, 0x5bdeae1d,
    0x9b64c2b0, 0xec63f226, 0x756aa39c, 0x026d930a, 0x9c0906a9, 0xeb0e363f, 0x72076785, 0x05005713,
    0x95bf4a82, 0xe2b87a14, 0x7bb12bae, 0x0cb61b38, 0x92d28e9b, 0xe5d5be0d, 0x7cdcefb7, 0x0bdbdf21,
    0x86d3d2d4, 0xf1d4e242, 0x68ddb3f8, 0x1fda836e, 0x81be16cd, 0xf6b9265b, 0x6fb077e1, 0x18b74777,
    0x88085ae6, 0xff0f6a70, 0x66063bca, 0x11010b5c, 0x8f659eff, 0xf862ae69, 0x616bffd3, 0x166ccf45,
    0xa00ae278, 0xd70dd2ee, 0x4e048354, 0x3903b3c2, 0xa7672661, 0xd06016f7, 0x4969474d, 0x3e6e77db,
    0xaed16a4a, 0xd9d65adc, 0x40df0b66, 0x37d83bf0, 0xa9bcae53, 0xdebb9ec5, 0x47b2cf7f, 0x30b5ffe9,
    0xbdbdf21c, 0xcabac28a, 0x53b39330, 0x24b4a3a6, 0xbad03605, 0xcdd70693, 0x54de5729, 0x23d967bf,
    0xb3667a2e, 0xc4614ab8, 0x5d681b02, 0x2a6f2b94, 0xb40bbe37, 0xc30c8ea1, 0x5a05df1b, 0x2d02ef8d,
];

/// 计算 CRC-16/CCITT（多项式 0x1021，初始值 0x0000）
///
/// 用于 XMODEM-CRC 和 YMODEM 协议、ZMODEM ZBIN/ZHEX 帧头。
/// 使用标准反射 CRC-16/XMODEM 算法（refin=true, refout=true）。
pub fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &byte in data {
        // 标准反射 CRC-16/XMODEM 查表法
        crc = (crc << 8) ^ CRC16_TABLE[((crc >> 8) as u8 ^ byte) as usize];
    }
    crc
}

/// lrzsz `updcrc` 宏的 Rust 实现
///
/// 算法: `crctab[((crc >> 8) & 255)] ^ (crc << 8) ^ byte`
/// 使用与 lrzsz crctab.c 完全相同的查表逻辑。与 `crc16_ccitt` 的不同之处
/// 在于字节的 XOR 位置：`updcrc` 将 byte XOR 到移位后的结果，
/// 而 `crc16_ccitt` 将 byte XOR 到表索引。
/// 对于完整数据序列，两者产生相同的最终 CRC。
#[inline]
fn updcrc(byte: u8, crc: u16) -> u16 {
    CRC16_TABLE[((crc >> 8) & 0xFF) as usize] ^ (crc << 8) ^ (byte as u16)
}

/// 使用 lrzsz updcrc 算法计算 CRC-16
///
/// 产生与 `crc16_ccitt` 相同的最终结果，但内部状态不同。
/// 用于需要与 lrzsz 部分状态兼容的操作（零填充、前馈验证）。
#[inline]
fn crc16_updcrc(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &byte in data {
        crc = updcrc(byte, crc);
    }
    crc
}

/// 计算 CRC-16/CCITT 零填充传输值（对齐 lrzsz `wcputsec`）
///
/// 在数据 CRC 基础上额外喂入两个零字节：`updcrc(0, updcrc(0, crc16_updcrc(data)))`。
/// YMODEM/XMODEM-CRC 发送方必须使用此函数计算要传输的 CRC 字节。
pub fn crc16_ccitt_zero_pad(data: &[u8]) -> u16 {
    let mut crc = crc16_updcrc(data);
    // 喂入两个零字节（对齐 lrzsz wcputsec: updcrc(0, updcrc(0, oldcrc))）
    crc = updcrc(0, crc);
    crc = updcrc(0, crc);
    crc
}

/// CRC-16 前馈验证（对齐 lrzsz `wcgetsec`）
///
/// 将接收到的两个 CRC 字节喂入 CRC 引擎，验证结果为零。
///
/// # 兼容性说明
///
/// 本函数使用 lrzsz `updcrc` 算法（`table[(crc>>8)] ^ (crc<<8) ^ byte`）。
/// 设备端通常使用标准反射 CRC-16/XMODEM 算法
/// （`(crc<<8) ^ table[(crc>>8) ^ byte]`）。对完整数据序列两者产生相同的
/// 最终 CRC 值（已验证 `crc16_ccitt("123456789") == 0x31C3`）。
///
/// 前馈验证的数学性质（CRC residual property）对两种实现均成立：
/// 发送方计算 `CRC(data)` 并传输 CRC 字节，接收方将 CRC 字节喂回引擎
/// 后结果必为零。因此无论发送方使用 lrzsz 还是标准 CRC-16/XMODEM 算法，
/// 只要 CRC 字节以大端序存储，本函数均能正确验证。
///
/// 返回 `true` 表示 CRC 验证通过。
pub fn crc16_ccitt_feedthrough_verify(data: &[u8], crc_hi: u8, crc_lo: u8) -> bool {
    let mut crc = crc16_updcrc(data);
    // 喂入 CRC 高字节（对齐 lrzsz wcgetsec）
    crc = updcrc(crc_hi, crc);
    // 喂入 CRC 低字节
    crc = updcrc(crc_lo, crc);
    // 结果必须为零
    crc == 0
}

/// CRC-16 位计算（用于验证查表结果，仅测试用）
#[cfg(test)]
fn crc16_ccitt_bitwise(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc = crc << 1;
            }
        }
    }
    crc
}

/// 计算 CRC-32（多项式 0xEDB88320，初始值 0xFFFFFFFF，最终异或 0xFFFFFFFF）
///
/// 用于 ZMODEM ZBIN32 帧格式。
/// 初始值 0xFFFFFFFF，逐字节更新，最终异或 0xFFFFFFFF（标准 CRC-32）。
pub fn crc32_zmodem(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        let idx = ((crc ^ (byte as u32)) & 0xFF) as usize;
        crc = CRC32_TABLE[idx] ^ (crc >> 8);
    }
    !crc // 最终异或 0xFFFFFFFF
}

/// 计算 XMODEM 校验和（数据字节算术和 mod 256）
///
/// 用于标准 XMODEM（无 CRC 模式）。
pub fn checksum(data: &[u8]) -> u8 {
    data.iter().fold(0u8, |acc, &b| acc.wrapping_add(b))
}

/// 验证 CRC-16/CCITT 匹配
#[inline]
pub fn crc16_ccitt_verify(data: &[u8], expected: u16) -> bool {
    crc16_ccitt(data) == expected
}

/// 验证 CRC-32 匹配
#[inline]
pub fn crc32_verify(data: &[u8], expected: u32) -> bool {
    crc32_zmodem(data) == expected
}

/// 验证校验和匹配
#[inline]
pub fn checksum_verify(data: &[u8], expected: u8) -> bool {
    checksum(data).wrapping_add(expected) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc16_ccitt_known_vector() {
        // 验证查表法和位计算法一致
        let data = b"123456789";
        let table_result = crc16_ccitt(data);
        let bitwise_result = crc16_ccitt_bitwise(data);
        assert_eq!(table_result, bitwise_result,
            "查表法 ({:#06X}) 与位计算法 ({:#06X}) 不一致", table_result, bitwise_result);
        // CRC-16/XMODEM 标准测试向量: "123456789" → 0x31C3
        assert_eq!(table_result, 0x31C3);
    }

    #[test]
    fn test_crc32_known_vector() {
        // 已知测试向量: "123456789" → 0xCBF43926
        assert_eq!(crc32_zmodem(b"123456789"), 0xCBF43926);
    }

    #[test]
    fn test_checksum() {
        let data = [1u8, 2, 3, 4, 5];
        let sum = checksum(&data);
        assert_eq!(sum, 15); // 1+2+3+4+5 = 15
    }

    #[test]
    fn test_crc16_verify() {
        let data = b"123456789";
        let crc = crc16_ccitt(data);
        assert!(crc16_ccitt_verify(data, crc));
        assert!(!crc16_ccitt_verify(data, 0x0000));
    }

    #[test]
    fn test_crc32_empty() {
        // 空数据: init=0xFFFFFFFF, final XOR → 0
        let crc = crc32_zmodem(b"");
        assert_eq!(crc, 0x0000_0000);
    }

    #[test]
    fn test_crc16_empty() {
        assert_eq!(crc16_ccitt(b""), 0x0000);
    }

    #[test]
    fn test_crc16_zero_pad_self_consistent() {
        // zero_pad 应等价于手动喂入两个零字节
        let data = b"hello ymodem test";
        // 手动计算
        let mut crc = 0u16;
        for &b in data {
            crc = CRC16_TABLE[((crc >> 8) & 0xFF) as usize] ^ (crc << 8) ^ (b as u16);
        }
        crc = CRC16_TABLE[((crc >> 8) & 0xFF) as usize] ^ (crc << 8); // feed 0
        crc = CRC16_TABLE[((crc >> 8) & 0xFF) as usize] ^ (crc << 8); // feed 0
        assert_eq!(crc16_ccitt_zero_pad(data), crc,
            "zero_pad 应与手动 updcrc(0, updcrc(0, crc)) 一致");
    }

    #[test]
    fn test_crc16_feedthrough_verify_round_trip() {
        // 发送方: zero_pad → 接收方: feedthrough_verify → 应通过
        let data = b"hello ymodem test data block";
        let crc = crc16_ccitt_zero_pad(data);
        let crc_hi = (crc >> 8) as u8;
        let crc_lo = (crc & 0xFF) as u8;
        assert!(crc16_ccitt_feedthrough_verify(data, crc_hi, crc_lo),
            "前馈验证应通过（发送方使用 zero_pad）");
    }

    #[test]
    fn test_crc16_feedthrough_verify_corrupt_data() {
        // 数据损坏 → 验证失败
        let data = b"hello ymodem test data block";
        let crc = crc16_ccitt_zero_pad(data);
        let crc_hi = (crc >> 8) as u8;
        let crc_lo = (crc & 0xFF) as u8;
        // 损坏一个数据字节
        let mut corrupt = data.to_vec();
        corrupt[5] ^= 0x01;
        assert!(!crc16_ccitt_feedthrough_verify(&corrupt, crc_hi, crc_lo),
            "数据损坏应导致验证失败");
    }

    #[test]
    fn test_crc16_feedthrough_verify_corrupt_crc() {
        // CRC 字节损坏 → 验证失败
        let data = b"hello ymodem test data block";
        let crc = crc16_ccitt_zero_pad(data);
        let crc_hi = (crc >> 8) as u8;
        let crc_lo = (crc & 0xFF) as u8;
        // 翻转 CRC 低字节的一位
        assert!(!crc16_ccitt_feedthrough_verify(data, crc_hi, crc_lo ^ 0x01),
            "CRC 损坏应导致验证失败");
    }
}
