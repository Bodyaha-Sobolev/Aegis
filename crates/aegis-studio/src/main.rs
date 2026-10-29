#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]
mod disassembly;
mod document;
mod editor_project;
mod highlighting;
mod inspector;
mod navigation;
mod permissions;
mod runner;

use aegis_lang::{Diagnostic, FunctionInfo, codec, storage};
use document::Document;
use eframe::egui::{self, Color32, FontId, Id, Key, Modifiers, RichText, Vec2};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

const SAMPLE: &str = include_str!("../../../examples/factorial.ae");
const OUTPUT_LIMIT: usize = 1024 * 1024;
const ACCENT: Color32 = Color32::from_rgb(116, 218, 193);
const MUTED: Color32 = Color32::from_rgb(137, 151, 171);

enum Event {
    Analyzed(
        u64,
        u64,
        editor_project::Stamp,
        Result<Vec<FunctionInfo>, Diagnostic>,
    ),
    Built {
        stamp: editor_project::Stamp,
        id: u64,
        revision: u64,
        path: PathBuf,
        run: bool,
        result: Result<(usize, usize, Duration), String>,
    },
    Saved {
        id: u64,
        revision: u64,
        path: PathBuf,
        protected: bool,
        result: Result<(), String>,
    },
    Opened(PathBuf, Result<String, String>),
    Output(u64, Vec<u8>),
    Finished(u64, Result<(i32, Duration), String>),
    Inspected(PathBuf, Result<(aegis_lang::pe::Image, Vec<u8>), String>),
}
enum ProtectedAction {
    Open(PathBuf, Zeroizing<Vec<u8>>),
    Save {
        id: u64,
        revision: u64,
        path: PathBuf,
        text: Zeroizing<String>,
    },
}
struct Unlock {
    action: ProtectedAction,
    key_path: Option<PathBuf>,
    dialect: String,
    error: Option<String>,
}

