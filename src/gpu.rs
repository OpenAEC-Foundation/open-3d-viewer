//! The graphics backend of the desktop window.
//!
//! wgpu takes Vulkan on Windows when the driver offers it. Intel's Vulkan swapchain shows the
//! whole window white for single frames while it redraws (issue #28, gfx-rs/wgpu#3452), and DXGI
//! swapchains behave better on every vendor (gfx-rs/wgpu#2719), so on Windows the window draws
//! with DirectX 12 when the GPU it gets is an Intel one (asked from DXGI, see `dxgi_gpu`).
//!
//! To try another combination: `--gpu-backend dx12|vulkan|gl` or the variable `WGPU_BACKEND`,
//! and `--present-mode fifo|mailbox|immediate|...` or `ICED_PRESENT_MODE` (both read by
//! iced and wgpu). What the window got is written to `gpu.log` in the settings folder.

use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use web_time::Instant;

/// The values of `--gpu-backend`, a comma list as `WGPU_BACKEND` takes it.
const BACKENDS: [&str; 7] = ["dx12", "d3d12", "vulkan", "vk", "gl", "gles", "opengl"];
/// The values of `--present-mode`, as `ICED_PRESENT_MODE` takes them.
const PRESENT_MODES: [&str; 6] = ["vsync", "no_vsync", "fifo", "fifo_relaxed", "mailbox", "immediate"];
/// Intel's PCI vendor id.
#[cfg(windows)]
const INTEL: u32 = 0x8086;
/// `gpu.log` stops growing here (wgpu may warn every frame).
const LOG_LIMIT: u64 = 256 * 1024;

/// Checks a `--gpu-backend` value.
pub fn parse_backend(value: &str) -> Result<String, String> {
    let value = value.trim().to_lowercase();
    match value.split(',').all(|b| BACKENDS.contains(&b.trim())) {
        true => Ok(value),
        false => Err(format!("--gpu-backend {}", BACKENDS.join("|"))),
    }
}

/// Checks a `--present-mode` value.
pub fn parse_present_mode(value: &str) -> Result<String, String> {
    let value = value.trim().to_lowercase();
    match PRESENT_MODES.contains(&value.as_str()) {
        true => Ok(value),
        false => Err(format!("--present-mode {}", PRESENT_MODES.join("|"))),
    }
}

/// Chooses the backend and present mode of the window before iced starts, and logs what the
/// window gets to `dir/gpu.log`. Must run before any other thread reads the environment.
pub fn configure(backend: Option<String>, present_mode: Option<String>, dir: Option<&Path>) {
    let start = Instant::now();
    // iced and wgpu read these when they create the window's surface
    if let Some(b) = backend {
        std::env::set_var("WGPU_BACKEND", b);
    }
    if let Some(p) = present_mode {
        std::env::set_var("ICED_PRESENT_MODE", p);
    }
    #[cfg(windows)]
    let gpu = dxgi_gpu();
    #[cfg(not(windows))]
    let gpu: Option<String> = None;
    let mut choice = "as set";
    if std::env::var_os("WGPU_BACKEND").is_none() {
        choice = "wgpu default";
        #[cfg(windows)]
        if gpu.as_ref().is_some_and(|g| g.vendor == INTEL) {
            std::env::set_var("WGPU_BACKEND", "dx12");
            choice = "DirectX 12 for Intel, whose Vulkan swapchain flickers";
        }
    }
    install_log(dir);
    let var = |name| std::env::var(name).unwrap_or_else(|_| "-".into());
    log::info!(
        target: "o3v::gpu",
        "Open 3D Viewer Studio {}: GPU {}; WGPU_BACKEND={} ({choice}), ICED_PRESENT_MODE={}, WGPU_POWER_PREF={} in {} ms",
        env!("CARGO_PKG_VERSION"),
        gpu.map_or_else(|| "-".into(), |g| g.to_string()),
        var("WGPU_BACKEND"),
        var("ICED_PRESENT_MODE"),
        var("WGPU_POWER_PREF"),
        start.elapsed().as_millis()
    );
    // At info level iced lists the adapters of every backend once more before it picks one,
    // which can take seconds with all of them; with one backend chosen the details are cheap.
    if std::env::var_os("WGPU_BACKEND").is_none() {
        log::info!(target: "o3v::gpu", "adapter details: set WGPU_BACKEND or --gpu-backend");
        log::set_max_level(log::LevelFilter::Warn);
    }
}

