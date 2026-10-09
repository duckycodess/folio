//! What the measurements ran on: host, runtime versions and prompt templates.
//!
//! Everything here is read from the machine or hashed from the templates in use.
//! A value that cannot be read is `None`; the installed RAM is capacity, never
//! how much was used.

use crate::contracts::{Language, OffsetUnit, SourcePassage};
use crate::error::{CoreError, CoreResult};
use crate::generation::ChatMessage;
use crate::grounding::build_summary_messages;
use crate::interpretation::build_interpretation_messages;
use crate::lab::record::HostInfo;
use crate::models::sha256_bytes;
use std::path::Path;
use std::process::Command;

#[cfg_attr(target_os = "linux", allow(dead_code))]
fn command_stdout(program: &str, args: &[&str]) -> Option<String> {
    let mut command = Command::new(program);
    command.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console flash when the app asks.
        command.creation_flags(0x0800_0000);
    }
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// Memory, in bytes, from the `MemTotal` line of a Linux `/proc/meminfo`.
pub fn parse_meminfo_total(meminfo: &str) -> Option<u64> {
    let line = meminfo.lines().find(|line| line.starts_with("MemTotal:"))?;
    let mut parts = line["MemTotal:".len()..].split_whitespace();
    let value: u64 = parts.next()?.parse().ok()?;
    match parts.next() {
        Some("kB") => value.checked_mul(1024),
        _ => None,
    }
}

/// The first `model name` of a Linux `/proc/cpuinfo`.
pub fn parse_cpuinfo_model(cpuinfo: &str) -> Option<String> {
    cpuinfo
        .lines()
        .find(|line| line.starts_with("model name"))
        .and_then(|line| line.split_once(':'))
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// The `PRETTY_NAME` of a Linux `/etc/os-release`.
pub fn parse_os_release(os_release: &str) -> Option<String> {
    os_release
        .lines()
        .find_map(|line| line.strip_prefix("PRETTY_NAME="))
        .map(|value| value.trim().trim_matches('"').to_string())
        .filter(|value| !value.is_empty())
}

/// The version inside the output of Windows `ver`, for example
/// `Microsoft Windows [Version 10.0.26100.1]`.
pub fn parse_windows_ver(output: &str) -> Option<String> {
    let start = output.find("[Version ")? + "[Version ".len();
    let end = output[start..].find(']')? + start;
    Some(output[start..end].trim().to_string()).filter(|value| !value.is_empty())
}

/// The processor name from `reg query ... /v ProcessorNameString`.
pub fn parse_reg_processor_name(output: &str) -> Option<String> {
    output
        .lines()
        .find(|line| line.contains("ProcessorNameString"))
        .and_then(|line| line.split_once("REG_SZ"))
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(target_os = "linux")]
fn platform() -> (Option<String>, Option<String>, Option<u64>) {
    (
        std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|text| parse_os_release(&text)),
        std::fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|text| parse_cpuinfo_model(&text)),
        std::fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|text| parse_meminfo_total(&text)),
    )
}

#[cfg(target_os = "macos")]
fn platform() -> (Option<String>, Option<String>, Option<u64>) {
    (
        command_stdout("sw_vers", &["-productVersion"]),
        command_stdout("sysctl", &["-n", "machdep.cpu.brand_string"]),
        command_stdout("sysctl", &["-n", "hw.memsize"]).and_then(|text| text.parse().ok()),
    )
}

