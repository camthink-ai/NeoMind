use crate::extension::accel::Variant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)] // Q4_K_M matches GGUF quant naming + test contract
pub enum Quant {
    Q4_K_M,
    Q8_0,
}

impl Quant {
    pub fn key(&self) -> &'static str {
        match self {
            Quant::Q4_K_M => "q4_k_m",
            Quant::Q8_0 => "q8_0",
        }
    }
    /// Approx size (bytes) of the 2.6B model at this quant, for UI progress hints.
    pub fn approx_bytes(&self) -> u64 {
        match self {
            Quant::Q4_K_M => 1_550_000_000,
            Quant::Q8_0 => 2_800_000_000,
        }
    }
}

/// GPU/Metal 带宽充足 → Q8_0 保质量;CPU/边缘 → Q4_K_M 保速度。
pub fn default_quant(os: &str, variant: Variant) -> Quant {
    if os == "macos" || matches!(variant, Variant::Cuda | Variant::Jetson) {
        Quant::Q8_0
    } else {
        Quant::Q4_K_M
    }
}

pub fn model_file_name(quant: Quant) -> String {
    format!("lfm25-2.6b-{}.gguf", quant.key())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension::accel::Variant;

    #[test]
    fn macos_prefers_q8() {
        assert_eq!(default_quant("macos", Variant::Cpu), Quant::Q8_0);
    }
    #[test]
    fn linux_gpu_prefers_q8() {
        assert_eq!(default_quant("linux", Variant::Cuda), Quant::Q8_0);
        assert_eq!(default_quant("linux", Variant::Jetson), Quant::Q8_0);
    }
    #[test]
    fn linux_cpu_prefers_q4() {
        assert_eq!(default_quant("linux", Variant::Cpu), Quant::Q4_K_M);
    }
    #[test]
    fn quant_keys_and_bytes() {
        assert_eq!(Quant::Q4_K_M.key(), "q4_k_m");
        assert_eq!(Quant::Q8_0.key(), "q8_0");
        assert!(Quant::Q8_0.approx_bytes() > Quant::Q4_K_M.approx_bytes());
    }
}
