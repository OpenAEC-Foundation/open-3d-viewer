//! Open 3D Viewer Studio, desktop app (iced + wgpu).
//!
//! Usage:
//!   open-3d-viewer [FILE] [--theme light|forge|night|blueprint] [--no-remote]
//!   open-3d-viewer [FILE] --screenshot OUT.png [--size 1600x1000] [--mode solid|wireframe|edges]
//!                  [--view iso|top|front|right] [--ortho] [--view-tab] [--theme ...]
//!                  [--explode X,Y,Z (percent)] [--explode-mode storeys|elements]
//!                  [--screen model|file|settings]
//!   open-3d-viewer [FILE] --window-screenshot OUT.png   (real window, captured, then exits)
//!   open-3d-viewer --bench FILE [--frames 300]
//!   open-3d-viewer FILE --save-as OUT.ifc   (convert without a window)

use crate::app::{self, App, Message};
use crate::theme::ThemeKind;
use crate::viewport::camera::ViewPreset;
use crate::viewport::render::RenderMode;
use crate::{bench, explode, export, loaders, shell, INTER_REGULAR, INTER_SEMIBOLD};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Default)]
struct Args {
    file: Option<PathBuf>,
    theme: Option<ThemeKind>,
    screenshot: Option<PathBuf>,
    screen: Option<String>,
    window_screenshot: Option<PathBuf>,
    size: Option<(f32, f32)>,
    mode: Option<RenderMode>,
    view: Option<ViewPreset>,
    bench: Option<PathBuf>,
    ortho: bool,
    view_tab: bool,
    frames: Option<u32>,
    /// Do not take remote control commands (the MCP server).
    no_remote: bool,
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
            "--screen" => {
                let screen = value()?;
                if !matches!(screen.as_str(), "model" | "file" | "settings") { return Err("--screen model|file|settings".into()); }
                args.screen = Some(screen);
            }
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
            "--no-remote" => args.no_remote = true,
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
                println!("{}", include_str!("desktop.rs").lines().take(10).map(|l| l.trim_start_matches("//!")).collect::<Vec<_>>().join("\n"));
                std::process::exit(0);
            }
            _ if s.starts_with("--") => return Err(format!("unknown option {s}")),
            _ => args.file = Some(PathBuf::from(a)),
        }
    }
    Ok(args)
}

/// Runs the desktop app with the command line of this process.
pub fn main() {
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
    let start = web_time::Instant::now();
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
    // remote control for AI agents (open-3d-viewer-mcp); O3V_REMOTE=0 or --no-remote switch it off
    app.remote.enabled = !args.no_remote && std::env::var("O3V_REMOTE").as_deref() != Ok("0");
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
    // The default shell starts with its Models pane and the properties pane closed.
    let left_width = (w * openaec_ui::Preferences::default().explorer_ratio).max(190.0).min(w - 300.0);
    app.viewport_size = (w - left_width - 2.0, h - 32.0 - 28.0 - 94.0 - 22.0);
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
    let shell_theme = match app.theme {
        ThemeKind::Light => openaec_ui::Theme::Light,
        ThemeKind::Forge => openaec_ui::Theme::Forge,
        ThemeKind::Night => openaec_ui::Theme::OpenAec,
        ThemeKind::Blueprint => openaec_ui::Theme::Blueprint,
        ThemeKind::Contrast => openaec_ui::Theme::Contrast,
    };
    let preferences = openaec_ui::Preferences { theme: shell_theme, language: app::standalone_language(), ..Default::default() };
    let mut state = openaec_ui::ShellState::new(preferences);
    state.set_window_width(w);
    match args.screen.as_deref() {
        Some("file") => state.update(&openaec_ui::UiEvent::Backstage(true)),
        Some("settings") => state.update(&openaec_ui::UiEvent::Settings(true)),
        _ => {},
    }
    let studio = shell::Studio { app };
    let theme = shell_theme.iced();
    let settings = iced::Settings {
        fonts: openaec_ui::fonts::FILES.iter().map(|bytes| (*bytes).into()).collect(),
        default_font: app::FONT,
        antialiasing: true,
        ..Default::default()
    };
    let mut ui = iced_test::Simulator::with_size(settings, iced::Size::new(w, h), openaec_ui::application_view(&studio, &state, None));
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

/// The app icon.
fn window_icon() -> Option<iced::window::Icon> {
    let decoder = png::Decoder::new(std::io::Cursor::new(include_bytes!("../assets/icons/128x128.png")));
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
