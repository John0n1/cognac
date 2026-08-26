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
}
