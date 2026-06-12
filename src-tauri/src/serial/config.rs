//! 串口配置类型
//!
//! 定义串口连接的所有可配置参数。

/// 串口配置结构体
#[derive(Debug, Clone)]
pub struct SerialConfig {
    /// 端口名称（如 "COM1"、"/dev/ttyUSB0"）
    pub port_name: String,
    /// 波特率（110-921600）
    pub baud_rate: u32,
    /// 数据位（5-8）
    pub data_bits: u8,
    /// 校验位
    pub parity: serialport::Parity,
    /// 停止位
    pub stop_bits: serialport::StopBits,
    /// 流控
    pub flow_control: serialport::FlowControl,
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            port_name: String::new(),
            baud_rate: 115200,
            data_bits: 8,
            parity: serialport::Parity::None,
            stop_bits: serialport::StopBits::One,
            flow_control: serialport::FlowControl::None,
        }
    }
}

impl SerialConfig {
    /// 创建新的串口配置
    pub fn new(
        port_name: String,
        baud_rate: u32,
        data_bits: u8,
        parity: serialport::Parity,
        stop_bits: serialport::StopBits,
        flow_control: serialport::FlowControl,
    ) -> Self {
        Self {
            port_name,
            baud_rate,
            data_bits,
            parity,
            stop_bits,
            flow_control,
        }
    }

    /// 返回匹配 StopBits 值的字符串标签
    pub fn data_bits_value(&self) -> serialport::DataBits {
        match self.data_bits {
            5 => serialport::DataBits::Five,
            6 => serialport::DataBits::Six,
            7 => serialport::DataBits::Seven,
            _ => serialport::DataBits::Eight,
        }
    }
}
