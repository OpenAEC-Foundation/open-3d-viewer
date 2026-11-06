//! Open 3D Viewer Studio, desktop app (iced + wgpu).
//!
//! Usage:
//!   open-3d-viewer [FILE] [--theme light|dark|blueprint|high-contrast-dark|high-contrast-light]
//!                  [--language en|nl] [--no-remote]
//!                  [--gpu-backend dx12|vulkan|gl] [--present-mode vsync|no_vsync|fifo|mailbox]
//!   open-3d-viewer [FILE] --screenshot OUT.png [--size 1600x1000] [--mode solid|wireframe|edges]
//!                  [--view iso|top|front|right] [--ortho] [--view-tab] [--theme ...] [--language ...]
//!                  [--page workspace|file|settings|shortcuts] [--schedule NAME] [--sheet NUMBER]
//!                  [--recent PATH]...
//!                  [--explode X,Y,Z (percent)] [--explode-mode storeys|elements]
//!   open-3d-viewer [FILE] --window-screenshot OUT.png   (real window, captured, then exits)
//!   open-3d-viewer --bench FILE [--frames 300]
//!   open-3d-viewer FILE --save-as OUT.ifc   (convert without a window)
//!
//! In the OpenAEC SuperCloud (`managed`): OPENAEC_BEHEERD=1, OPENAEC_MAP (work folder),
//! OPENAEC_TAAL, OPENAEC_THEMA, OPENAEC_OPEN (file to open), and OPENAEC_MCP_POORT with
//! OPENAEC_MCP_GEHEIM (and optionally OPENAEC_MCP_ADRES) for the MCP endpoint over HTTP.