#[cfg(windows)]
fn platform() -> (Option<String>, Option<String>, Option<u64>) {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let ram = unsafe {
        let mut status = MEMORYSTATUSEX::default();
        status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        (GlobalMemoryStatusEx(&mut status) != 0).then_some(status.ullTotalPhys)
    };
    (
        command_stdout("cmd", &["/C", "ver"]).and_then(|text| parse_windows_ver(&text)),
        command_stdout(
            "reg",
            &[
                "query",
                r"HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0",
                "/v",
                "ProcessorNameString",
            ],
        )
        .and_then(|text| parse_reg_processor_name(&text)),
        ram,
    )
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn platform() -> (Option<String>, Option<String>, Option<u64>) {
    (None, None, None)
}

pub fn host_info() -> HostInfo {
    let (os_version, cpu_brand, installed_ram_bytes) = platform();
    HostInfo {
        os: std::env::consts::OS.to_string(),
        os_version,
        arch: std::env::consts::ARCH.to_string(),
        cpu_brand,
        // 0 means the count could not be read.
        logical_cpus: std::thread::available_parallelism()
            .map(|count| count.get() as u32)
            .unwrap_or(0),
        installed_ram_bytes,
    }
}

/// The one-line `hardware` summary of the frozen `BenchmarkResult`.
pub fn hardware_summary(host: &HostInfo) -> String {
    let ram = host
        .installed_ram_bytes
        .map(|bytes| format!("{bytes} B installed RAM"))
        .unwrap_or_else(|| "installed RAM unavailable".to_string());
    let cpus = if host.logical_cpus == 0 {
        "logical CPU count unavailable".to_string()
    } else {
        format!("{} logical CPUs", host.logical_cpus)
    };
    format!(
        "{} {}{}, {}{}, {}",
        host.os,
        host.arch,
        host.os_version
            .as_deref()
            .map(|version| format!(" ({version})"))
            .unwrap_or_default(),
        host.cpu_brand.as_deref().unwrap_or("CPU model unavailable"),
        format!(", {cpus}"),
        ram
    )
}

fn combined_output(executable: &Path, argument: &str) -> CoreResult<String> {
    let mut command = Command::new(executable);
    command.arg(argument);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let output = command.output()?;
    let mut text = String::new();
    for stream in [&output.stdout, &output.stderr] {
        for line in String::from_utf8_lossy(stream).lines() {
            let line = line.trim();
            if !line.is_empty() {
                if !text.is_empty() {
                    text.push_str(" | ");
                }
                text.push_str(line);
            }
        }
    }
    if text.is_empty() {
        return Err(CoreError::Message(format!(
            "llama-server {argument} printed nothing"
        )));
    }
    Ok(text)
}

/// The exact `llama-server --version` output of the verified executable. The
/// version prints to stderr or stdout depending on the build, so both are kept.
pub fn llama_server_version(executable: &Path) -> CoreResult<String> {
    combined_output(executable, "--version")
}

const MAX_DEVICE_LISTING: usize = 2000;

/// Keeps a device listing as observed, bounded so a record cannot grow without
/// limit. Truncation is marked, not silent.
pub fn bound_device_listing(listing: &str) -> String {
    if listing.chars().count() <= MAX_DEVICE_LISTING {
        return listing.to_string();
    }
    let kept: String = listing.chars().take(MAX_DEVICE_LISTING).collect();
    format!("{kept} [truncated]")
}

/// `llama-server --list-devices`, as printed. This is an observation of what
/// the runtime can see, not proof of what a request used.
pub fn llama_server_devices(executable: &Path) -> CoreResult<String> {
    combined_output(executable, "--list-devices").map(|text| bound_device_listing(&text))
}

/// What a llama-server's own startup output says about its backend.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BackendObservation {
    /// Backend, device and offload lines, as printed.
    pub excerpt: Option<String>,
    /// A line other than the offload count names a GPU backend or device
    /// (Metal, CUDA, Vulkan, OpenCL, ROCm, SYCL or "GPU").
    pub gpu_backend_mentioned: bool,
    pub gpu_layers_offloaded: Option<u32>,
    pub layers_total: Option<u32>,
}

const BACKEND_KEYWORDS: &[&str] = &[
    "backend", "offload", "device", "metal", "cuda", "vulkan", "opencl", "blas", "gpu",
];
const GPU_WORDS: &[&str] = &["metal", "cuda", "vulkan", "opencl", "rocm", "sycl", "gpu"];
const MAX_EXCERPT_LINES: usize = 30;
const MAX_EXCERPT_CHARS: usize = 3000;

fn offloaded_layers(line: &str) -> Option<(u32, u32)> {
    let rest = &line[line.find("offloaded ")? + "offloaded ".len()..];
    let (done, rest) = rest.split_once('/')?;
    let total: String = rest.chars().take_while(char::is_ascii_digit).collect();
    Some((done.trim().parse().ok()?, total.parse().ok()?))
}