/// The GPU that iced will ask for on Windows.
#[cfg(windows)]
struct Gpu {
    name: String,
    vendor: u32,
    device: u32,
    driver: String,
}

#[cfg(windows)]
impl std::fmt::Display for Gpu {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{} ({:04x}:{:04x}), driver {}", self.name, self.vendor, self.device, self.driver)
    }
}

/// The first hardware adapter in DXGI's order for iced's power preference (high performance
/// unless `WGPU_POWER_PREF` says otherwise), as wgpu ranks them: a laptop with a discrete GPU
/// gets that one. DXGI alone answers in milliseconds, where wgpu opens a device per adapter.
#[cfg(windows)]
fn dxgi_gpu() -> Option<Gpu> {
    use iced::wgpu;
    use windows::core::Interface;
    use windows::Win32::Graphics::Dxgi::*;
    let preference = match wgpu::PowerPreference::from_env() {
        Some(wgpu::PowerPreference::LowPower) => DXGI_GPU_PREFERENCE_MINIMUM_POWER,
        Some(wgpu::PowerPreference::None) => DXGI_GPU_PREFERENCE_UNSPECIFIED,
        _ => DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE,
    };
    // SAFETY: DXGI queries on a factory and adapters owned here; the factory is dropped after.
    unsafe {
        let factory: IDXGIFactory6 = CreateDXGIFactory1().ok()?;
        (0..).map_while(|i| factory.EnumAdapterByGpuPreference::<IDXGIAdapter1>(i, preference).ok()).find_map(|adapter| {
            let desc = adapter.GetDesc1().ok()?;
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                return None;
            }
            let len = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
            // the user-mode driver version, as Windows shows it: four 16-bit parts
            let driver = adapter.CheckInterfaceSupport(&IDXGIDevice::IID).map_or_else(
                |_| "-".into(),
                |v| format!("{}.{}.{}.{}", v >> 48 & 0xffff, v >> 32 & 0xffff, v >> 16 & 0xffff, v & 0xffff),
            );
            Some(Gpu { name: String::from_utf16_lossy(&desc.Description[..len]), vendor: desc.VendorId, device: desc.DeviceId, driver })
        })
    }
}

/// Writes what iced and wgpu report about the GPU to `gpu.log` (and stderr): the adapters, the
/// one selected with its driver and backend, the surface format and alpha mode, and warnings.
/// Starts at info level; `configure` lowers it to warnings when the backend is wgpu's choice.
struct GpuLog {
    file: Mutex<Option<std::fs::File>>,
    start: Instant,
}

impl log::Log for GpuLog {
    fn enabled(&self, meta: &log::Metadata) -> bool {
        let target = meta.target();
        match meta.level() {
            log::Level::Error | log::Level::Warn => target.starts_with("wgpu") || target.starts_with("iced") || target.starts_with("o3v"),
            log::Level::Info => target.starts_with("iced_wgpu") || target.starts_with("o3v"),
            _ => false,
        }
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!("[{:7.3}] {} {}: {}\n", self.start.elapsed().as_secs_f32(), record.level(), record.target(), record.args());
        eprint!("{line}");
        let mut file = self.file.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(f) = file.as_mut() {
            if f.metadata().map_or(true, |m| m.len() < LOG_LIMIT) {
                let _ = f.write_all(line.as_bytes());
            }
        }
    }

    fn flush(&self) {}
}

/// The log of this run, `gpu.log` in `dir`, replacing the one of the previous run.
fn install_log(dir: Option<&Path>) {
    let file = dir.and_then(|d| std::fs::File::create(d.join("gpu.log")).ok());
    if log::set_boxed_logger(Box::new(GpuLog { file: Mutex::new(file), start: Instant::now() })).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::wgpu;

    #[test]
    fn overrides_take_what_wgpu_and_iced_read() {
        assert_eq!(parse_backend("DX12").as_deref(), Ok("dx12"));
        assert_eq!(parse_backend("vulkan, gl").as_deref(), Ok("vulkan, gl"));
        assert!(parse_backend("metal").is_err());
        assert!(parse_backend("").is_err());
        assert_eq!(parse_present_mode("Mailbox").as_deref(), Ok("mailbox"));
        assert!(parse_present_mode("adaptive").is_err());
        // the same names as wgpu's own parser
        assert_eq!(wgpu::Backends::from_comma_list("vulkan, gl"), wgpu::Backends::VULKAN | wgpu::Backends::GL);
    }
}
