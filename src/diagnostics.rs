use crate::model::{Failure, Repair};
use regex::Regex;

pub fn classify(output: &str, status: Option<i32>) -> Failure {
    let rules = [
        (
            r"(?i)(msvcp\d+|vcruntime\d+)\.dll.*(not found|missing)",
            "missing-vc-runtime",
            "A Visual C++ runtime is missing",
            Repair::AddVcrun,
        ),
        (
            r"(?i)(mscoree|clr|\.net).*(failed|not found|missing)",
            "missing-dotnet",
            "The required .NET runtime is missing",
            Repair::AddDotNet,
        ),
        (
            r"(?i)(failed to create.*(device|swapchain)|dxvk.*error|vulkan.*(failed|missing))",
            "graphics-vulkan",
            "Vulkan graphics initialization failed",
            Repair::UseOpenGl,
        ),
        (
            r"(?i)(d3d12|vkd3d).*(failed|error)",
            "graphics-vkd3d",
            "Direct3D 12 translation failed",
            Repair::DisableDxvk,
        ),
        (
            r"(?i)(unsupported windows version|requires windows (7|8|10|11))",
            "windows-version",
            "The selected Windows version is incompatible",
            Repair::ChangeWindowsVersion,
        ),
        (
            r"(?i)(mfplat|mfreadwrite|evr)\.dll.*(not found|missing|failed)",
            "missing-media-foundation",
            "Windows Media Foundation components are missing",
            Repair::AddMediaFoundation,
        ),
        (
            r"(?i)d3dcompiler_4[367]\.dll.*(not found|missing)",
            "missing-d3d-compiler",
            "A DirectX shader compiler runtime is missing",
            Repair::AddDirectXCompiler,
        ),
        (
            r"(?i)(xactengine|xaudio2)_?\d*\.dll.*(not found|missing)",
            "missing-xact",
            "A legacy XAudio/XACT runtime is missing",
            Repair::AddXact,
        ),
        (
            r"(?i)(fsync|ntsync).*(not supported|failed|permission denied)",
            "sync-backend",
            "The selected Wine synchronization backend is unavailable",
            Repair::DisableSync,
        ),
        (
            r"(?i)(service_kernel_driver|ntloaddriver|failed to load.*\.sys|kernel.*anti.?cheat)",
            "windows-kernel-required",
            "The application attempted to install or load a Windows kernel component",
            Repair::FallbackExecutionClass,
        ),
        (
            r"(?i)(virtual machine detected|hypervisor detected|unsupported virtual environment)",
            "virtualization-sensitive",
            "The application's trust system rejected the current environment",
            Repair::FallbackExecutionClass,
        ),
    ];
    for (pattern, category, summary, repair) in rules {
        if Regex::new(pattern).expect("static regex").is_match(output) {
            return Failure {
                category,
                summary: summary.into(),
                retryable: true,
                repair: Some(repair),
            };
        }
    }
    Failure {
        category: "unknown",
        summary: format!(
            "the Windows process exited with {}",
            status
                .map(|s| s.to_string())
                .unwrap_or_else(|| "no status".into())
        ),
        retryable: true,
        repair: Some(Repair::FallbackRunner),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finds_vcrun() {
        assert_eq!(
            classify("vcruntime140.dll not found", Some(1)).category,
            "missing-vc-runtime"
        );
    }
    #[test]
    fn falls_back() {
        assert_eq!(classify("mystery", Some(1)).category, "unknown");
    }

    #[test]
    fn kernel_failures_advance_execution_class() {
        let failure = classify("NtLoadDriver failed for guard.sys", Some(1));
        assert_eq!(failure.category, "windows-kernel-required");
        assert!(matches!(
            failure.repair,
            Some(Repair::FallbackExecutionClass)
        ));
    }
}