use crate::app::{self, App, Message};
use crate::viewport::camera::ViewPreset;
use crate::viewport::render::RenderMode;
use crate::{bench, explode, export, gpu, loaders, managed, shell, INTER_REGULAR, INTER_SEMIBOLD};
use openaec_ui::design::ThemeId;
use openaec_ui::Language;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Default)]
struct Args {
    file: Option<PathBuf>,
    theme: Option<ThemeId>,
    language: Option<Language>,
    /// For `--screenshot`: the page over the workspace and a schedule or sheet (by number) to show
    /// (Revit).
    page: shell::Page,
    schedule: Option<String>,
    sheet: Option<String>,
    /// For `--screenshot`: recent files, besides FILE.
    recent: Vec<PathBuf>,
    screenshot: Option<PathBuf>,
    window_screenshot: Option<PathBuf>,
    size: Option<(f32, f32)>,
    mode: Option<RenderMode>,
    view: Option<ViewPreset>,
    bench: Option<PathBuf>,
    ortho: bool,
    view_tab: bool,
    frames: Option<u32>,
    /// `--bench` keeps two frames in flight and checks the 120 fps criterion.
    throughput: bool,
    /// Do not take remote control commands (the MCP server).
    no_remote: bool,
    save_as: Option<PathBuf>,
    /// Exploded view for `--screenshot`: percent per axis, and the mode.
    explode: Option<[f32; 3]>,
    explode_mode: Option<explode::ExplodeMode>,
    /// The graphics backend and present mode of the window, overriding the default (`gpu`).
    gpu_backend: Option<String>,
    present_mode: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args::default();
    let mut it = std::env::args_os().skip(1);
    while let Some(a) = it.next() {
        let s = a.to_string_lossy().to_string();
        let mut value = || it.next().map(|v| v.to_string_lossy().to_string()).ok_or(format!("{s} needs a value"));
        match s.as_str() {
            "--theme" => args.theme = Some(shell::theme_from_arg(&value()?).ok_or("unknown theme")?),
            "--language" => {
                args.language = Some(match value()?.as_str() {
                    "en" => Language::English,
                    "nl" => Language::Dutch,
                    _ => return Err("--language en|nl".into()),
                })
            }
            "--page" => args.page = shell::Page::parse(&value()?).ok_or("--page workspace|file|settings|shortcuts")?,
            "--schedule" => args.schedule = Some(value()?),
            "--sheet" => args.sheet = Some(value()?),
            "--recent" => args.recent.push(PathBuf::from(value()?)),
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
            "--throughput" => args.throughput = true,
            "--view-tab" => args.view_tab = true,
            "--no-remote" => args.no_remote = true,
            "--gpu-backend" => args.gpu_backend = Some(gpu::parse_backend(&value()?)?),
            "--present-mode" => args.present_mode = Some(gpu::parse_present_mode(&value()?)?),
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
                println!("{}", include_str!("desktop.rs").lines().take_while(|l| l.starts_with("//!")).map(|l| l.trim_start_matches("//!")).collect::<Vec<_>>().join("\n"));
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
        bench::run(file, args.frames.unwrap_or(300), args.throughput)
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
    export::save(&model, out, true)?;
    println!("{} -> {} in {:.2} s", model.file_name(), out.display(), start.elapsed().as_secs_f32());
    Ok(())
}

fn initial_state(args: &Args) -> App {
    let mut app = App::default();
    if let Some(t) = args.theme {
        app.theme = shell::viewport_kind(t);
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

/// The application window: the viewer inside the OpenAEC Blueprint shell (`shell`), with the
/// preferences and own shortcuts in %APPDATA%\OpenAEC\Open 3D Viewer Studio (on Linux and macOS
/// under ~/.config, see `remote_link::settings_dir`).
fn run_app(args: Args) -> iced::Result {
    let dir = crate::remote_link::settings_dir();
    if let Some(dir) = &dir {
        let _ = std::fs::create_dir_all(dir);
    }
    // the MCP endpoint of the SuperCloud; its secret leaves the environment before any other
    // thread or child process (a file dialog) can read it
    let mcp_http = crate::mcp::HttpConfig::from_env(managed::managed());
    std::env::remove_var("OPENAEC_MCP_GEHEIM");
    gpu::configure(args.gpu_backend.clone(), args.present_mode.clone(), dir.as_deref());
    let mut app = initial_state(&args);
    app.initial_view = args.view;
    // remote control for AI agents (open-3d-viewer-mcp); O3V_REMOTE=0 or --no-remote switch it off
    app.remote.enabled = !args.no_remote && std::env::var("O3V_REMOTE").as_deref() != Ok("0");
    // the work folder, as an absolute path before the app moves into it; with the HTTP endpoint
    // on, the MCP tools reach only files in it, also outside managed mode
    managed::fix_work_folder(app.remote.enabled && matches!(mcp_http, Some(Ok(_))));
    app.pending_open = managed::start_file(args.file.clone());
    managed::enter_work_folder();
    #[cfg(unix)]
    stop_on_sigterm();
    match mcp_http {
        Some(Ok(config)) if app.remote.enabled => crate::remote::serve_mcp_http(config),
        Some(Ok(_)) => eprintln!("MCP over HTTP: off, as remote control is off"),
        Some(Err(e)) => eprintln!("MCP over HTTP: not started: {e}"),
        None => {}
    }
    let mut studio = shell::Studio::new(app);
    if let Some(dir) = dir {
        studio = studio.with_settings_in(dir);
    }
    studio.set_look(args.theme.or_else(managed::theme), args.language.or_else(managed::language));
    shell::run(studio, window_icon(), true)
}

/// How long a save under way may take to finish after SIGTERM: within Docker's 10 s before it
/// kills the process.
#[cfg(unix)]
const STOP_GRACE: std::time::Duration = std::time::Duration::from_secs(8);

/// SIGTERM (the SuperCloud ends the session, the desktop logs out; OpenAEC-Blueprint#61): a save
/// under way is finished first, for [`STOP_GRACE`] at most, else its temporary file is removed so
/// no half file stays in the work folder; then the viewer stops. That is all the unsaved work
/// there is: the viewer does not change models, and what it shows (camera, sections,
/// measurements) is not kept between sessions.
#[cfg(unix)]
fn stop_on_sigterm() {
    let mut signals = match signal_hook::iterator::Signals::new([signal_hook::consts::SIGTERM]) {
        Ok(signals) => signals,
        Err(e) => return eprintln!("SIGTERM: {e}"),
    };
    let stop = move || {
        if signals.forever().next().is_none() {
            return;
        }
        let until = std::time::Instant::now() + STOP_GRACE;
        let saving = !export::writing().is_empty();
        while !export::writing().is_empty() && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let unfinished = export::writing();
        for part in &unfinished {
            let _ = std::fs::remove_file(part);
            eprintln!("SIGTERM: a save was not finished; removed {}", part.display());
        }
        if saving && unfinished.is_empty() {
            // the answer to the save_file call that waited for it goes out
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        std::process::exit(if unfinished.is_empty() { 0 } else { 1 });
    };
    if let Err(e) = std::thread::Builder::new().name("sigterm".into()).spawn(stop) {
        eprintln!("SIGTERM: {e}");
    }
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

/// Renders one frame of the whole window headlessly (wgpu offscreen) and writes a PNG.
fn screenshot(args: &Args, out: &std::path::Path) -> Result<(), String> {
    let (w, h) = args.size.unwrap_or((1440.0, 900.0));
    let mut studio = shell::Studio::new(initial_state(args));
    studio.set_look(args.theme, args.language);
    studio.show(args.page);
    for path in args.recent.iter().rev().chain(&args.file) {
        studio.remember(path.clone());
    }
    let viewport = studio.viewport_in(w, h);
    let app = &mut studio.app;
    app.viewport_size = viewport;
    if let Some(file) = &args.file {
        let model = loaders::load(file)?;
        eprintln!(
            "loaded {}: {} layers, {} triangles, {} points in {:.2} s",
            model.file_name(),
            model.layers.len(),
            model.triangle_count(),
            model.point_count(),
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
        if let Some(name) = &args.schedule {
            // the window computes the schedules after the model shows; here, before the frame
            let model = app.model.clone().ok_or("no model")?;
            let schedules = model.project.schedules.take().map(|work| work()).unwrap_or_default();
            let _ = app.update(Message::Schedules(model.id, schedules));
            let i = app.schedules.iter().position(|s| &s.name == name).ok_or(format!("no schedule {name} in the file"))?;
            let _ = app.update(Message::OpenSchedule(i));
        }
        if let Some(number) = &args.sheet {
            let model = app.model.clone().ok_or("no model")?;
            let i = model.project.sheets.iter().position(|s| &s.number == number).ok_or(format!("no sheet {number} in the file"))?;
            let _ = app.update(Message::OpenSheet(i));
        }
    }
    studio.snapshot(iced::Size::new(w, h), out)?;
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