struct Studio {
    docs: Vec<Document>,
    active: usize,
    next_id: u64,
    sender: SyncSender<Event>,
    receiver: Receiver<Event>,
    busy: bool,
    analyzing: bool,
    output: Vec<u8>,
    status: String,
    run: Option<(u64, Arc<AtomicBool>)>,
    next_run: u64,
    scratch: Option<tempfile::TempDir>,
    workspace: Option<PathBuf>,
    files: Vec<PathBuf>,
    close_doc: Option<u64>,
    close_after_save: Option<u64>,
    quit: bool,
    allow_exit: bool,
    unlock: Option<Unlock>,
    find_open: bool,
    find: String,
    replacement: String,
    find_from: usize,
    navigator: Option<bool>, // true: project files; false: source location
    navigation_query: String,
    navigation_error: Option<String>,
    navigation_focus: bool,
    navigation_selected: usize,
    smoke_frames: Option<u32>,
    smoke_exports: bool,
    smoke_disasm: bool,
    licenses_open: bool,
    screenshot: Option<PathBuf>,
    inspector: Option<inspector::Inspector>,
    inspecting: bool,
    project_entry: Option<PathBuf>,
    analysis_stamp: Option<editor_project::Stamp>,
    permission_ledger: permissions::PermissionLedger,
    active_run_permission: Option<u64>,
}
impl Default for Studio {
    fn default() -> Self {
        let (sender, receiver) = mpsc::sync_channel(64);
        Self {
            docs: vec![Document::new(1, None, SAMPLE.into(), false)],
            active: 0,
            next_id: 2,
            sender,
            receiver,
            busy: false,
            analyzing: false,
            output: Vec::new(),
            status: "Ready".into(),
            run: None,
            next_run: 0,
            scratch: tempfile::tempdir().ok(),
            workspace: None,
            files: Vec::new(),
            close_doc: None,
            close_after_save: None,
            quit: false,
            allow_exit: false,
            unlock: None,
            find_open: false,
            find: String::new(),
            replacement: String::new(),
            find_from: 0,
            navigator: None,
            navigation_query: String::new(),
            navigation_error: None,
            navigation_focus: false,
            navigation_selected: 0,
            smoke_frames: None,
            smoke_exports: false,
            smoke_disasm: false,
            licenses_open: false,
            screenshot: None,
            inspector: None,
            inspecting: false,
            project_entry: None,
            analysis_stamp: None,
            permission_ledger: permissions::PermissionLedger::default(),
            active_run_permission: None,
        }
    }
}
impl Studio {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(19, 25, 34);
        visuals.window_fill = Color32::from_rgb(25, 32, 43);
        visuals.extreme_bg_color = Color32::from_rgb(14, 19, 27);
        visuals.selection.bg_fill = Color32::from_rgb(48, 88, 91);
        visuals.selection.stroke.color = Color32::WHITE;
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(34, 43, 56);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(45, 62, 75);
        cc.egui_ctx.set_visuals(visuals);
        cc.egui_ctx.style_mut(|s| {
            s.spacing.item_spacing = Vec2::new(10.0, 8.0);
            s.spacing.button_padding = Vec2::new(13.0, 7.0);
            s.text_styles
                .insert(egui::TextStyle::Monospace, FontId::monospace(15.0));
        });
        let mut app = Self::default();
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        if args
            .first()
            .is_some_and(|s| s == "--smoke-test" || s == "--smoke-inspect")
        {
            app.smoke_frames = Some(0);
            app.screenshot = args.get(1).map(PathBuf::from);
            if let Some(path) = args.get(2) {
                if args[0] == "--smoke-inspect" {
                    app.smoke_exports = args.get(3).is_some_and(|s| s == "exports");
                    app.smoke_disasm = args.get(3).is_some_and(|s| s == "disasm");
                    app.inspect_binary(PathBuf::from(path), cc.egui_ctx.clone());
                } else {
                    app.open_path(PathBuf::from(path));
                }
            }
        } else if let Some(path) = args.first() {
            app.open_path(PathBuf::from(path));
        }
        app
    }
    fn log(&mut self, text: impl AsRef<str>) {
        let bytes = text.as_ref().as_bytes();
        let n = bytes
            .len()
            .min(OUTPUT_LIMIT.saturating_sub(self.output.len()));
        self.output.extend_from_slice(&bytes[..n]);
    }
    fn inspect_binary(&mut self, path: PathBuf, ctx: egui::Context) {
        if self.inspecting {
            return;
        }
        self.inspecting = true;
        self.status = "Inspecting binary…".into();
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = storage::read_bounded(&path, aegis_lang::pe::MAX_FILE)
                .map_err(|e| e.to_string())
                .and_then(|bytes| {
                    aegis_lang::pe::inspect(&bytes)
                        .map(|image| (image, bytes))
                        .map_err(|e| e.to_string())
                });
            let _ = sender.send(Event::Inspected(path, result));
            ctx.request_repaint();
        });
    }
    fn fail(&mut self, error: impl AsRef<str>) {
        self.status = "Operation failed".into();
        self.log(format!("ERROR: {}\n", error.as_ref()));
    }
    fn new_doc(&mut self) {
        let id = self.next_id;
        self.next_id += 1;
        self.docs.push(Document::new(
            id,
            None,
            "fn main() {\n    return 0;\n}\n".into(),
            false,
        ));
        self.active = self.docs.len() - 1;
    }
    fn remember_entry(&mut self, path: &Path) {
        if path.file_name().is_some_and(|name| name == "main.ae")
            && self
                .project_entry
                .as_ref()
                .is_none_or(|entry| entry.parent().is_none_or(|root| !path.starts_with(root)))
        {
            self.project_entry = Some(path.to_owned());
        }
    }
    fn add_opened(&mut self, path: PathBuf, text: String, protected: bool) {
        self.remember_entry(&path);
        let id = self.next_id;
        self.next_id += 1;
        self.docs
            .push(Document::new(id, Some(path), text, protected));
        self.active = self.docs.len() - 1;
        self.status = "Source opened".into();
    }
    fn open_dialog(&mut self) {
        if self.busy || self.unlock.is_some() {
            return;
        }
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("AEGIS source", &["ae", "aeg"])
            .pick_file()
        {
            self.open_path(path);
        }
    }
    fn open_path(&mut self, path: PathBuf) {
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        if let Some(index) = self
            .docs
            .iter()
            .position(|d| d.path.as_ref() == Some(&path))
        {
            self.active = index;
            self.remember_entry(&path);
            return;
        }
        match storage::read_bounded(&path, aegis_lang::MAX_SOURCE + codec::OVERHEAD) {
            Err(e) => self.fail(e.to_string()),
            Ok(bytes) if codec::is_protected(&bytes) => {
                self.unlock = Some(Unlock {
                    action: ProtectedAction::Open(path, Zeroizing::new(bytes)),
                    key_path: None,
                    dialect: String::new(),
                    error: None,
                })
            }
            Ok(bytes) if bytes.starts_with(b"AEG0") => {
                self.fail("Legacy AEG0 must first be decoded using the legacy prototype.")
            }
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) if text.len() <= aegis_lang::MAX_SOURCE && !text.contains('\0') => {
                    self.add_opened(path, text, false)
                }
                _ => self.fail("Expected UTF-8 text without NUL bytes, at most 1 MiB."),
            },
        }
    }
    fn save(&mut self, id: u64, save_as: bool, protect: bool) -> bool {
        if self.busy || self.unlock.is_some() {
            return false;
        }
        let Some(doc) = self.docs.iter().find(|d| d.id == id) else {
            return false;
        };
        let protected = protect || doc.protected;
        let path = if save_as || doc.path.is_none() || protect {
            let extension = if protected { "aeg" } else { "ae" };
            let name = Path::new(&doc.title()).with_extension(extension);
            let Some(path) = rfd::FileDialog::new()
                .set_file_name(name.to_string_lossy())
                .add_filter("AEGIS source", &[extension])
                .save_file()
            else {
                return false;
            };
            path
        } else if let Some(path) = &doc.path {
            path.clone()
        } else {
            return false;
        };
        // Never silently overwrite another open buffer on Save As.
        let normalized = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if self
            .docs
            .iter()
            .any(|d| d.id != id && d.path.as_ref() == Some(&normalized))
        {
            self.fail("That file is already open in another tab.");
            return false;
        }
        let revision = doc.revision;
        let text = Zeroizing::new(doc.text.clone());
        if protected {
            self.unlock = Some(Unlock {
                action: ProtectedAction::Save {
                    id,
                    revision,
                    path,
                    text,
                },
                key_path: None,
                dialect: String::new(),
                error: None,
            });
        } else {
            let events = self.sender.clone();
            self.busy = true;
            self.status = "Saving source…".into();
            std::thread::spawn(move || {
                let result =
                    storage::atomic_write(&path, text.as_bytes()).map_err(|e| e.to_string());
                let _ = events.send(Event::Saved {
                    id,
                    revision,
                    path,
                    protected: false,
                    result,
                });
            });
        }
        true
    }
    fn open_location(&mut self, path: Option<PathBuf>, offset: usize) {
        if let Some(path) = path {
            let path = path.canonicalize().unwrap_or(path);
            self.open_path(path.clone());
            if self.docs.get(self.active).and_then(|doc| doc.path.as_ref()) != Some(&path) {
                return;
            }
        }
        if let Some(doc) = self.docs.get_mut(self.active) {
            doc.jump_to = Some(offset);
        }
    }
    fn build(&mut self, run: bool) {
        if self.busy || self.run.is_some() || self.unlock.is_some() {
            return;
        }
        let Some(doc) = self.docs.get(self.active) else {
            return;
        };
        let snapshot =
            match editor_project::Snapshot::new(&self.docs, self.active, &self.project_entry) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    self.fail(error);
                    return;
                }
            };
        let stamp = editor_project::Stamp::new(&self.docs, &self.project_entry);
        let path = if let Some(path) = &snapshot.entry {
            let mut name = path.as_os_str().to_os_string();
            name.push(".exe");
            PathBuf::from(name)
        } else if let Some(scratch) = &self.scratch {
            scratch.path().join(format!("untitled-{}.exe", doc.id))
        } else {
            self.fail("Cannot create temporary build directory");
            return;
        };
        let id = doc.id;
        let revision = doc.revision;
        let events = self.sender.clone();
        self.busy = true;
        self.output.clear();
        self.status = "Building native x64 executable…".into();
        self.log("AEGIS / native Windows x64\n");
        std::thread::spawn(move || {
            let started = Instant::now();
            let result = snapshot
                .compile()
                .map_err(|e| e.to_string())
                .and_then(|build| {
                    storage::atomic_write(&path, &build.executable).map_err(|e| e.to_string())?;
                    Ok((build.functions.len(), build.code_bytes, started.elapsed()))
                });
            let _ = events.send(Event::Built {
                stamp,
                id,
                revision,
                path,
                run,
                result,
            });
        });
    }
    fn remove_doc(&mut self, id: u64, ctx: &egui::Context) {
        ctx.data_mut(|data| data.remove::<egui::text_edit::TextEditState>(Id::new(("source", id))));
        if let Some(at) = self.docs.iter().position(|d| d.id == id) {
            self.docs.remove(at);
            self.active = self.active.min(self.docs.len().saturating_sub(1));
        }
        if self.docs.is_empty() && !self.quit {
            self.new_doc();
        }
    }
    fn request_close(&mut self, id: u64, ctx: &egui::Context) {
        if self.busy {
            self.status = "Wait for the current operation before closing a tab.".into();
            return;
        }
        if self.docs.iter().any(|d| d.id == id && d.dirty()) {
            self.close_doc = Some(id);
        } else {
            self.remove_doc(id, ctx);
        }
    }
    fn receive(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                Event::Inspected(path, result) => {
                    self.inspecting = false;
                    match result {
                        Ok((image, bytes)) => {
                            let mut inspector = inspector::Inspector::new(path, image, bytes);
                            if self.smoke_exports {
                                inspector.select_exports();
                            }
                            if self.smoke_disasm {
                                inspector.select_disassembly();
                            }
                            self.inspector = Some(inspector);
                            self.status = "Binary inspected · file not executed".into();
                        }
                        Err(error) => self.fail(error),
                    }
                }
                Event::Analyzed(id, revision, stamp, result) => {
                    self.analyzing = false;
                    if stamp != editor_project::Stamp::new(&self.docs, &self.project_entry) {
                        if let Some(doc) = self.docs.get_mut(self.active) {
                            doc.analyze_after = Some(Instant::now());
                        }
                        continue;
                    }
                    if let Some(doc) = self
                        .docs
                        .iter_mut()
                        .find(|d| d.id == id && d.revision == revision)
                    {
                        match result {
                            Ok(functions) => {
                                doc.functions = functions;
                                doc.diagnostic = None;
                            }
                            Err(error) => {
                                doc.functions.clear();
                                doc.diagnostic = Some(error);
                            }
                        }
                    }
                }
                Event::Built {
                    stamp,
                    id,
                    revision,
                    path,
                    run,
                    result,
                } => {
                    self.busy = false;
                    match result {
                        Err(e) => self.fail(e),
                        Ok((functions, bytes, time)) => {
                            self.status =
                                format!("Build succeeded · {:.1} ms", time.as_secs_f64() * 1000.0);
                            self.log(format!("Built {functions} functions / {bytes} bytes of native code in {:.1} ms\n{}\n",time.as_secs_f64()*1000.0,path.display()));
                            if self
                                .docs
                                .iter()
                                .any(|d| d.id == id && d.revision != revision)
                                || stamp
                                    != editor_project::Stamp::new(&self.docs, &self.project_entry)
                            {
                                self.log("The editor changed during the build; this executable uses the earlier snapshot.\n");
                            }
                            if run {
                                self.next_run += 1;
                                let run_id = self.next_run;
                                let stop = Arc::new(AtomicBool::new(false));
                                self.run = Some((run_id, stop.clone()));
                                self.active_run_permission = Some(self.permission_ledger.request(
                                    "studio",
                                    permissions::Capability::RunOwnedChild,
                                    &path.display().to_string(),
                                    "local-build-output",
                                    permissions::Decision::Allow,
                                    "user requested Build and run",
                                ));
                                self.log("\nRunning…\n");
                                self.status = "Program running".into();
                                runner::run(path, run_id, stop, self.sender.clone());
                            }
                        }
                    }
                }
                Event::Saved {
                    id,
                    revision,
                    path,
                    protected,
                    result,
                } => {
                    self.busy = false;
                    match result {
                        Err(e) => {
                            self.close_after_save = None;
                            self.fail(e);
                        }
                        Ok(()) => {
                            if let Some(doc) = self.docs.iter_mut().find(|d| d.id == id) {
                                let was_entry = self
                                    .project_entry
                                    .as_ref()
                                    .is_some_and(|entry| doc.path.as_ref() == Some(entry));
                                let path = path.canonicalize().unwrap_or(path);
                                if was_entry {
                                    self.project_entry = Some(path.clone());
                                }
                                doc.saved(revision, path, protected);
                            }
                            self.status = "Source saved".into();
                            if self.close_after_save.take() == Some(id) {
                                if self.docs.iter().any(|d| d.id == id && d.dirty()) {
                                    self.close_doc = Some(id);
                                } else {
                                    self.remove_doc(id, ctx);
                                }
                            }
                        }
                    }
                }
                Event::Opened(path, result) => {
                    self.busy = false;
                    match result {
                        Ok(text) => self.add_opened(path, text, true),
                        Err(e) => self.fail(e),
                    }
                }
                Event::Output(id, bytes) => {
                    if self.run.as_ref().is_some_and(|(run, _)| *run == id) {
                        let n = bytes
                            .len()
                            .min(OUTPUT_LIMIT.saturating_sub(self.output.len()));
                        self.output.extend_from_slice(&bytes[..n]);
                    }
                }
                Event::Finished(id, result) => {
                    if self.run.as_ref().is_some_and(|(run, _)| *run == id) {
                        self.run = None;
                        if let Some(request_id) = self.active_run_permission.take() {
                            let result = match &result {
                                Ok((code, _)) => format!("exit:{code}"),
                                Err(_) => String::from("runner-error"),
                            };
                            self.permission_ledger.finish(request_id, &result, Duration::ZERO);
                        }
                        match result {
                            Ok((code, time)) => {
                                self.status = format!("Program exited with code {code}");
                                self.log(format!(
                                    "\n[Exit {code} · {:.2} s]\n",
                                    time.as_secs_f64()
                                ));
                            }
                            Err(e) => self.fail(e),
                        }
                    }
                }
            }
        }
    }
    fn analyze(&mut self) {
        let stamp = editor_project::Stamp::new(&self.docs, &self.project_entry);
        if self.analysis_stamp.as_ref() != Some(&stamp) {
            for doc in &mut self.docs {
                if doc.analyze_after.is_none() {
                    doc.analyze_after = Some(Instant::now() + Duration::from_millis(400));
                }
            }
            self.analysis_stamp = Some(stamp.clone());
        }
        if self.analyzing {
            return;
        }
        let Some(doc) = self.docs.get_mut(self.active) else {
            return;
        };
        if doc.analyze_after.is_none_or(|at| Instant::now() < at) {
            return;
        }
        doc.analyze_after = None;
        self.analyzing = true;
        let id = doc.id;
        let revision = doc.revision;
        let snapshot =
            match editor_project::Snapshot::new(&self.docs, self.active, &self.project_entry) {
                Ok(snapshot) => snapshot,
                Err(message) => {
                    self.analyzing = false;
                    if let Some(doc) = self.docs.get_mut(self.active) {
                        doc.diagnostic = Some(Diagnostic {
                            message,
                            path: None,
                            span: 0..0,
                            line: 1,
                            column: 1,
                        });
                    }
                    return;
                }
            };
        let events = self.sender.clone();
        std::thread::spawn(move || {
            let result = snapshot.compile().map(|build| build.functions);
            let _ = events.send(Event::Analyzed(id, revision, stamp, result));
        });
    }
    fn find_next(&mut self) {
        if self.find.is_empty() {
            return;
        }
        let Some(doc) = self.docs.get_mut(self.active) else {
            return;
        };
        let start = self.find_from.min(doc.text.len());
        let start = if doc.text.is_char_boundary(start) {
            start
        } else {
            0
        };
        let found = doc.text[start..]
            .find(&self.find)
            .map(|n| n + start)
            .or_else(|| doc.text.find(&self.find));
        if let Some(at) = found {
            doc.jump_to = Some(at);
            self.find_from = at + self.find.len();
            self.status = "Match found".into();
        } else {
            self.status = "No matches".into();
        }
    }
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.unlock.is_some() || self.close_doc.is_some() {
            return;
        }
        let pressed = |key, mods| ctx.input_mut(|i| i.consume_key(mods, key));
        if pressed(Key::P, Modifiers::CTRL) {
            self.open_navigator(true);
        }
        if pressed(Key::G, Modifiers::CTRL) {
            self.open_navigator(false);
        }
        if self.navigator.is_some() {
            return;
        }
        if pressed(Key::N, Modifiers::CTRL) {
            self.new_doc();
        }
        if pressed(Key::O, Modifiers::CTRL) {
            self.open_dialog();
        }
        if pressed(Key::S, Modifiers::CTRL | Modifiers::SHIFT) {
            if let Some(doc) = self.docs.get(self.active) {
                self.save(doc.id, true, false);
            }
        } else if pressed(Key::S, Modifiers::CTRL)
            && let Some(doc) = self.docs.get(self.active)
        {
            self.save(doc.id, false, false);
        }
        if pressed(Key::W, Modifiers::CTRL)
            && let Some(doc) = self.docs.get(self.active)
        {
            self.request_close(doc.id, ctx);
        }
        if pressed(Key::F, Modifiers::CTRL) {
            self.find_open = !self.find_open;
            self.find_from = 0;
        }
        if pressed(Key::F5, Modifiers::NONE) {
            self.build(true);
        }
        if pressed(Key::F6, Modifiers::NONE) {
            self.build(false);
        }
    }
    fn show(&mut self, ctx: &egui::Context) {
        // Test-only command-line mode exercises an actual native GPU window and
        // optionally captures that window's own rendered frame, without input.
        if let Some(frames) = &mut self.smoke_frames {
            *frames += 1;
            if *frames == 16 && self.screenshot.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            if *frames >= 40 {
                self.allow_exit = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ctx.request_repaint_after(Duration::from_millis(40));
        }
        for event in ctx.input(|i| i.events.clone()) {
            if let egui::Event::Screenshot { image, .. } = event
                && let Some(path) = self.screenshot.take()
            {
                let rgba: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                if let Err(e) = image::save_buffer(
                    path,
                    &rgba,
                    image.size[0] as u32,
                    image.size[1] as u32,
                    image::ColorType::Rgba8,
                ) {
                    self.fail(e.to_string());
                }
            }
        }
        self.receive(ctx);
        self.analyze();
        self.shortcuts(ctx);
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_exit {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.quit = true;
        }
        if self.quit && self.close_doc.is_none() && self.unlock.is_none() && !self.busy {
            if let Some(doc) = self.docs.iter().find(|d| d.dirty()) {
                self.close_doc = Some(doc.id);
            } else if let Some((_, stop)) = &self.run {
                stop.store(true, Ordering::Relaxed);
            } else {
                self.allow_exit = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        let title = self.docs.get(self.active).map_or_else(
            || "Aegis Studio".into(),
            |d| {
                format!(
                    "{}{} — Aegis Studio",
                    if d.dirty() { "• " } else { "" },
                    d.title()
                )
            },
        );
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        egui::TopBottomPanel::top("toolbar")
            .exact_height(62.0)
            .show(ctx, |ui| {
                ui.add_space(9.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("A E G I S").size(22.0).strong().color(ACCENT));
                    ui.label(RichText::new("STUDIO").size(11.0).color(MUTED));
                    ui.separator();
                    if ui.button("New").on_hover_text("Ctrl+N").clicked() {
                        self.new_doc();
                    }
                    if ui
                        .add_enabled(!self.busy, egui::Button::new("Open"))
                        .on_hover_text("Ctrl+O")
                        .clicked()
                    {
                        self.open_dialog();
                    }
                    let id = self.docs.get(self.active).map(|d| d.id);
                    if ui
                        .add_enabled(!self.busy, egui::Button::new("Save"))
                        .on_hover_text("Ctrl+S")
                        .clicked()
                        && let Some(id) = id
                    {
                        self.save(id, false, false);
                    }
                    ui.menu_button("File", |ui| {
                        if ui.button("Component licenses").clicked() {
                            self.licenses_open = true;
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                !self.inspecting,
                                egui::Button::new("Inspect EXE / DLL / SYS…"),
                            )
                            .clicked()
                        {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("Windows binary", &["exe", "dll", "sys"])
                                .pick_file()
                            {
                                self.inspect_binary(path, ctx.clone());
                            }
                            ui.close();
                        }
                        if ui.button("Save as…   Ctrl+Shift+S").clicked() {
                            if let Some(id) = id {
                                self.save(id, true, false);
                            }
                            ui.close();
                        }
                        if ui.button("Protect source as…").clicked() {
                            if let Some(id) = id {
                                self.save(id, true, true);
                            }
                            ui.close();
                        }
                        if ui.button("Use active file as project entry").clicked() {
                            self.project_entry =
                                self.docs.get(self.active).and_then(|doc| doc.path.clone());
                            self.analysis_stamp = None;
                            ui.close();
                        }
                        if ui.button("Clear project entry").clicked() {
                            self.project_entry = None;
                            self.analysis_stamp = None;
                            ui.close();
                        }
                        if ui.button("Open project folder…").clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                self.files = project_files(&path);
                                let entry = path.join("main.ae");
                                self.project_entry = entry
                                    .is_file()
                                    .then(|| entry.canonicalize().unwrap_or(entry));
                                self.workspace = Some(path);
                            }
                            ui.close();
                        }
                        if ui.button("Close tab   Ctrl+W").clicked() {
                            if let Some(id) = id {
                                self.request_close(id, ctx);
                            }
                            ui.close();
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some((_, stop)) = &self.run {
                            if ui
                                .button(
                                    RichText::new("Stop").color(Color32::from_rgb(249, 131, 142)),
                                )
                                .clicked()
                            {
                                stop.store(true, Ordering::Relaxed);
                            }
                        } else if ui
                            .add_enabled(
                                !self.busy,
                                egui::Button::new(
                                    RichText::new("Run  F5").color(Color32::from_rgb(15, 30, 29)),
                                )
                                .fill(ACCENT),
                            )
                            .clicked()
                        {
                            self.build(true);
                        }
                        if ui
                            .add_enabled(
                                !self.busy && self.run.is_none(),
                                egui::Button::new("Build  F6"),
                            )
                            .clicked()
                        {
                            self.build(false);
                        }
                    });
                });
            });
        egui::TopBottomPanel::bottom("status")
            .exact_height(30.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new(&self.status).size(12.0).color(MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new("UTF-8  ·  Windows x64")
                                .size(12.0)
                                .color(MUTED),
                        );
                        if let Some(doc) = self.docs.get(self.active) {
                            ui.label(
                                RichText::new(format!("Ln {}, Col {}", doc.cursor.0, doc.cursor.1))
                                    .size(12.0)
                                    .color(MUTED),
                            );
                            if doc.protected {
                                ui.label(
                                    RichText::new("Protected source").size(12.0).color(ACCENT),
                                );
                            }
                        }
                    });
                });
            });
        self.sidebar(ctx);
        egui::TopBottomPanel::bottom("output").resizable(true).default_height(200.0).min_height(90.0).show(ctx,|ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("OUTPUT").small().strong().color(MUTED));
                if ui.small_button("Clear").clicked() { self.output.clear(); }
                if self.busy || self.analyzing { ui.spinner(); }
                if self.output.len()>=OUTPUT_LIMIT { ui.label(RichText::new("Output limit reached (1 MiB)").small().color(Color32::YELLOW)); }
            });
            egui::ScrollArea::both().stick_to_bottom(true).show(ui,|ui| {
                if self.output.is_empty() { ui.label(RichText::new("Build and run output appears here. Source is compiled directly to native machine code.").color(MUTED)); }
                else { ui.add(egui::Label::new(RichText::new(String::from_utf8_lossy(&self.output)).monospace()).selectable(true)); }
            });
        });
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.style()).fill(Color32::from_rgb(14, 19, 27)))
            .show(ctx, |ui| {
                let mut close = None;
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (i, doc) in self.docs.iter().enumerate() {
                            let label = format!(
                                "{}{}{}",
                                if doc.protected { "◈ " } else { "" },
                                doc.title(),
                                if doc.dirty() { " •" } else { "" }
                            );
                            if ui
                                .selectable_label(i == self.active, label)
                                .on_hover_text(doc.path.as_ref().map_or_else(
                                    || "Unsaved buffer".into(),
                                    |p| p.display().to_string(),
                                ))
                                .clicked()
                            {
                                self.active = i;
                                self.find_from = 0;
                            }
                            if ui.small_button("×").on_hover_text("Close tab").clicked() {
                                close = Some(doc.id);
                            }
                        }
                        if ui.small_button("+").clicked() {
                            self.new_doc();
                        }
                    })
                });
                if let Some(id) = close {
                    self.request_close(id, ctx);
                }
                ui.separator();
                self.find_bar(ui);
                self.editor(ui);
            });
        self.close_modal(ctx);
        self.unlock_modal(ctx);
        if let Some(inspector) = &mut self.inspector
            && !inspector.show(ctx)
        {
            self.inspector = None;
        }
        self.navigation_modal(ctx);
        if self.busy
            || self.inspecting
            || self.analyzing
            || self.run.is_some()
            || self.docs.iter().any(|d| d.analyze_after.is_some())
        {
            ctx.request_repaint_after(Duration::from_millis(35));
        }
    }
    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("explorer").resizable(true).default_width(220.0).width_range(160.0..=420.0).show(ctx,|ui| {
            ui.add_space(12.0); ui.label(RichText::new("WORKSPACE").small().strong().color(MUTED));
            if let Some(root)=&self.workspace { ui.label(RichText::new(root.file_name().unwrap_or_default().to_string_lossy()).strong()); }
            else { ui.label(RichText::new("Open a project folder\nfrom the File menu.").small().color(MUTED)); }
            ui.horizontal(|ui| {
                if ui.add_enabled(self.workspace.is_some(), egui::Button::new("Refresh")).clicked()
                    && let Some(root) = &self.workspace {
                        self.files = project_files(root); self.analysis_stamp = None;
                        for doc in &mut self.docs { doc.analyze_after = Some(Instant::now()); }
                    }
                if ui.button("Find file").on_hover_text("Ctrl+P").clicked() { self.open_navigator(true); }
            });
            let mut open=None;
            egui::ScrollArea::vertical().max_height(220.0).show(ui,|ui| {
                for path in &self.files {
                    let title=path.strip_prefix(self.workspace.as_deref().unwrap_or(Path::new(""))).unwrap_or(path).display().to_string();
                    if ui.button(title).clicked() { open=Some(path.clone()); }
                }
            });
            if let Some(path)=open { self.open_path(path); }
            ui.add_space(18.0); ui.separator(); ui.add_space(8.0);
            ui.label(RichText::new("SYMBOLS").small().strong().color(MUTED));
            if let Some(entry) = &self.project_entry { ui.label(RichText::new(format!("Entry: {}", entry.file_name().unwrap_or_default().to_string_lossy())).small().color(MUTED)).on_hover_text(entry.display().to_string()); }
            let mut location = None;
            egui::ScrollArea::vertical().id_salt("project-symbols").max_height((ui.available_height()-145.0).max(80.0)).show(ui, |ui| {
            if let Some(doc)=self.docs.get_mut(self.active) {
                for function in &doc.functions {
                    if ui.selectable_label(false,RichText::new(format!("ƒ  {}",function.signature)).monospace()).clicked() { location=Some((function.path.clone(),function.span.start)); }
                }
                if let Some(error)=&doc.diagnostic {
                    ui.add_space(14.0); ui.label(RichText::new("PROBLEM").small().color(Color32::from_rgb(249,131,142)));
                    if let Some(path) = &error.path { ui.label(RichText::new(path.display().to_string()).small()); }
                    if ui.link(format!("{}:{}  {}",error.line,error.column,error.message)).clicked() { location=Some((error.path.clone(),error.span.start)); }
                }
            }
            });
            if let Some((path, offset)) = location { self.open_location(path, offset); }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT),|ui| {
                ui.label(RichText::new(concat!("AEGIS ", env!("CARGO_PKG_VERSION"), "\nNative compiler + editor\n\nCtrl+P  Find file\nCtrl+G  Go to line\nCtrl+F  Find\nCtrl+S  Save\nF6  Build\nF5  Build and run")).small().color(MUTED));
            });
        });
    }
    fn open_navigator(&mut self, files: bool) {
        self.navigator = Some(files);
        self.navigation_query.clear();
        self.navigation_error = None;
        self.navigation_focus = true;
        self.navigation_selected = 0;
    }
    fn navigation_modal(&mut self, ctx: &egui::Context) {
        let Some(files) = self.navigator else {
            return;
        };
        let mut selected = None;
        let mut go = false;
        let response = egui::Modal::new(Id::new("navigation")).show(ctx, |ui| {
            ui.set_min_width(450.0);
            ui.heading(if files {
                "Find project file"
            } else {
                "Go to line"
            });
            let moved = files
                && ui.input_mut(|i| {
                    let down = i.consume_key(Modifiers::NONE, Key::ArrowDown);
                    let up = i.consume_key(Modifiers::NONE, Key::ArrowUp);
                    if down {
                        self.navigation_selected = self.navigation_selected.saturating_add(1);
                    }
                    if up {
                        self.navigation_selected = self.navigation_selected.saturating_sub(1);
                    }
                    down || up
                });
            let enter = ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.navigation_query)
                    .desired_width(450.0)
                    .hint_text(if files {
                        "Filter by path or filename"
                    } else {
                        "line:column, for example 12:5"
                    }),
            );
            if self.navigation_focus {
                field.request_focus();
                self.navigation_focus = false;
            }
            if field.changed() {
                self.navigation_error = None;
                self.navigation_selected = 0;
            }
            if files {
                let root = self.workspace.as_deref().unwrap_or(Path::new(""));
                let candidates: Vec<_> = self
                    .files
                    .iter()
                    .filter(|path| {
                        navigation::matches(
                            &path.strip_prefix(root).unwrap_or(path).to_string_lossy(),
                            &self.navigation_query,
                        )
                    })
                    .collect();
                self.navigation_selected = self
                    .navigation_selected
                    .min(candidates.len().saturating_sub(1));
                ui.label(format!(
                    "{} matching files · ↑ ↓ select · Enter opens",
                    candidates.len()
                ));
                if self.workspace.is_none() {
                    ui.label("Open a project folder from the File menu first.");
                }
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        for (index, path) in candidates.iter().enumerate() {
                            let row = ui.selectable_label(
                                index == self.navigation_selected,
                                path.strip_prefix(root)
                                    .unwrap_or(path)
                                    .display()
                                    .to_string(),
                            );
                            if moved && index == self.navigation_selected {
                                row.scroll_to_me(Some(egui::Align::Center));
                            }
                            if row.clicked() {
                                selected = Some((*path).clone());
                            }
                        }
                    });
                if enter {
                    selected = candidates
                        .get(self.navigation_selected)
                        .map(|p| (*p).clone());
                }
            } else {
                go = enter || ui.button("Go").clicked();
            }
            if let Some(error) = &self.navigation_error {
                ui.colored_label(Color32::LIGHT_RED, error);
            }
        });
        if let Some(path) = selected {
            self.navigator = None;
            self.open_path(path);
        } else if go {
            if let Some(doc) = self.docs.get_mut(self.active) {
                match navigation::location(&doc.text, &self.navigation_query) {
                    Ok(at) => {
                        doc.jump_to = Some(at);
                        self.navigator = None;
                    }
                    Err(error) => self.navigation_error = Some(error.into()),
                }
            } else {
                self.navigation_error = Some("Open a document first.".into());
            }
        } else if response.should_close() {
            self.navigator = None;
        }
    }
    fn find_bar(&mut self, ui: &mut egui::Ui) {
        if !self.find_open {
            return;
        }
        ui.horizontal(|ui| {
            ui.label("Find");
            let changed = ui
                .add(egui::TextEdit::singleline(&mut self.find).desired_width(170.0))
                .changed();
            if changed {
                self.find_from = 0;
            }
            if ui.button("Next").clicked() {
                self.find_next();
            }
            ui.label("Replace");
            ui.add(egui::TextEdit::singleline(&mut self.replacement).desired_width(140.0));
            if ui.button("All").clicked()
                && !self.find.is_empty()
                && let Some(doc) = self.docs.get_mut(self.active)
            {
                let matches = doc.text.matches(&self.find).count();
                let new_len = doc
                    .text
                    .len()
                    .saturating_sub(matches * self.find.len())
                    .saturating_add(matches.saturating_mul(self.replacement.len()));
                if new_len <= aegis_lang::MAX_SOURCE {
                    doc.text = doc.text.replace(&self.find, &self.replacement);
                    doc.changed();
                    self.status = format!("Replaced {matches} matches");
                } else {
                    self.status = "Replacement would exceed source size limit".into();
                }
            }
            if ui.small_button("×").clicked() {
                self.find_open = false;
            }
        });
        ui.separator();
    }
    fn editor(&mut self, ui: &mut egui::Ui) {
        let Some(doc) = self.docs.get_mut(self.active) else {
            return;
        };
        let id = Id::new(("source", doc.id));
        let jump = doc
            .jump_to
            .take()
            .map(|at| doc.text.get(..at).unwrap_or("").chars().count());
        if let Some(index) = jump {
            let mut state = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(
                    egui::text::CCursor::new(index),
                )));
            state.store(ui.ctx(), id);
            ui.ctx().memory_mut(|m| m.request_focus(id));
        }
        let lines = doc.text.lines().count().max(1);
        // Gutter follows the same scroll area as the source text.
        let numbers = (1..=lines.min(20000))
            .map(|n| format!("{n:>4}"))
            .collect::<Vec<_>>()
            .join("\n");
        egui::ScrollArea::both()
            .id_salt(("editor-scroll", doc.id))
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.label(
                        RichText::new(numbers)
                            .font(FontId::monospace(15.0))
                            .color(Color32::from_rgb(76, 92, 112)),
                    );
                    let mut layouter = highlighting::layout;
                    let output = egui::TextEdit::multiline(&mut doc.text)
                        .id(id)
                        .code_editor()
                        .font(FontId::monospace(15.0))
                        .frame(false)
                        .desired_width(ui.available_width().max(600.0))
                        .desired_rows(24)
                        .char_limit(aegis_lang::MAX_SOURCE)
                        .layouter(&mut layouter)
                        .show(ui);
                    if output.response.changed() {
                        doc.changed();
                    }
                    if let Some(range) = output.cursor_range {
                        doc.cursor = doc.text.chars().take(range.primary.index).fold(
                            (1, 1),
                            |(line, col), c| {
                                if c == '\n' {
                                    (line + 1, 1)
                                } else {
                                    (line, col + 1)
                                }
                            },
                        );
                    }
                    if jump.is_some() {
                        let y = output.response.rect.top()
                            + (doc.cursor.0.saturating_sub(1)) as f32
                                * ui.fonts_mut(|f| f.row_height(&FontId::monospace(15.0)));
                        ui.scroll_to_rect(
                            egui::Rect::from_min_size(
                                egui::pos2(output.response.rect.left(), y),
                                Vec2::new(1.0, 20.0),
                            ),
                            Some(egui::Align::Center),
                        );
                    }
                });
            });
    }
    fn close_modal(&mut self, ctx: &egui::Context) {
        let Some(id) = self.close_doc else {
            return;
        };
        let title = self
            .docs
            .iter()
            .find(|d| d.id == id)
            .map_or_else(|| "document".into(), Document::title);
        let mut action = 0;
        egui::Modal::new(Id::new("unsaved")).show(ctx, |ui| {
            ui.heading("Unsaved changes");
            ui.label(format!("Save changes to {title}?"));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    action = 1;
                }
                if ui.button("Discard").clicked() {
                    action = 2;
                }
                if ui.button("Cancel").clicked() {
                    action = 3;
                }
            });
        });
        match action {
            1 => {
                if self.save(id, false, false) {
                    self.close_doc = None;
                    self.close_after_save = Some(id);
                }
            }
            2 => {
                self.close_doc = None;
                self.remove_doc(id, ctx);
            }
            3 => {
                self.close_doc = None;
                self.quit = false;
            }
            _ => {}
        }
    }
    fn unlock_modal(&mut self, ctx: &egui::Context) {
        let Some(unlock) = &mut self.unlock else {
            return;
        };
        let mut action = 0;
        egui::Modal::new(Id::new("credentials")).show(ctx, |ui| {
            ui.heading("Protected source");
            ui.label("Select the external key and enter its dialect (0–4095 or 0xABC).");
            ui.label("The key path is kept only while this dialog is open.");
            ui.add_space(8.0);
            if ui.button("Choose key file…").clicked() {
                unlock.key_path = rfd::FileDialog::new().pick_file();
            }
            ui.label(unlock.key_path.as_ref().map_or_else(
                || "No key selected".into(),
                |p| {
                    p.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                },
            ));
            ui.horizontal(|ui| {
                ui.label("Dialect");
                ui.add(
                    egui::TextEdit::singleline(&mut unlock.dialect)
                        .desired_width(120.0)
                        .char_limit(8),
                );
            });
            if let Some(error) = &unlock.error {
                ui.colored_label(Color32::from_rgb(249, 131, 142), error);
            }
            ui.horizontal(|ui| {
                if ui.button("Continue").clicked() {
                    action = 1;
                }
                if ui.button("Cancel").clicked() {
                    action = 2;
                }
            });
        });
        if action == 2 {
            self.unlock = None;
            self.close_after_save = None;
            self.quit = false;
        }
        if action != 1 {
            return;
        }
        let Some(unlock) = &mut self.unlock else {
            return;
        };
        let dialect = if unlock.dialect.starts_with("0x") || unlock.dialect.starts_with("0X") {
            u16::from_str_radix(&unlock.dialect[2..], 16)
        } else {
            unlock.dialect.parse()
        };
        let Some(dialect) = dialect.ok().filter(|&d| d < 4096) else {
            unlock.error = Some("Invalid dialect".into());
            return;
        };
        let Some(key_path) = unlock.key_path.clone() else {
            unlock.error = Some("Choose a key file".into());
            return;
        };
        let Some(unlock) = self.unlock.take() else {
            return;
        };
        let events = self.sender.clone();
        self.busy = true;
        self.status = "Processing protected source…".into();
        std::thread::spawn(move || {
            let key = storage::read_bounded(&key_path, codec::MAX_KEY)
                .map(Zeroizing::new)
                .map_err(|_| "Cannot read the selected key file".to_owned());
            match unlock.action {
                ProtectedAction::Open(path, bytes) => {
                    let result = key
                        .and_then(|key| codec::open(&bytes, &key, dialect))
                        .and_then(|source| {
                            String::from_utf8(source.to_vec())
                                .map_err(|_| "Decoded source is not UTF-8".into())
                        });
                    let _ = events.send(Event::Opened(path, result));
                }
                ProtectedAction::Save {
                    id,
                    revision,
                    path,
                    text,
                } => {
                    let result = key
                        .and_then(|key| codec::seal(text.as_bytes(), &key, dialect))
                        .and_then(|bytes| {
                            storage::atomic_write(&path, &bytes).map_err(|e| e.to_string())
                        });
                    let _ = events.send(Event::Saved {
                        id,
                        revision,
                        path,
                        protected: true,
                        result,
                    });
                }
            }
        });
    }
}
impl eframe::App for Studio {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.show(ctx);
        if self.licenses_open {
            egui::Window::new("Component licenses")
                .open(&mut self.licenses_open)
                .default_size([650.0, 450.0])
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.label(aegis_lang::THIRD_PARTY_NOTICES);
                    });
                });
        }
    }
}
impl Drop for Studio {
    fn drop(&mut self) {
        if let Some((_, stop)) = &self.run {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

fn project_files(root: &Path) -> Vec<PathBuf> {
    fn visit(path: &Path, depth: usize, visited: &mut usize, files: &mut Vec<PathBuf>) {
        if depth > 4 || *visited >= 4000 || files.len() >= 256 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            *visited += 1;
            if *visited > 4000 || files.len() >= 256 {
                break;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                if !matches!(
                    entry.file_name().to_str(),
                    Some("target" | ".git" | "node_modules" | "legacy")
                ) {
                    visit(&path, depth + 1, visited, files);
                }
            } else if matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("ae" | "aeg")
            ) {
                files.push(path);
            }
        }
    }
    let mut files = Vec::new();
    visit(root, 0, &mut 0, &mut files);
    files.sort();
    files
}
fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 850.0])
            .with_min_inner_size([900.0, 600.0]),
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(
        "Aegis Studio",
        options,
        Box::new(|cc| Ok(Box::new(Studio::new(cc)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keyboard_navigation_opens_selected_file_and_jumps() {
        let dir = tempfile::tempdir().expect("temp directory");
        let first = dir.path().join("first.ae");
        let second = dir.path().join("second.ae");
        std::fs::write(&first, "fn main() { return 1; }").expect("source");
        std::fs::write(&second, "// привет\nfn main() { return 2; }").expect("source");
        let ctx = egui::Context::default();
        let mut app = Studio::default();
        app.workspace = Some(dir.path().into());
        app.files = vec![first, second.clone()];
        app.open_navigator(true);
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.navigation_modal(ctx));
        let key = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let _ = ctx.run(
            egui::RawInput {
                events: vec![key(Key::ArrowDown), key(Key::Enter)],
                ..Default::default()
            },
            |ctx| app.navigation_modal(ctx),
        );
        assert!(app.navigator.is_none());
        assert_eq!(
            app.docs[app.active].path.as_ref(),
            Some(&std::fs::canonicalize(second).expect("path"))
        );
        app.open_navigator(false);
        app.navigation_query = "2:4".into();
        let _ = ctx.run(
            egui::RawInput {
                events: vec![key(Key::Enter)],
                ..Default::default()
            },
            |ctx| app.navigation_modal(ctx),
        );
        let doc = &app.docs[app.active];
        assert_eq!(doc.jump_to, Some("// привет\nfn ".len()));
        assert!(app.navigator.is_none());
    }
    #[test]
    fn renders_headless_and_tracks_tabs() {
        let ctx = egui::Context::default();
        let mut app = Studio::default();
        app.new_doc();
        assert_eq!(app.docs.len(), 2);
        app.find = "return".into();
        app.find_next();
        assert!(app.docs[app.active].jump_to.is_some());
        let out = ctx.run(egui::RawInput::default(), |ctx| app.show(ctx));
        assert!(!out.shapes.is_empty());
        let id = app.docs[app.active].id;
        app.request_close(id, &ctx);
        assert_eq!(app.docs.len(), 1);
    }
    #[test]
    fn dependency_edits_invalidate_project_analysis() {
        let ctx = egui::Context::default();
        let mut app = Studio::default();
        app.docs
            .push(Document::new(2, None, "fn helper() {}".into(), false));
        let stamp = editor_project::Stamp::new(&app.docs, &app.project_entry);
        app.docs[1].changed();
        app.analyzing = true;
        app.sender
            .send(Event::Analyzed(
                1,
                0,
                stamp,
                Err(Diagnostic {
                    message: "stale dependency".into(),
                    path: None,
                    span: 0..0,
                    line: 1,
                    column: 1,
                }),
            ))
            .expect("event");
        app.receive(&ctx);
        assert!(app.docs[0].diagnostic.is_none());
        assert!(!app.analyzing);
        assert!(app.docs[0].analyze_after.is_some());
    }
    #[test]
    fn project_locations_switch_buffers_without_overwriting_unsaved_text() {
        let dir = tempfile::tempdir().expect("dir");
        let entry = dir.path().join("main.ae");
        let child = dir.path().join("child.ae");
        std::fs::write(&entry, "mod child; fn main() {}").expect("entry");
        std::fs::write(&child, "fn helper() {}").expect("child");
        let mut app = Studio::default();
        app.open_path(entry);
        app.open_location(Some(child.clone()), 3);
        assert_eq!(app.docs[app.active].jump_to, Some(3));
        app.docs[app.active].text = "unsaved child".into();
        app.docs[app.active].changed();
        app.open_location(Some(child), 7);
        assert_eq!(app.docs[app.active].text, "unsaved child");
        assert_eq!(app.docs[app.active].jump_to, Some(7));
        app.open_location(Some(dir.path().join("missing.ae")), 99);
        assert_eq!(app.docs[app.active].jump_to, Some(7));
    }
    #[test]
    fn stale_analysis_is_ignored() {
        let ctx = egui::Context::default();
        let mut app = Studio::default();
        app.docs[0].changed();
        app.analyzing = true;
        app.sender
            .send(Event::Analyzed(
                1,
                0,
                editor_project::Stamp::new(&app.docs, &app.project_entry),
                Err(Diagnostic {
                    message: "stale".into(),
                    path: None,
                    span: 0..0,
                    line: 1,
                    column: 1,
                }),
            ))
            .expect("send");
        app.receive(&ctx);
        assert!(app.docs[0].diagnostic.is_none());
        assert!(!app.analyzing);
    }
}
