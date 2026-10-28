//! Open 3D Viewer Studio, pure Rust edition (iced + wgpu).
//!
//! Usage:
//!   open-3d-viewer-iced [FILE] [--theme light|forge|night|blueprint]
//!   open-3d-viewer-iced [FILE] --screenshot OUT.png [--size 1600x1000] [--mode solid|wireframe|edges]
//!                       [--view iso|top|front|right] [--ortho] [--view-tab] [--theme ...]
//!                       [--explode X,Y,Z (percent)] [--explode-mode storeys|elements]
//!   open-3d-viewer-iced [FILE] --window-screenshot OUT.png   (real window, captured, then exits)
//!   open-3d-viewer-iced --bench FILE [--frames 300]
//!   open-3d-viewer-iced FILE --save-as OUT.ifc   (convert without a window)
#![cfg_attr(all(not(debug_assertions), not(test)), windows_subsystem = "windows")]

mod annotations;
mod app;
mod bench;
mod explode;
mod export;
mod icons;
mod loaders;
mod model;
mod section;
mod shell;
mod theme;
mod view_cube;
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
    ortho: bool,
    view_tab: bool,
    frames: Option<u32>,
    save_as: Option<PathBuf>,
    /// Exploded view for `--screenshot`: percent per axis, and the mode.
    explode: Option<[f32; 3]>,
    explode_mode: Option<explode::ExplodeMode>,
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
            "--ortho" => args.ortho = true,
            "--view-tab" => args.view_tab = true,
            "--bench" => args.bench = Some(PathBuf::from(value()?)),
            "--frames" => args.frames = Some(value()?.parse().map_err(|_| "--frames N")?),
            "--save-as" => args.save_as = Some(PathBuf::from(value()?)),
            "--explode" => {
                let v: Vec<f32> = value()?.split(',').map(|p| p.trim().parse().map_err(|_| "--explode X,Y,Z")).collect::<Result<_, _>>()?;
                args.explode = Some(<[f32; 3]>::try_from(v).map_err(|_| "--explode X,Y,Z")?);
            }
            "--explode-mode" => {
                args.explode_mode = Some(match value()?.as_str() {
                    "storeys" => explode::ExplodeMode::Storeys,
                    "elements" => explode::ExplodeMode::Elements,
                    _ => return Err("--explode-mode storeys|elements".into()),
                })
            }
            "-h" | "--help" => {
                println!("{}", include_str!("main.rs").lines().take(10).map(|l| l.trim_start_matches("//!")).collect::<Vec<_>>().join("\n"));
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
    } else if let Some(out) = &args.save_as {
        save_as(&args, out)
    } else if let Some(out) = &args.screenshot {
        screenshot(&args, out)
    } else if args.window_screenshot.is_some() {
        run_app_standalone(args).map_err(|e| e.to_string())
    } else {
        run_app(args).map_err(|e| e.to_string())
    };
    if let Err(e) = code {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

/// `--save-as`: loads the file and writes it in the format of `out`'s extension.
fn save_as(args: &Args, out: &std::path::Path) -> Result<(), String> {
    let file = args.file.as_ref().ok_or("--save-as needs a FILE to convert")?;
    let model = loaders::load(file)?;
    let start = std::time::Instant::now();
    export::save(&model, out)?;
    println!("{} -> {} in {:.2} s", model.file_name(), out.display(), start.elapsed().as_secs_f32());
    Ok(())
}

fn initial_state(args: &Args) -> App {
    let mut app = App::default();
    if let Some(t) = args.theme {
        app.theme = t;
    }
    if let Some(m) = args.mode {
        app.mode = m;
    }
    app.camera.ortho = args.ortho;
    if args.view_tab {
        app.tab = app::RibbonTab::View;
    }
    app
}

/// The application window: the viewer inside the shared OpenAEC shell (`openaec-ui`), with the
/// preferences (theme, language, scale) in %APPDATA%\OpenAEC\Open 3D Viewer Studio.
fn run_app(args: Args) -> iced::Result {
    let mut app = initial_state(&args);
    app.initial_view = args.view;
    app.pending_open = args.file.clone();
    let prefs = std::env::var_os("APPDATA").map(|d| {
        let dir = PathBuf::from(d).join("OpenAEC").join("Open 3D Viewer Studio");
        let _ = std::fs::create_dir_all(&dir);
        dir.join("preferences.json")
    });
    openaec_ui::run(shell::Studio { app }, prefs)
}

/// The earlier stand-alone window (own ribbon and panels); kept for `--window-screenshot`.
#[allow(dead_code)]
fn run_app_standalone(args: Args) -> iced::Result {
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
    app.viewport_size = (w - 220.0 - 290.0, h - 34.0 - 84.0 - 24.0);
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
        // the model pulled apart, framed as F would
        if let Some(amount) = args.explode {
            app.explode_mode = args.explode_mode.unwrap_or_default();
            let _ = app.update(Message::ToggleExplode);
            for (axis, percent) in amount.into_iter().enumerate() {
                let _ = app.update(Message::Explode(axis, percent));
            }
            app.fit();
        }
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
