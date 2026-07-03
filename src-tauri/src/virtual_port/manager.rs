//! VirtualPortManager — com0com 虚拟串口端口对管理

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PortPair {
    pub port_a: String,
    pub port_b: String,
    pub bus_name: String,
}

#[derive(Debug, Clone)]
pub struct VirtualPortConfig {
    pub enabled: bool,
    pub count: u32,
}

pub struct VirtualPortManager {
    driver_installed: bool,
    active_pairs: HashSet<PortPair>,
    resource_dir: PathBuf,
    next_bus: u32,
}

impl VirtualPortManager {
    pub fn new(resource_dir: PathBuf) -> Self {
        Self {
            driver_installed: false,
            active_pairs: HashSet::new(),
            resource_dir,
            next_bus: 0,
        }
    }

    fn setupc_path(&self) -> PathBuf {
        self.resource_dir.join("com0com").join("setupc.exe")
    }

    pub fn detect_driver(&self) -> bool {
        let setupc = self.setupc_path();
        if !setupc.exists() {
            log::warn!("com0com setupc.exe 未找到: {:?}", setupc);
            return false;
        }
        Command::new(&setupc)
            .arg("status")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn install_driver(&mut self) -> Result<(), String> {
        if self.detect_driver() {
            self.driver_installed = true;
            return Ok(());
        }

        let setupc = self.setupc_path();
        if !setupc.exists() {
            return Err(format!(
                "com0com setupc.exe 未找到: {:?}\n请确保 com0com 文件已正确打包",
                setupc
            ));
        }

        log::info!("正在安装 com0com 驱动...");
        let output = Command::new(&setupc)
            .args(["install", "0", "com0com.sys"])
            .output()
            .map_err(|e| format!("无法启动 setupc.exe: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("com0com 驱动安装失败: {}", stderr));
        }

        self.driver_installed = true;
        log::info!("com0com 驱动安装成功");
        Ok(())
    }

    pub fn create_pairs(&mut self, config: &VirtualPortConfig) -> Result<Vec<PortPair>, String> {
        if !self.driver_installed && !self.detect_driver() {
            return Err("com0com 驱动未安装".into());
        }

        let count = config.count.clamp(1, 4);
        let mut pairs = Vec::new();
        let setupc = self.setupc_path();

        for i in 0..count {
            let port_a_num = 10 + (i * 2) as u32;
            let port_b_num = port_a_num + 1;
            let port_a = format!("COM{}", port_a_num);
            let port_b = format!("COM{}", port_b_num);

            let output = Command::new(&setupc)
                .args([
                    "install",
                    &format!("PortName={}", port_a),
                    &format!("PortName={}", port_b),
                    "--emulate-baud-rate",
                    "--emulate-line-control",
                ])
                .output()
                .map_err(|e| format!("创建虚拟端口对失败: {}", e))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("创建 {}↔{} 失败: {}", port_a, port_b, stderr));
            }

            let bus_name = format!("com0com{}", self.next_bus);
            self.next_bus += 1;

            let pair = PortPair { port_a: port_a.clone(), port_b: port_b.clone(), bus_name };
            log::info!("虚拟端口对已创建: {} ↔ {}", port_a, port_b);
            pairs.push(pair.clone());
            self.active_pairs.insert(pair);
        }

        Ok(pairs)
    }

    pub fn destroy_pair(&mut self, pair: &PortPair) -> Result<(), String> {
        let setupc = self.setupc_path();
        let output = Command::new(&setupc)
            .args(["remove", &pair.port_a])
            .output()
            .map_err(|e| format!("销毁虚拟端口对失败: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            log::warn!("销毁 {}↔{} 失败: {}", pair.port_a, pair.port_b, stderr);
        } else {
            log::info!("虚拟端口对已销毁: {} ↔ {}", pair.port_a, pair.port_b);
        }

        self.active_pairs.remove(pair);
        Ok(())
    }

    pub fn cleanup_all(&mut self) {
        let pairs: Vec<PortPair> = self.active_pairs.iter().cloned().collect();
        for pair in pairs {
            let _ = self.destroy_pair(&pair);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_number_allocation() {
        let base: Vec<(u32, u32)> = (0..4)
            .map(|i| (10 + (i * 2) as u32, 10 + (i * 2) as u32 + 1))
            .collect();
        assert_eq!(base[0], (10, 11));
        assert_eq!(base[1], (12, 13));
        assert_eq!(base[2], (14, 15));
        assert_eq!(base[3], (16, 17));
    }

    #[test]
    fn test_count_clamping() {
        assert_eq!(0u32.clamp(1, 4), 1);
        assert_eq!(5u32.clamp(1, 4), 4);
        assert_eq!(3u32.clamp(1, 4), 3);
    }
}
