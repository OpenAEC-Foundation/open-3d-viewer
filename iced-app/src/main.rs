//! Open 3D Viewer Studio, pure Rust edition (iced + wgpu).
//!
//! Usage:
//!   open-3d-viewer-iced [FILE] [--theme light|forge|night|blueprint]
//!   open-3d-viewer-iced [FILE] --screenshot OUT.png [--size 1600x1000] [--mode solid|wireframe|edges]
//!                       [--view iso|top|front|right] [--theme ...]
//!   open-3d-viewer-iced [FILE] --window-screenshot OUT.png   (real window, captured, then exits)
//!   open-3d-viewer-iced --bench FILE [--frames 300]
#![cfg_attr(all(not(debug_assertions), not(test)), windows_subsystem = "windows")]

mod app;
mod bench;
mod icons;
mod loaders;
mod model;
mod theme;
mod viewport;

use app::{App, Message};
use std::path::PathBuf;
use std::sync::Arc;
use theme::ThemeKind;
use viewport::camera::ViewPreset;
use viewport::render::RenderMode;

const INTER_REGULAR: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
const INTER_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/Inter-SemiBold.ttf");

#[derive(Default)]
struct Args {
    file: Option<PathBuf>,
    theme: Option<ThemeKind>,
    screenshot: Option<PathBuf>,
    window_screenshot: Option<PathBuf>,
    size: Option<(f32, f32)>,
    mode: Option<RenderMode>,
    view: Option<ViewPreset>,
    bench: Option<PathBuf>,
    frames: Option<u32>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args::default();
    let mut it = std::env::args_os().skip(1);
    while let Some(a) = it.next() {
        let s = a.to_string_lossy().to_string();
        let mut value = || it.next().map(|v| v.to_string_lossy().to_string()).ok_or(format!("{s} needs a value"));
        match s.as_str() {
            "--theme" => args.theme = Some(ThemeKind::parse(&value()?).ok_or("unknown theme")?),
            "--screenshot" => args.screenshot = Some(PathBuf::from(value()?)),
            "--window-screenshot" => args.window_screenshot = Some(PathBuf::from(value()?)),
            "--size" => {
                let v = value()?;
                let (w, h) = v.split_once('x').ok_or("--size WxH")?;
                args.size = Some((w.parse().map_err(|_| "--size WxH")?, h.parse().map_err(|_| "--size WxH")?));
            }
            "--mode" => {
                args.mode = Some(match value()?.as_str() {
                    "solid" => RenderMode::Solid,
                    "wireframe" => RenderMode::Wireframe,
                    "edges" => RenderMode::SolidEdges,
                    _ => return Err("--mode solid|wireframe|edges".into()),
                })
            }
            "--view" => {
                args.view = Some(match value()?.as_str() {
                    "iso" => ViewPreset::Iso,
                    "top" => ViewPreset::Top,
                    "front" => ViewPreset::Front,
                    "right" => ViewPreset::Right,
                    _ => return Err("--view iso|top|front|right".into()),
                })
            }
            "--bench" => args.bench = Some(PathBuf::from(value()?)),
            "--frames" => args.frames = Some(value()?.parse().map_err(|_| "--frames N")?),
            "-h" | "--help" => {
                println!("{}", include_str!("main.rs").lines().take(8).map(|l| l.trim_start_matches("//!")).collect::<Vec<_>>().join("\n"));
                std::process::exit(0);
            }
            _ if s.starts_with("--") => return Err(format!("unknown option {s}")),
            _ => args.file = Some(PathBuf::from(a)),
        }
    }
    Ok(args)
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    };
    let code = if let Some(file) = &args.bench {
        bench::run(file, args.frames.unwrap_or(300))
    } else if let Some(out) = &args.screenshot {
        screenshot(&args, out)
    } else {
        run_app(args).map_err(|e| e.to_string())
    };
    if let Err(e) = code {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn initial_state(args: &Args) -> App {
    let mut app = App::default();
    if let Some(t) = args.theme {
        app.theme = t;
    }
    if let Some(m) = args.mode {
        app.mode = m;
    }
    app
}

fn run_app(args: Args) -> iced::Result {
    let file = args.file.clone();
    let boot = move || {
        let mut app = initial_state(&args);
        app.initial_view = args.view;
        app.window_shot = args.window_screenshot.clone();
        let task = match &file {
            Some(path) => iced::Task::done(Message::Open(path.clone())),
            None => app.shot_after_delay(),
        };
        (app, task)
    };
    iced::application(boot, App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .font(INTER_REGULAR)
        .font(INTER_SEMIBOLD)
        .default_font(app::FONT)
        .antialiasing(true)
        .window(iced::window::Settings {
            size: iced::Size::new(1440.0, 900.0),
            icon: window_icon(),
            ..Default::default()
        })
        .run()
}

/// Renders one frame of the full UI headlessly (wgpu offscreen) and writes a PNG.
fn screenshot(args: &Args, out: &std::path::Path) -> Result<(), String> {
    let mut app = initial_state(args);
    let (w, h) = args.size.unwrap_or((1440.0, 900.0));
    // Viewport area = window minus side panels, ribbon and status bar (see app.rs)
    app.viewport_size = (w - 2.0 * 270.0, h - 34.0 - 84.0 - 24.0);
    if let Some(file) = &args.file {
        let model = loaders::load(file)?;
        eprintln!(
            "loaded {}: {} layers, {} triangles in {:.2} s",
            model.file_name(),
            model.layers.len(),
            model.triangle_count(),
            model.load_time.as_secs_f32()
        );
        app.initial_view = args.view;
        app.set_model(Arc::new(model));
    }
    let theme = app.theme();
    let settings = iced::Settings {
        fonts: vec![INTER_REGULAR.into(), INTER_SEMIBOLD.into()],
        default_font: app::FONT,
        antialiasing: true,
        ..Default::default()
    };
    let mut ui = iced_test::Simulator::with_size(settings, iced::Size::new(w, h), app.view());
    let snapshot = ui.snapshot(&theme).map_err(|e| e.to_string())?;
    // iced_test names the file "<stem>-<renderer>.png"; write it next to OUT and rename.
    let tmp = out.with_file_name(format!("{}.o3vtmp", out.file_stem().unwrap_or_default().to_string_lossy()));
    let written = tmp.with_file_name(format!("{}-wgpu.png", tmp.file_stem().unwrap_or_default().to_string_lossy()));
    let _ = std::fs::remove_file(&written);
    snapshot.matches_image(&tmp).map_err(|e| e.to_string())?;
    if !written.exists() {
        return Err("no wgpu adapter for the headless renderer (tiny-skia cannot draw the 3D viewport)".into());
    }
    std::fs::rename(&written, out).map_err(|e| e.to_string())?;
    eprintln!("wrote {}", out.display());
    Ok(())
}

/// The app icon (same artwork as the Tauri app).
fn window_icon() -> Option<iced::window::Icon> {
    let decoder = png::Decoder::new(std::io::Cursor::new(include_bytes!("../../src-tauri/icons/128x128.png")));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => buf[..info.buffer_size()].chunks_exact(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        _ => return None,
    };
    iced::window::icon::from_rgba(rgba, info.width, info.height).ok()
}

/// Writes RGBA8 pixels as a PNG file.
pub fn write_png(path: &std::path::Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())
}