/// Reads the backend facts a server printed. It keeps what the server said and
/// concludes nothing: a log with no offload line leaves the counts `None`.
pub fn parse_backend_log(log: &str) -> BackendObservation {
    let mut lines = Vec::new();
    let mut offloaded = None;
    let mut gpu_backend_mentioned = false;
    for line in log.lines() {
        let line = line.trim();
        let lower = line.to_lowercase();
        if line.is_empty() || !BACKEND_KEYWORDS.iter().any(|word| lower.contains(word)) {
            continue;
        }
        if let Some(found) = offloaded_layers(line) {
            offloaded = Some(found);
        } else if GPU_WORDS.iter().any(|word| lower.contains(word)) {
            gpu_backend_mentioned = true;
        }
        if lines.len() < MAX_EXCERPT_LINES {
            lines.push(line.to_string());
        }
    }
    let excerpt = (!lines.is_empty()).then(|| {
        let joined = lines.join(" | ");
        if joined.chars().count() > MAX_EXCERPT_CHARS {
            let kept: String = joined.chars().take(MAX_EXCERPT_CHARS).collect();
            format!("{kept} [truncated]")
        } else {
            joined
        }
    });
    BackendObservation {
        excerpt,
        gpu_backend_mentioned,
        gpu_layers_offloaded: offloaded.map(|(done, _)| done),
        layers_total: offloaded.map(|(_, total)| total),
    }
}

/// ONNX Runtime as linked into this build, with the `ort` crate version.
pub fn onnxruntime_version() -> String {
    // `OrtE5Provider` registers no execution provider, so ONNX Runtime uses its
    // default CPU provider; the crate enables no GPU provider feature.
    format!(
        "ort 2.0.0-rc.13 (CPU execution provider; no GPU execution provider is registered); {}",
        ort::info()
    )
}

fn placeholder_passage() -> SourcePassage {
    SourcePassage {
        document_id: "prompt-fingerprint.md".into(),
        document_content_hash: "sha256:fingerprint".into(),
        offset_unit: OffsetUnit::Utf8Byte,
        start: 0,
        end: 11,
        text: "placeholder".into(),
        page: None,
    }
}

