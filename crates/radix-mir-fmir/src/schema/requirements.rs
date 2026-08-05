/// Whether a runtime requirement is a known `host:*` capability.
///
/// `kernel:*` requirements are validated by the `faber` loading policy, which
/// has access to the kernel module/verb manifest resolution in the `radix`
/// package.
pub fn is_known_host_requirement(requirement: &str) -> bool {
    matches!(
        requirement,
        "host:argv"
            | "host:exit"
            | "host:stdout"
            | "host:stderr"
            | "host:stdin"
            | "host:fs"
            | "host:env"
            | "host:cwd"
            | "host:pid"
            | "host:random"
            | "host:process"
    )
}

/// Whether a runtime requirement is a known `device:*` capability (N1.7).
/// `device:metal` and `device:cuda` are the frozen device requirements for the
/// differentiable-GPU campaign.
pub fn is_known_device_requirement(requirement: &str) -> bool {
    matches!(requirement, "device:metal" | "device:cuda")
}

/// Whether a runtime requirement is a known admitted capability (host **or**
/// device). This is the package-load admission allowlist: the
/// [`is_known_host_requirement`] allowlist extended with `device:metal` /
/// `device:cuda` (N1.7). A requirement that is neither a known `host:*` nor a
/// known `device:*` fails admission.
pub fn is_known_requirement(requirement: &str) -> bool {
    is_known_host_requirement(requirement) || is_known_device_requirement(requirement)
}