/// Hash of the prompt templates a lab run uses: the interpretation prompt and
/// the summary prompt in each response language, built around fixed
/// placeholders. A change to any template changes it.
pub fn prompt_fingerprint() -> String {
    let mut messages: Vec<ChatMessage> = build_interpretation_messages("placeholder request");
    for language in [Language::En, Language::Fil, Language::Mixed] {
        messages.extend(build_summary_messages(&[placeholder_passage()], &language));
    }
    sha256_bytes(&serde_json::to_vec(&messages).expect("chat messages are serializable"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_files_are_parsed_without_guessing() {
        assert_eq!(
            parse_meminfo_total("MemTotal:       16384 kB\nMemFree: 1 kB\n"),
            Some(16384 * 1024)
        );
        assert_eq!(parse_meminfo_total("MemTotal: 16 MB\n"), None);
        assert_eq!(parse_meminfo_total("MemFree: 1 kB\n"), None);
        assert_eq!(
            parse_cpuinfo_model("processor\t: 0\nmodel name\t: Example CPU @ 3GHz\n"),
            Some("Example CPU @ 3GHz".to_string())
        );
        assert_eq!(parse_cpuinfo_model("processor\t: 0\n"), None);
        assert_eq!(
            parse_os_release("NAME=\"X\"\nPRETTY_NAME=\"Example OS 1.2\"\n"),
            Some("Example OS 1.2".to_string())
        );
    }

    #[test]
    fn windows_output_is_parsed_without_guessing() {
        assert_eq!(
            parse_windows_ver("\r\nMicrosoft Windows [Version 10.0.26100.1742]\r\n"),
            Some("10.0.26100.1742".to_string())
        );
        assert_eq!(parse_windows_ver("not windows"), None);
        let reg = "\r\nHKEY_LOCAL_MACHINE\\HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0\r\n    ProcessorNameString    REG_SZ    Example(R) CPU @ 2.60GHz\r\n";
        assert_eq!(
            parse_reg_processor_name(reg),
            Some("Example(R) CPU @ 2.60GHz".to_string())
        );
        assert_eq!(parse_reg_processor_name("nothing here"), None);
    }

    #[test]
    fn the_host_reports_the_running_system() {
        let host = host_info();
        assert_eq!(host.os, std::env::consts::OS);
        assert_eq!(host.arch, std::env::consts::ARCH);
        assert!(host.logical_cpus >= 1);
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        assert!(host.installed_ram_bytes.is_some_and(|bytes| bytes > 0));
    }

    #[test]
    fn the_hardware_summary_says_what_is_unavailable() {
        let host = HostInfo {
            os: "testos".into(),
            os_version: None,
            arch: "x86_64".into(),
            cpu_brand: None,
            logical_cpus: 0,
            installed_ram_bytes: None,
        };
        let summary = hardware_summary(&host);
        assert!(summary.contains("CPU model unavailable"), "{summary}");
        assert!(
            summary.contains("logical CPU count unavailable"),
            "{summary}"
        );
        assert!(summary.contains("installed RAM unavailable"), "{summary}");
    }

    #[test]
    fn a_long_device_listing_is_bounded_and_says_so() {
        assert_eq!(bound_device_listing("Metal: Apple M1"), "Metal: Apple M1");
        let long = "x".repeat(MAX_DEVICE_LISTING + 5);
        let bounded = bound_device_listing(&long);
        assert!(bounded.ends_with(" [truncated]"));
        assert_eq!(
            bounded.chars().count(),
            MAX_DEVICE_LISTING + " [truncated]".len()
        );
    }

    #[test]
    fn a_server_log_is_read_for_backend_lines_and_the_offload_count() {
        // Lines in the shape llama.cpp prints; not output from a real run.
        let log = "build: 11524 (abc)\n\
            load_backend: loaded CPU backend from C:\\x\\ggml-cpu.dll\n\
            llama_model_load: loading model\n\
            load_tensors: offloaded 0/29 layers to GPU\n\
            main: server is listening";
        let seen = parse_backend_log(log);
        assert_eq!(seen.gpu_layers_offloaded, Some(0));
        assert_eq!(seen.layers_total, Some(29));
        let excerpt = seen.excerpt.unwrap();
        assert!(excerpt.contains("loaded CPU backend"));
        assert!(excerpt.contains("offloaded 0/29 layers to GPU"));
        assert!(!excerpt.contains("loading model"));

        let gpu = parse_backend_log("load_tensors: offloaded 29/29 layers to GPU");
        assert_eq!(
            gpu.gpu_layers_offloaded,
            Some(29),
            "a GPU is reported as observed"
        );
    }

    #[test]
    fn a_log_without_backend_lines_leaves_everything_unobserved() {
        assert_eq!(parse_backend_log(""), BackendObservation::default());
        let quiet = parse_backend_log("main: server is listening");
        assert_eq!(quiet, BackendObservation::default());
        let no_count = parse_backend_log("ggml_metal_init: found device: Apple M1");
        assert!(no_count.excerpt.is_some());
        assert!(no_count.gpu_backend_mentioned);
        assert_eq!(
            no_count.gpu_layers_offloaded, None,
            "a device line is not an offload count"
        );
    }

    #[test]
    fn the_excerpt_is_bounded() {
        let log: String = (0..100).map(|i| format!("backend line {i}\n")).collect();
        let seen = parse_backend_log(&log);
        assert_eq!(
            seen.excerpt.unwrap().matches("backend line").count(),
            MAX_EXCERPT_LINES
        );
    }

    #[test]
    fn the_prompt_fingerprint_is_stable_and_covers_the_templates() {
        let first = prompt_fingerprint();
        assert_eq!(first.len(), 64);
        assert_eq!(first, prompt_fingerprint());
    }

    #[test]
    fn a_missing_server_executable_is_an_error_not_a_version() {
        let missing = Path::new("this-llama-server-does-not-exist");
        assert!(llama_server_version(missing).is_err());
        assert!(llama_server_devices(missing).is_err());
    }
}
