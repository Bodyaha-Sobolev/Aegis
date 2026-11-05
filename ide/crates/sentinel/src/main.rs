#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]
mod completion;
mod disassembly;
mod document;
mod editor_project;
mod highlighting;
mod icons;
mod inspector;
mod navigation;
mod permissions;
mod runner;
mod sky;
mod theme;

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

const SAMPLE: &str = include_str!("../assets/sample.ae");
const OUTPUT_LIMIT: usize = 1024 * 1024;
const MUTED: Color32 = theme::TEXT_2;

enum Event {
    Analyzed(u64, u64, editor_project::Stamp, AnalysisOutcome),
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

/// Result of one background project analysis: functions on success, first diagnostic otherwise.
type AnalysisOutcome = Result<Vec<FunctionInfo>, Diagnostic>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BottomTab {
    Output,
    Problems,
}

/// Content of the left icon rail / side panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Activity {
    Explorer,
    Symbols,
    Problems,
    Audit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CompletionState {
    prefix: String,
    items: Vec<completion::Completion>,
    selected: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command {
    New,
    OpenFile,
    OpenFolder,
    Save,
    SaveAs,
    CloseTab,
    Find,
    GoToFile,
    GoToLine,
    GoToDefinition,
    Build,
    Run,
    Stop,
    Inspect,
    Audit,
    Licenses,
    UseAsEntry,
    ClearEntry,
    Refresh,
    OutputTab,
    ProblemsTab,
}

struct CommandInfo {
    id: Command,
    label: &'static str,
    shortcut: &'static str,
}

const COMMANDS: &[CommandInfo] = &[
    CommandInfo {
        id: Command::Build,
        label: "Build",
        shortcut: "F6",
    },
    CommandInfo {
        id: Command::Run,
        label: "Build and run",
        shortcut: "F5",
    },
    CommandInfo {
        id: Command::Stop,
        label: "Stop run",
        shortcut: "",
    },
    CommandInfo {
        id: Command::Save,
        label: "Save",
        shortcut: "Ctrl+S",
    },
    CommandInfo {
        id: Command::SaveAs,
        label: "Save as…",
        shortcut: "Ctrl+Shift+S",
    },
    CommandInfo {
        id: Command::New,
        label: "New tab",
        shortcut: "Ctrl+N",
    },
    CommandInfo {
        id: Command::OpenFile,
        label: "Open file…",
        shortcut: "Ctrl+O",
    },
    CommandInfo {
        id: Command::OpenFolder,
        label: "Open project folder…",
        shortcut: "",
    },
    CommandInfo {
        id: Command::CloseTab,
        label: "Close tab",
        shortcut: "Ctrl+W",
    },
    CommandInfo {
        id: Command::Find,
        label: "Find and replace",
        shortcut: "Ctrl+F",
    },
    CommandInfo {
        id: Command::GoToFile,
        label: "Go to file…",
        shortcut: "Ctrl+P",
    },
    CommandInfo {
        id: Command::GoToLine,
        label: "Go to line…",
        shortcut: "Ctrl+G",
    },
    CommandInfo {
        id: Command::GoToDefinition,
        label: "Go to definition",
        shortcut: "F12",
    },
    CommandInfo {
        id: Command::Inspect,
        label: "Inspect EXE / DLL / SYS…",
        shortcut: "",
    },
    CommandInfo {
        id: Command::Audit,
        label: "Permission audit",
        shortcut: "",
    },
    CommandInfo {
        id: Command::Licenses,
        label: "Component licenses",
        shortcut: "",
    },
    CommandInfo {
        id: Command::UseAsEntry,
        label: "Use active file as project entry",
        shortcut: "",
    },
    CommandInfo {
        id: Command::ClearEntry,
        label: "Clear project entry",
        shortcut: "",
    },
    CommandInfo {
        id: Command::Refresh,
        label: "Refresh project files",
        shortcut: "",
    },
    CommandInfo {
        id: Command::ProblemsTab,
        label: "Show problems",
        shortcut: "",
    },
    CommandInfo {
        id: Command::OutputTab,
        label: "Show output",
        shortcut: "",
    },
];

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
    /// `--sky-preview`: the animated background alone, no interface on top.
    sky_preview: bool,
    /// The exact DeepSeek hero pattern, evaluated per frame.
    sky: sky::SkyField,
    smoke_exports: bool,
    smoke_disasm: bool,
    licenses_open: bool,
    screenshot: Option<PathBuf>,
    inspector: Option<inspector::Inspector>,
    inspecting: bool,
    project_entry: Option<PathBuf>,
    analysis_stamp: Option<editor_project::Stamp>,
    analysis_cache: Option<(editor_project::Stamp, u64, u64, AnalysisOutcome)>,
    permission_ledger: permissions::PermissionLedger,
    active_key_permission: Option<u64>,
    active_key_started: Option<Instant>,
    active_inspect_permission: Option<u64>,
    active_inspect_started: Option<Instant>,
    active_save_permission: Option<u64>,
    active_save_started: Option<Instant>,
    active_run_permission: Option<u64>,
    active_run_started: Option<Instant>,
    bottom_tab: BottomTab,
    activity: Activity,
    completion: Option<CompletionState>,
    palette: Option<(String, usize)>,
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
            sky_preview: false,
            sky: sky::SkyField::default(),
            smoke_exports: false,
            smoke_disasm: false,
            licenses_open: false,
            screenshot: None,
            inspector: None,
            inspecting: false,
            project_entry: None,
            analysis_stamp: None,
            analysis_cache: None,
            permission_ledger: permissions::PermissionLedger::default(),
            active_key_permission: None,
            active_key_started: None,
            active_inspect_permission: None,
            active_inspect_started: None,
            active_save_permission: None,
            active_save_started: None,
            active_run_permission: None,
            active_run_started: None,
            bottom_tab: BottomTab::Output,
            activity: Activity::Explorer,
            completion: None,
            palette: None,
        }
    }
}
impl Studio {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);
        let mut app = Self::default();
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        if args.first().is_some_and(|s| s == "--sky-preview") {
            app.sky_preview = true;
            // Optional second argument captures a frame for verification.
            if let Some(path) = args.get(1) {
                app.smoke_frames = Some(0);
                app.screenshot = Some(PathBuf::from(path));
            }
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            return app;
        }
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
        let target = path.display().to_string();
        let request_id = self.permission_ledger.request(
            "studio",
            permissions::Capability::InspectFile,
            &target,
            &permissions::parameter_hash(&target),
            permissions::Decision::Allow,
            "user requested binary inspection",
        );
        self.inspecting = true;
        self.active_inspect_permission = Some(request_id);
        self.active_inspect_started = Some(Instant::now());
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
        let audit_id = self.permission_ledger.request(
            "studio",
            permissions::Capability::ReadWorkspace,
            &path.display().to_string(),
            &permissions::parameter_hash(&path.display().to_string()),
            permissions::Decision::Allow,
            "user opened source file",
        );
        if let Some(index) = self
            .docs
            .iter()
            .position(|d| d.path.as_ref() == Some(&path))
        {
            self.active = index;
            self.remember_entry(&path);
            self.permission_ledger
                .finish(audit_id, "already-open", Duration::ZERO);
            return;
        }
        match storage::read_bounded(&path, aegis_lang::MAX_SOURCE + codec::OVERHEAD) {
            Err(e) => {
                self.permission_ledger
                    .finish(audit_id, "read-error", Duration::ZERO);
                self.fail(e.to_string())
            }
            Ok(bytes) if codec::is_protected(&bytes) => {
                self.permission_ledger
                    .finish(audit_id, "protected-pending-unlock", Duration::ZERO);
                self.unlock = Some(Unlock {
                    action: ProtectedAction::Open(path, Zeroizing::new(bytes)),
                    key_path: None,
                    dialect: String::new(),
                    error: None,
                })
            }
            Ok(bytes) if bytes.starts_with(b"AEG0") => {
                self.permission_ledger
                    .finish(audit_id, "legacy-rejected", Duration::ZERO);
                self.fail("Legacy AEG0 must first be decoded using the legacy prototype.")
            }
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) if text.len() <= aegis_lang::MAX_SOURCE && !text.contains('\0') => {
                    self.permission_ledger
                        .finish(audit_id, "opened", Duration::ZERO);
                    self.add_opened(path, text, false)
                }
                _ => {
                    self.permission_ledger
                        .finish(audit_id, "invalid-source", Duration::ZERO);
                    self.fail("Expected UTF-8 text without NUL bytes, at most 1 MiB.")
                }
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
        self.active_save_permission = Some(self.permission_ledger.request(
            "studio",
            permissions::Capability::WriteWorkspace,
            &path.display().to_string(),
            &permissions::parameter_hash(&path.display().to_string()),
            permissions::Decision::Allow,
            "user requested source save",
        ));
        self.active_save_started = Some(Instant::now());
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
                let result = storage::atomic_write_text(&path, &text, aegis_lang::MAX_SOURCE)
                    .map_err(|e| e.to_string());
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
                    if let Some(request_id) = self.active_inspect_permission.take() {
                        let outcome = if result.is_ok() {
                            "inspected"
                        } else {
                            "inspect-error"
                        };
                        let duration = self
                            .active_inspect_started
                            .take()
                            .map_or(Duration::ZERO, |started| started.elapsed());
                        self.permission_ledger.finish(request_id, outcome, duration);
                    }
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
                    let fresh = self
                        .docs
                        .iter()
                        .any(|d| d.id == id && d.revision == revision);
                    if fresh {
                        self.analysis_cache = Some((stamp, id, revision, result.clone()));
                        self.apply_analysis(id, revision, result);
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
                                    &permissions::parameter_hash(&path.display().to_string()),
                                    permissions::Decision::Allow,
                                    "user requested Build and run",
                                ));
                                self.active_run_started = Some(Instant::now());
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
                    if let Some(request_id) = self.active_key_permission.take() {
                        let outcome = if result.is_ok() {
                            "key-read-and-saved"
                        } else {
                            "key-read-error"
                        };
                        let duration = self
                            .active_key_started
                            .take()
                            .map_or(Duration::ZERO, |started| started.elapsed());
                        self.permission_ledger.finish(request_id, outcome, duration);
                    }
                    if let Some(request_id) = self.active_save_permission.take() {
                        let result = if result.is_ok() {
                            "saved"
                        } else {
                            "save-error"
                        };
                        let duration = self
                            .active_save_started
                            .take()
                            .map_or(Duration::ZERO, |started| started.elapsed());
                        self.permission_ledger.finish(request_id, result, duration);
                    }
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
                    if let Some(request_id) = self.active_key_permission.take() {
                        let outcome = if result.is_ok() {
                            "key-read"
                        } else {
                            "key-read-error"
                        };
                        let duration = self
                            .active_key_started
                            .take()
                            .map_or(Duration::ZERO, |started| started.elapsed());
                        self.permission_ledger.finish(request_id, outcome, duration);
                    }
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
                            let duration = self
                                .active_run_started
                                .take()
                                .map_or(Duration::ZERO, |started| started.elapsed());
                            self.permission_ledger.finish(request_id, &result, duration);
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
        if let Some((cached_stamp, cached_id, cached_revision, cached_result)) =
            &self.analysis_cache
            && cached_stamp == &stamp
            && *cached_id == id
            && *cached_revision == revision
        {
            self.analyzing = false;
            self.apply_analysis(id, revision, cached_result.clone());
            return;
        }
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

    fn apply_analysis(&mut self, id: u64, revision: u64, result: AnalysisOutcome) {
        match result {
            Ok(functions) => {
                for doc in &mut self.docs {
                    doc.diagnostic = None;
                }
                if let Some(doc) = self
                    .docs
                    .iter_mut()
                    .find(|doc| doc.id == id && doc.revision == revision)
                {
                    doc.functions = functions;
                }
            }
            Err(error) => {
                // One analysis pipeline covers the whole project snapshot: the
                // latest result replaces every stored diagnostic, and the error
                // lands on the document its path points to.
                for doc in &mut self.docs {
                    doc.diagnostic = None;
                }
                let target = error
                    .path
                    .as_ref()
                    .and_then(|path| {
                        self.docs
                            .iter()
                            .position(|doc| doc.path.as_ref() == Some(path))
                    })
                    .or_else(|| {
                        self.docs
                            .iter()
                            .position(|doc| doc.id == id && doc.revision == revision)
                    });
                if let Some(index) = target {
                    let doc = &mut self.docs[index];
                    doc.functions.clear();
                    doc.diagnostic = Some(error);
                }
            }
        }
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
    /// Byte offset of the editor caret as a char index into the active document.
    fn caret_char_index(ctx: &egui::Context, id: Id) -> usize {
        egui::text_edit::TextEditState::load(ctx, id)
            .and_then(|state| state.cursor.char_range())
            .map_or(0, |range| range.primary.index)
    }
    fn caret_byte_offset(text: &str, char_index: usize) -> usize {
        text.char_indices()
            .nth(char_index)
            .map_or(text.len(), |(at, _)| at)
    }
    fn go_to_definition(&mut self, ctx: &egui::Context) {
        let Some(doc) = self.docs.get(self.active) else {
            return;
        };
        let id = Id::new(("source", doc.id));
        let byte_at = Self::caret_byte_offset(&doc.text, Self::caret_char_index(ctx, id));
        let Some((start, end)) = navigation::identifier_at(&doc.text, byte_at) else {
            self.status = "Place the cursor on an identifier.".into();
            return;
        };
        let name = doc.text[start..end].to_owned();
        if let Some((doc_index, at)) = navigation::find_definition(&self.docs, self.active, &name) {
            self.active = doc_index;
            self.find_from = 0;
            if let Some(target) = self.docs.get_mut(doc_index) {
                target.jump_to = Some(at);
            }
            self.status = format!("Definition: {name}");
            return;
        }
        // `mod name;` has no in-buffer declaration: the definition is the file.
        let base = self
            .docs
            .get(self.active)
            .and_then(|doc| doc.path.as_ref())
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .or_else(|| {
                self.project_entry
                    .as_ref()
                    .and_then(|path| path.parent().map(|dir| dir.to_path_buf()))
            });
        if let Some(base) = base {
            let candidate = base.join(format!("{name}.ae"));
            if candidate.is_file() {
                self.open_path(candidate);
                return;
            }
        }
        self.status = format!("No definition for {name}");
    }
    fn open_completion(&mut self, ctx: &egui::Context) {
        let Some(doc) = self.docs.get(self.active) else {
            return;
        };
        let id = Id::new(("source", doc.id));
        let byte_at = Self::caret_byte_offset(&doc.text, Self::caret_char_index(ctx, id));
        self.completion = Self::fresh_completion(&doc.text, doc.functions.as_slice(), byte_at);
    }
    fn fresh_completion(
        text: &str,
        functions: &[FunctionInfo],
        byte_at: usize,
    ) -> Option<CompletionState> {
        let prefix = navigation::prefix_before(text, byte_at);
        let items = completion::completions(&prefix, functions, text);
        (!items.is_empty()).then_some(CompletionState {
            prefix,
            items,
            selected: 0,
        })
    }
    fn accept_completion(&mut self, ctx: &egui::Context, index: usize) {
        let Some(state) = self.completion.take() else {
            return;
        };
        let Some(item) = state.items.get(index) else {
            return;
        };
        let label = item.label.clone();
        let Some(doc) = self.docs.get_mut(self.active) else {
            return;
        };
        let id = Id::new(("source", doc.id));
        let char_index = Self::caret_char_index(ctx, id);
        let byte_at = Self::caret_byte_offset(&doc.text, char_index);
        let start = byte_at.saturating_sub(state.prefix.len());
        let applies = start <= byte_at
            && doc
                .text
                .get(start..byte_at)
                .is_some_and(|slice| slice == state.prefix.as_str());
        if !applies {
            return;
        }
        doc.text.replace_range(start..byte_at, &label);
        doc.changed();
        let new_index = char_index - state.prefix.chars().count() + label.chars().count();
        if let Some(mut loaded) = egui::text_edit::TextEditState::load(ctx, id) {
            loaded
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(
                    egui::text::CCursor::new(new_index),
                )));
            loaded.store(ctx, id);
        }
    }
    fn problems_panel(&mut self, ui: &mut egui::Ui) {
        let mut jump = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut any = false;
            for doc in &self.docs {
                let Some(error) = &doc.diagnostic else {
                    continue;
                };
                any = true;
                let where_ = error
                    .path
                    .as_ref()
                    .map_or_else(|| doc.title(), |path| path.display().to_string());
                let row = ui.selectable_label(
                    false,
                    RichText::new(format!("⛔ {}:{}:{}", where_, error.line, error.column))
                        .monospace(),
                );
                let row = row.on_hover_text(&error.message);
                if row.clicked() {
                    jump = Some((doc.id, error.span.start));
                }
                ui.label(RichText::new(&error.message).small());
            }
            if !any {
                ui.label(
                    RichText::new(
                        "No problems in open sources. Analysis covers the active project snapshot.",
                    )
                    .color(MUTED),
                );
            }
        });
        if let Some((id, offset)) = jump
            && let Some(index) = self.docs.iter().position(|doc| doc.id == id)
        {
            self.active = index;
            self.find_from = 0;
            if let Some(doc) = self.docs.get_mut(index) {
                doc.jump_to = Some(offset);
            }
        }
    }
    fn palette_modal(&mut self, ctx: &egui::Context) {
        if self.palette.is_none() {
            return;
        }
        let mut chosen: Option<Command> = None;
        let mut close = false;
        let response = egui::Modal::new(Id::new("palette")).show(ctx, |ui| {
            ui.set_min_width(480.0);
            ui.heading("Command palette");
            let moved = ui.input_mut(|i| {
                let down = i.consume_key(Modifiers::NONE, Key::ArrowDown);
                let up = i.consume_key(Modifiers::NONE, Key::ArrowUp);
                if down && let Some((_, selected)) = &mut self.palette {
                    *selected = selected.saturating_add(1);
                }
                if up && let Some((_, selected)) = &mut self.palette {
                    *selected = selected.saturating_sub(1);
                }
                down || up
            });
            let enter = ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
            let escape = ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
            if let Some((query, selected)) = &mut self.palette {
                let field = ui.add(
                    egui::TextEdit::singleline(query)
                        .desired_width(480.0)
                        .hint_text("Type a command"),
                );
                if field.changed() {
                    *selected = 0;
                }
                let candidates: Vec<&CommandInfo> = COMMANDS
                    .iter()
                    .filter(|info| navigation::matches(info.label, query))
                    .collect();
                *selected = (*selected).min(candidates.len().saturating_sub(1));
                ui.label(format!(
                    "{} commands · ↑ ↓ select · Enter runs · Esc closes",
                    candidates.len()
                ));
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        for (index, info) in candidates.iter().enumerate() {
                            let row = ui.selectable_label(
                                index == *selected,
                                RichText::new(info.label.to_string()).monospace(),
                            );
                            let row = if info.shortcut.is_empty() {
                                row
                            } else {
                                row.on_hover_text(info.shortcut)
                            };
                            if moved && index == *selected {
                                row.scroll_to_me(Some(egui::Align::Center));
                            }
                            if row.clicked() {
                                chosen = Some(info.id);
                            }
                        }
                    });
                if enter {
                    chosen = candidates.get(*selected).map(|info| info.id);
                }
            }
            if escape {
                close = true;
            }
        });
        if let Some(command) = chosen {
            self.palette = None;
            self.completion = None;
            self.run_command(command, ctx);
        } else if close || response.should_close() {
            self.palette = None;
        }
    }
    fn run_command(&mut self, command: Command, ctx: &egui::Context) {
        match command {
            Command::New => self.new_doc(),
            Command::OpenFile => self.open_dialog(),
            Command::OpenFolder => self.open_folder(),
            Command::Save => {
                if let Some(doc) = self.docs.get(self.active) {
                    self.save(doc.id, false, false);
                }
            }
            Command::SaveAs => {
                if let Some(doc) = self.docs.get(self.active) {
                    self.save(doc.id, true, false);
                }
            }
            Command::CloseTab => {
                if let Some(doc) = self.docs.get(self.active) {
                    let id = doc.id;
                    self.request_close(id, ctx);
                }
            }
            Command::Find => {
                self.find_open = true;
                self.find_from = 0;
            }
            Command::GoToFile => self.open_navigator(true),
            Command::GoToLine => self.open_navigator(false),
            Command::GoToDefinition => self.go_to_definition(ctx),
            Command::Build => self.build(false),
            Command::Run => self.build(true),
            Command::Stop => {
                if let Some((_, stop)) = &self.run {
                    stop.store(true, Ordering::Relaxed);
                }
            }
            Command::Inspect => {
                if !self.inspecting
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("Windows binary", &["exe", "dll", "sys"])
                        .pick_file()
                {
                    self.inspect_binary(path, ctx.clone());
                }
            }
            Command::Audit => self.activity = Activity::Audit,
            Command::Licenses => self.licenses_open = true,
            Command::UseAsEntry => {
                self.project_entry = self.docs.get(self.active).and_then(|doc| doc.path.clone());
                self.analysis_stamp = None;
            }
            Command::ClearEntry => {
                self.project_entry = None;
                self.analysis_stamp = None;
            }
            Command::Refresh => {
                if let Some(root) = &self.workspace {
                    self.files = project_files(root);
                    self.analysis_stamp = None;
                    for doc in &mut self.docs {
                        doc.analyze_after = Some(Instant::now());
                    }
                }
            }
            Command::OutputTab => self.bottom_tab = BottomTab::Output,
            Command::ProblemsTab => self.bottom_tab = BottomTab::Problems,
        }
    }
    fn open_folder(&mut self) {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            self.files = project_files(&path);
            let entry = path.join("main.ae");
            self.project_entry = entry
                .is_file()
                .then(|| entry.canonicalize().unwrap_or(entry));
            self.workspace = Some(path);
        }
    }
    fn tree_nodes(ui: &mut egui::Ui, nodes: &[navigation::TreeNode], open: &mut Option<PathBuf>) {
        for node in nodes {
            if node.is_dir {
                egui::CollapsingHeader::new(RichText::new(node.name.clone()).strong().size(13.0))
                    .default_open(true)
                    .show(ui, |ui| Self::tree_nodes(ui, &node.children, open));
            } else {
                let row = ui.selectable_label(false, RichText::new(node.name.clone()).size(13.0));
                let row = row.on_hover_text(node.path.display().to_string());
                if row.clicked() {
                    *open = Some(node.path.clone());
                }
            }
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
        if pressed(Key::P, Modifiers::CTRL | Modifiers::SHIFT) {
            self.palette = match self.palette.is_some() {
                true => None,
                false => Some((String::new(), 0)),
            };
        }
        if self.navigator.is_some() {
            return;
        }
        if self.palette.is_some() {
            return;
        }
        // The completion popup consumes navigation keys before the editor does.
        if self.completion.is_some() {
            if pressed(Key::Escape, Modifiers::NONE) {
                self.completion = None;
            } else if pressed(Key::ArrowDown, Modifiers::NONE) {
                if let Some(state) = &mut self.completion {
                    state.selected = (state.selected + 1).min(state.items.len().saturating_sub(1));
                }
            } else if pressed(Key::ArrowUp, Modifiers::NONE) {
                if let Some(state) = &mut self.completion {
                    state.selected = state.selected.saturating_sub(1);
                }
            } else if pressed(Key::Enter, Modifiers::NONE) || pressed(Key::Tab, Modifiers::NONE) {
                let selected = self.completion.as_ref().map_or(0, |state| state.selected);
                self.accept_completion(ctx, selected);
            }
        }
        if pressed(Key::Space, Modifiers::CTRL) {
            self.open_completion(ctx);
        }
        if pressed(Key::F12, Modifiers::NONE) {
            self.go_to_definition(ctx);
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
        // Headless tests render through show() without Studio::new(): make sure
        // the named text styles exist before any RichText references them.
        if !ctx
            .style()
            .text_styles
            .contains_key(&egui::TextStyle::Name("Brand".into()))
        {
            theme::install(ctx);
        }
        if self.sky_preview {
            // Nothing but the living background fills the window; a hairline
            // custom titlebar (drag zone + minimal controls) replaces the
            // native one. The smoke frame counter still runs so
            // `--sky-preview shot.png` captures and exits on its own.
            if let Some(frames) = &mut self.smoke_frames {
                *frames += 1;
                if *frames == 16 && self.screenshot.is_some() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                }
                if *frames >= 40 {
                    self.allow_exit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    let full = ui.max_rect();
                    self.sky.paint(ui, full);
                    // --- custom titlebar ------------------------------------------------
                    let bar_height = 40.0;
                    let bar =
                        egui::Rect::from_min_size(full.min, egui::vec2(full.width(), bar_height));
                    let button_size = egui::vec2(46.0, bar_height);
                    let close_rect = egui::Rect::from_min_size(
                        bar.right_top() - egui::vec2(46.0, 0.0),
                        button_size,
                    );
                    let max_rect_w = egui::Rect::from_min_size(
                        close_rect.left_top() - egui::vec2(46.0, 0.0),
                        button_size,
                    );
                    let min_rect_w = egui::Rect::from_min_size(
                        max_rect_w.left_top() - egui::vec2(46.0, 0.0),
                        button_size,
                    );
                    let drag_rect =
                        egui::Rect::from_min_max(bar.min, egui::pos2(min_rect_w.left(), bar.max.y));
                    let drag = ui.interact(
                        drag_rect,
                        Id::new("sky-drag"),
                        egui::Sense::click_and_drag(),
                    );
                    if drag.double_clicked() {
                        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                    } else if drag.dragged() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                    let window_button =
                        |ui: &egui::Ui,
                         rect: egui::Rect,
                         id: &str,
                         hover_fill: Color32,
                         draw: fn(&egui::Painter, egui::Rect, Color32)| {
                            let response = ui.interact(rect, Id::new(id), egui::Sense::click());
                            let painter = ui.painter();
                            if response.hovered() {
                                painter.rect_filled(rect, 0.0, hover_fill);
                            }
                            let ink = if response.hovered() {
                                theme::TEXT
                            } else {
                                theme::TEXT_3
                            };
                            draw(painter, rect.shrink2(egui::vec2(16.0, 12.0)), ink);
                            response
                        };
                    if window_button(
                        ui,
                        close_rect,
                        "sky-close",
                        Color32::from_rgba_unmultiplied(232, 17, 35, 230),
                        icons::close,
                    )
                    .clicked()
                    {
                        self.allow_exit = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if window_button(
                        ui,
                        max_rect_w,
                        "sky-maximize",
                        theme::HOVER,
                        |painter, rect, ink| {
                            painter.rect_stroke(
                                rect,
                                2.0,
                                egui::Stroke::new(1.6, ink),
                                egui::StrokeKind::Inside,
                            );
                        },
                    )
                    .clicked()
                    {
                        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                    }
                    if window_button(
                        ui,
                        min_rect_w,
                        "sky-minimize",
                        theme::HOVER,
                        |painter, rect, ink| {
                            painter.line_segment(
                                [rect.left_bottom(), rect.right_bottom()],
                                egui::Stroke::new(1.6, ink),
                            );
                        },
                    )
                    .clicked()
                    {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    }
                });
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
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.allow_exit = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ctx.request_repaint_after(Duration::from_millis(33));
            return;
        }
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
            || "Sentinel".into(),
            |d| {
                format!(
                    "{}{} — Sentinel",
                    if d.dirty() { "• " } else { "" },
                    d.title()
                )
            },
        );
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        egui::TopBottomPanel::top("toolbar")
            .exact_height(52.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let (mark, _) = ui.allocate_exact_size(Vec2::splat(28.0), egui::Sense::hover());
                    theme::brand_mark(ui.painter(), mark);
                    ui.label(
                        RichText::new("S E N T I N E L")
                            .text_style(egui::TextStyle::Name("Brand".into()))
                            .color(theme::TEXT),
                    );
                    let (chip, _) =
                        ui.allocate_exact_size(egui::vec2(64.0, 18.0), egui::Sense::hover());
                    ui.painter().rect_stroke(
                        chip,
                        9.0,
                        theme::border_strong(),
                        egui::StrokeKind::Inside,
                    );
                    ui.painter().text(
                        chip.center(),
                        egui::Align2::CENTER_CENTER,
                        "AEGIS IDE",
                        egui::FontId::proportional(9.5),
                        theme::TEXT_3,
                    );
                    ui.add_space(4.0);
                    if Self::icon_button(ui, icons::plus, "New tab  (Ctrl+N)").clicked() {
                        self.new_doc();
                    }
                    if !self.busy
                        && Self::icon_button(ui, icons::folder, "Open file  (Ctrl+O)").clicked()
                    {
                        self.open_dialog();
                    }
                    let id = self.docs.get(self.active).map(|d| d.id);
                    if Self::icon_button(ui, icons::save, "Save  (Ctrl+S)").clicked()
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
                            self.open_folder();
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
                                .add(
                                    egui::Button::new(RichText::new("Stop").color(theme::DANGER))
                                        .corner_radius(16.0)
                                        .fill(theme::RAISED)
                                        .stroke(theme::border_strong()),
                                )
                                .on_hover_text("Stop the running program")
                                .clicked()
                            {
                                stop.store(true, Ordering::Relaxed);
                            }
                        } else if !self.busy
                            && theme::gradient_button(ui, "Run  F5", Some(icons::play)).clicked()
                        {
                            self.build(true);
                        }
                        if !self.busy
                            && self.run.is_none()
                            && ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Build  F6").color(theme::TEXT_2),
                                    )
                                    .corner_radius(16.0)
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(theme::border()),
                                )
                                .clicked()
                        {
                            self.build(false);
                        }
                    });
                });
                ui.painter().line_segment(
                    [ui.max_rect().left_bottom(), ui.max_rect().right_bottom()],
                    theme::border(),
                );
            });
        egui::TopBottomPanel::bottom("status")
            .exact_height(28.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(10.0);
                    let failed =
                        self.status == "Operation failed" || self.status.starts_with("ERROR");
                    let (dot, _) = ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                    ui.painter().circle_filled(
                        dot.center(),
                        3.5,
                        if failed { theme::DANGER } else { theme::OK },
                    );
                    ui.label(RichText::new(&self.status).size(12.0).color(theme::TEXT_2));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(10.0);
                        ui.label(
                            theme::caption(concat!("Sentinel ", env!("CARGO_PKG_VERSION")))
                                .color(theme::TEXT_3),
                        );
                        ui.label(RichText::new("·").size(12.0).color(theme::TEXT_3));
                        ui.label(RichText::new("Windows x64").size(12.0).color(theme::TEXT_3));
                        ui.label(RichText::new("·").size(12.0).color(theme::TEXT_3));
                        ui.label(RichText::new("UTF-8").size(12.0).color(theme::TEXT_3));
                        if let Some(doc) = self.docs.get(self.active) {
                            if doc.protected {
                                ui.label(
                                    RichText::new("Protected").size(12.0).color(theme::WARNING),
                                );
                                ui.label(RichText::new("·").size(12.0).color(theme::TEXT_3));
                            }
                            ui.label(
                                RichText::new(format!("Ln {}, Col {}", doc.cursor.0, doc.cursor.1))
                                    .size(12.0)
                                    .color(theme::TEXT_3),
                            );
                        }
                    });
                });
            });
        self.activity_bar(ctx);
        self.side_panel(ctx);
        egui::TopBottomPanel::bottom("output").resizable(true).default_height(200.0).min_height(90.0).show(ctx,|ui| {
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                let problems = self.docs.iter().filter(|doc| doc.diagnostic.is_some()).count();
                if ui
                    .selectable_label(
                        self.bottom_tab == BottomTab::Output,
                        RichText::new("OUTPUT").small().strong().color(
                            if self.bottom_tab == BottomTab::Output { theme::TEXT } else { theme::TEXT_3 },
                        ),
                    )
                    .clicked()
                {
                    self.bottom_tab = BottomTab::Output;
                }
                if ui
                    .selectable_label(
                        self.bottom_tab == BottomTab::Problems,
                        RichText::new(format!("PROBLEMS ({problems})"))
                            .small()
                            .strong()
                            .color(if problems > 0 {
                                theme::DANGER
                            } else if self.bottom_tab == BottomTab::Problems {
                                theme::TEXT
                            } else {
                                theme::TEXT_3
                            }),
                    )
                    .clicked()
                {
                    self.bottom_tab = BottomTab::Problems;
                }
                if self.bottom_tab == BottomTab::Output {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Clear").size(12.0).color(theme::TEXT_3))
                                .corner_radius(6.0)
                                .fill(Color32::TRANSPARENT)
                                .stroke(egui::Stroke::NONE),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        self.output.clear();
                    }
                    if self.busy || self.analyzing { ui.spinner(); }
                    if self.output.len()>=OUTPUT_LIMIT { ui.label(RichText::new("Output limit reached (1 MiB)").small().color(Color32::YELLOW)); }
                }
                ui.painter().line_segment(
                    [ui.max_rect().left_bottom(), ui.max_rect().right_bottom()],
                    theme::border(),
                );
            });
            match self.bottom_tab {
                BottomTab::Output => {
                    egui::ScrollArea::both().stick_to_bottom(true).show(ui,|ui| {
                        if self.output.is_empty() { ui.label(RichText::new("Build and run output appears here. Source is compiled directly to native machine code.").color(MUTED)); }
                        else { ui.add(egui::Label::new(RichText::new(String::from_utf8_lossy(&self.output)).monospace()).selectable(true)); }
                    });
                }
                BottomTab::Problems => self.problems_panel(ui),
            }
        });
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.style()).fill(theme::BG))
            .show(ctx, |ui| {
                self.sky.paint(ui, ui.max_rect());
                let mut close = None;
                let tab_strip = egui::ScrollArea::horizontal().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        for (i, doc) in self.docs.iter().enumerate() {
                            let active = i == self.active;
                            let title = doc.title();
                            let text_width = ui.fonts_mut(|f| {
                                f.layout_no_wrap(
                                    title.clone(),
                                    egui::FontId::new(
                                        13.5,
                                        egui::FontFamily::Name("Medium".into()),
                                    ),
                                    theme::TEXT,
                                )
                                .size()
                                .x
                            });
                            let width = (text_width + 44.0).max(96.0);
                            let (rect, response) = ui
                                .allocate_exact_size(egui::vec2(width, 34.0), egui::Sense::click());
                            let response = response.on_hover_text(doc.path.as_ref().map_or_else(
                                || "Unsaved buffer".into(),
                                |p| p.display().to_string(),
                            ));
                            let painter = ui.painter();
                            let mut text_x = rect.left() + 6.0;
                            if doc.protected {
                                painter.text(
                                    egui::pos2(text_x + 5.0, rect.center().y),
                                    egui::Align2::LEFT_CENTER,
                                    "◈",
                                    egui::FontId::proportional(12.5),
                                    theme::ACCENT_VIOLET,
                                );
                                text_x += 14.0;
                            }
                            painter.text(
                                egui::pos2(text_x + 5.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                &title,
                                egui::FontId::new(13.5, egui::FontFamily::Name("Medium".into())),
                                if active { theme::TEXT } else { theme::TEXT_3 },
                            );
                            if active {
                                painter.rect_filled(
                                    egui::Rect::from_min_size(
                                        egui::pos2(rect.left() + 6.0, rect.bottom() - 2.0),
                                        egui::vec2(rect.width() - 12.0, 2.0),
                                    ),
                                    1.0,
                                    theme::ACCENT,
                                );
                            } else if response.hovered() {
                                painter.text(
                                    egui::pos2(text_x + 5.0, rect.center().y),
                                    egui::Align2::LEFT_CENTER,
                                    &title,
                                    egui::FontId::new(
                                        13.5,
                                        egui::FontFamily::Name("Medium".into()),
                                    ),
                                    theme::TEXT_2,
                                );
                            }
                            let close_rect = egui::Rect::from_center_size(
                                egui::pos2(rect.right() - 12.0, rect.center().y),
                                egui::vec2(17.0, 17.0),
                            );
                            let close_response = ui
                                .interact(
                                    close_rect,
                                    Id::new(("tab-close", doc.id)),
                                    egui::Sense::click(),
                                )
                                .on_hover_text("Close tab  (Ctrl+W)");
                            if close_response.hovered() {
                                painter.rect_filled(close_rect, 5.0, theme::HOVER);
                            }
                            if close_response.hovered() || response.hovered() {
                                icons::close(painter, close_rect.shrink(4.0), theme::TEXT_2);
                            } else if doc.dirty() {
                                painter.circle_filled(close_rect.center(), 3.0, theme::ACCENT);
                            } else if active {
                                icons::close(painter, close_rect.shrink(4.0), theme::TEXT_3);
                            } else {
                                icons::close(painter, close_rect.shrink(4.0), theme::GUTTER);
                            }
                            if close_response.clicked() {
                                close = Some(doc.id);
                            }
                            if response.clicked() {
                                self.active = i;
                                self.find_from = 0;
                            }
                        }
                        let (plus_rect, plus_response) =
                            ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::click());
                        if plus_response.hovered() {
                            ui.painter().rect_filled(plus_rect, 7.0, theme::HOVER);
                        }
                        icons::plus(ui.painter(), plus_rect.shrink(6.0), theme::TEXT_3);
                        if plus_response.clicked() {
                            self.new_doc();
                        }
                    })
                });
                // Hairline under the whole tab strip, DeepSeek-docs style.
                ui.painter().line_segment(
                    [
                        tab_strip.inner.response.rect.left_bottom(),
                        tab_strip.inner.response.rect.right_bottom(),
                    ],
                    theme::border(),
                );
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
        self.palette_modal(ctx);
        if self.busy
            || self.inspecting
            || self.analyzing
            || self.run.is_some()
            || self.docs.iter().any(|d| d.analyze_after.is_some())
        {
            ctx.request_repaint_after(Duration::from_millis(35));
        } else {
            // Keep the living background drifting even when idle.
            ctx.request_repaint_after(Duration::from_millis(66));
        }
    }
    fn icon_button(
        ui: &mut egui::Ui,
        draw: fn(&egui::Painter, egui::Rect, Color32),
        tip: &str,
    ) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(34.0, 30.0), egui::Sense::click());
        let response = response.on_hover_text(tip);
        if response.hovered() {
            ui.painter().rect_filled(rect, 8.0, theme::HOVER);
        }
        draw(ui.painter(), rect.shrink(6.0), theme::TEXT_2);
        response
    }
    fn activity_bar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("activity")
            .exact_width(48.0)
            .frame(egui::Frame::new().fill(theme::BG))
            .show(ctx, |ui| {
                ui.add_space(12.0);
                ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                    let problems = self
                        .docs
                        .iter()
                        .filter(|doc| doc.diagnostic.is_some())
                        .count();
                    let items = [
                        ("Explorer", Activity::Explorer),
                        ("Symbols", Activity::Symbols),
                        ("Problems", Activity::Problems),
                        ("Audit log", Activity::Audit),
                    ];
                    for (tip, target) in items {
                        let active = self.activity == target;
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(40.0, 36.0), egui::Sense::click());
                        let response = response.on_hover_text(tip);
                        let painter = ui.painter();
                        if active {
                            painter.rect_filled(rect, 10.0, theme::ACTIVE);
                        } else if response.hovered() {
                            painter.rect_filled(rect, 10.0, theme::HOVER);
                        }
                        let color = if active { theme::ACCENT } else { theme::TEXT_3 };
                        let inner = rect.shrink(9.0);
                        match target {
                            Activity::Explorer => {
                                icons::document(painter, inner, color);
                            }
                            Activity::Symbols => {
                                painter.text(
                                    inner.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "ƒ",
                                    egui::FontId::proportional(16.0),
                                    color,
                                );
                            }
                            Activity::Problems => {
                                icons::warning(painter, inner, color);
                            }
                            Activity::Audit => {
                                icons::shield(painter, inner, color);
                            }
                        }
                        if target == Activity::Problems && problems > 0 {
                            painter.circle_filled(
                                rect.right_top() + egui::vec2(-7.0, 7.0),
                                3.5,
                                theme::DANGER,
                            );
                        }
                        if response.clicked() {
                            self.activity = target;
                        }
                    }
                });
            });
    }
    fn side_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("explorer")
            .resizable(true)
            .default_width(232.0)
            .width_range(170.0..=420.0)
            .show(ctx, |ui| {
                ui.add_space(12.0);
                match self.activity {
                    Activity::Explorer => self.explorer_panel(ui),
                    Activity::Symbols => self.symbols_panel(ui),
                    Activity::Problems => {
                        ui.label(
                            RichText::new("PROBLEMS")
                                .small()
                                .strong()
                                .color(theme::TEXT_3),
                        );
                        self.problems_panel(ui);
                    }
                    Activity::Audit => self.audit_panel(ui),
                }
            });
    }
    fn explorer_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("EXPLORER")
                    .small()
                    .strong()
                    .color(theme::TEXT_3),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if Self::icon_button(ui, icons::search, "Find file  (Ctrl+P)").clicked() {
                    self.open_navigator(true);
                }
                if Self::icon_button(ui, icons::refresh, "Refresh project files").clicked()
                    && let Some(root) = &self.workspace
                {
                    self.files = project_files(root);
                    self.analysis_stamp = None;
                    for doc in &mut self.docs {
                        doc.analyze_after = Some(Instant::now());
                    }
                }
            });
        });
        let Some(root) = self.workspace.clone() else {
            theme::card().show(ui, |ui| {
                ui.label(RichText::new("No project folder").strong());
                ui.label(
                    RichText::new(
                        "Open a folder with AEGIS sources to browse its files and symbols.",
                    )
                    .small()
                    .color(MUTED),
                );
                let open_width = ui.available_width();
                let (button_rect, button_response) =
                    ui.allocate_exact_size(egui::vec2(open_width, 32.0), egui::Sense::click());
                let button_response =
                    button_response.on_hover_cursor(egui::CursorIcon::PointingHand);
                ui.painter().rect_filled(
                    button_rect,
                    10.0,
                    if button_response.hovered() {
                        theme::ACCENT_HOVER
                    } else {
                        theme::ACCENT
                    },
                );
                ui.painter().text(
                    button_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Open project folder…",
                    egui::FontId::new(13.5, egui::FontFamily::Name("Medium".into())),
                    Color32::WHITE,
                );
                if button_response.clicked() {
                    self.open_folder();
                }
            });
            return;
        };
        ui.label(RichText::new(root.file_name().unwrap_or_default().to_string_lossy()).strong());
        let mut open = None;
        egui::ScrollArea::vertical()
            .max_height(260.0)
            .show(ui, |ui| {
                let tree = navigation::build_tree(
                    self.workspace.as_deref().unwrap_or(Path::new("")),
                    &self.files,
                );
                Self::tree_nodes(ui, &tree, &mut open);
            });
        if let Some(path) = open {
            self.open_path(path);
        }
    }
    fn symbols_panel(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("SYMBOLS")
                .small()
                .strong()
                .color(theme::TEXT_3),
        );
        if let Some(entry) = &self.project_entry {
            ui.label(
                RichText::new(format!(
                    "Entry: {}",
                    entry.file_name().unwrap_or_default().to_string_lossy()
                ))
                .small()
                .color(MUTED),
            )
            .on_hover_text(entry.display().to_string());
        }
        let mut location = None;
        egui::ScrollArea::vertical()
            .id_salt("project-symbols")
            .show(ui, |ui| {
                if let Some(doc) = self.docs.get_mut(self.active) {
                    for function in &doc.functions {
                        if ui
                            .selectable_label(
                                false,
                                RichText::new(format!("ƒ  {}", function.signature)).monospace(),
                            )
                            .clicked()
                        {
                            location = Some((function.path.clone(), function.span.start));
                        }
                    }
                    if let Some(error) = &doc.diagnostic {
                        ui.add_space(14.0);
                        ui.label(RichText::new("PROBLEM").small().color(theme::DANGER));
                        if let Some(path) = &error.path {
                            ui.label(RichText::new(path.display().to_string()).small());
                        }
                        if ui
                            .link(format!(
                                "{}:{}  {}",
                                error.line, error.column, error.message
                            ))
                            .clicked()
                        {
                            location = Some((error.path.clone(), error.span.start));
                        }
                    }
                }
            });
        if let Some((path, offset)) = location {
            self.open_location(path, offset);
        }
    }
    fn audit_panel(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("PERMISSION AUDIT")
                .small()
                .strong()
                .color(theme::TEXT_3),
        );
        ui.label(
            RichText::new("Журнал содержит только метаданные действий; содержимое файлов и секреты не записываются.")
                .small()
                .color(MUTED),
        );
        if ui.button("Copy redacted TSV").clicked() {
            ui.ctx().copy_text(self.permission_ledger.redacted_tsv());
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for entry in self.permission_ledger.entries() {
                ui.horizontal_wrapped(|ui| {
                    ui.monospace(
                        RichText::new(format!("#{}", entry.request_id))
                            .size(12.0)
                            .color(theme::TEXT_3),
                    );
                    ui.label(
                        RichText::new(format!("{} → {}", entry.actor, entry.target)).size(12.0),
                    );
                    ui.label(
                        RichText::new(format!("{:?} / {:?}", entry.capability, entry.decision))
                            .size(12.0)
                            .color(theme::TEXT_2),
                    );
                    ui.label(RichText::new(&entry.result).size(12.0));
                    ui.label(
                        RichText::new(format!("{} ms", entry.duration.as_millis()))
                            .size(12.0)
                            .color(theme::TEXT_3),
                    );
                });
                ui.separator();
            }
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
        let ctx = ui.ctx().clone();
        let completion_state = self.completion.clone();
        let mut completion_replace: Option<Option<CompletionState>> = None;
        let mut accept: Option<usize> = None;
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
                            .color(theme::GUTTER),
                    );
                    let mut layouter = highlighting::layout;
                    let output = egui::TextEdit::multiline(&mut doc.text)
                        .id(id)
                        .code_editor()
                        .font(FontId::monospace(15.0))
                        .frame(false)
                        .background_color(Color32::TRANSPARENT)
                        .desired_width(ui.available_width().max(600.0))
                        .desired_rows(24)
                        .char_limit(aegis_lang::MAX_SOURCE)
                        .layouter(&mut layouter)
                        .show(ui);
                    if output.response.changed() {
                        doc.changed();
                        let byte_at = Self::caret_byte_offset(
                            &doc.text,
                            output.cursor_range.map_or(0, |range| range.primary.index),
                        );
                        completion_replace = Some(Self::fresh_completion(
                            &doc.text,
                            doc.functions.as_slice(),
                            byte_at,
                        ));
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
                    let shown = completion_replace
                        .as_ref()
                        .and_then(|inner| inner.as_ref())
                        .or(completion_state.as_ref());
                    if let Some(state) = shown
                        && !state.items.is_empty()
                    {
                        let row_height = ui.fonts_mut(|f| f.row_height(&FontId::monospace(15.0)));
                        let y = (output.response.rect.top() + doc.cursor.0 as f32 * row_height)
                            .min(output.response.rect.bottom() - 8.0);
                        egui::Area::new(Id::new("completion-popup"))
                            .fixed_pos(egui::pos2(output.response.rect.left() + 28.0, y))
                            .order(egui::Order::Foreground)
                            .show(ui.ctx(), |ui| {
                                egui::Frame::popup(ui.style()).show(ui, |ui| {
                                    for (index, item) in state.items.iter().enumerate() {
                                        let kind = match item.kind {
                                            completion::Kind::Keyword => "kw",
                                            completion::Kind::Type => "type",
                                            completion::Kind::Function => "fn",
                                            completion::Kind::Buffer => "var",
                                        };
                                        let row = ui.selectable_label(
                                            index == state.selected,
                                            RichText::new(format!("{kind}  {}", item.label))
                                                .monospace()
                                                .size(13.0),
                                        );
                                        if row.clicked() {
                                            accept = Some(index);
                                        }
                                    }
                                });
                            });
                    }
                });
            });
        if let Some(replace) = completion_replace {
            self.completion = replace;
        }
        if let Some(index) = accept {
            self.accept_completion(&ctx, index);
        }
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
                ui.colored_label(theme::DANGER, error);
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
            if let Some(request_id) = self.active_save_permission.take() {
                let duration = self
                    .active_save_started
                    .take()
                    .map_or(Duration::ZERO, |started| started.elapsed());
                self.permission_ledger
                    .finish(request_id, "canceled", duration);
            }
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
        let key_target = key_path.display().to_string();
        self.active_key_permission = Some(self.permission_ledger.request(
            "studio",
            permissions::Capability::ReadWorkspace,
            &key_target,
            &permissions::parameter_hash(&key_target),
            permissions::Decision::Allow,
            "user selected protected-source key",
        ));
        self.active_key_started = Some(Instant::now());
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
    let sky_preview = std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--sky-preview");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 850.0])
            .with_min_inner_size([900.0, 600.0])
            // The sky preview draws its own minimal window controls over the
            // background; the IDE keeps the native frame.
            .with_decorations(!sky_preview),
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(
        "Sentinel",
        options,
        Box::new(|cc| Ok(Box::new(Studio::new(cc)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn analysis_error_is_routed_to_its_own_document() {
        let dir = tempfile::tempdir().expect("temp directory");
        let entry = dir.path().join("main.ae");
        let child = dir.path().join("helper.ae");
        std::fs::write(&entry, "mod helper; fn main() {}").expect("entry");
        std::fs::write(&child, "fn value() { return missing; }").expect("child");
        let mut app = Studio::default();
        app.open_path(entry);
        app.open_path(child.clone());
        // Studio::default() already carries the untitled sample buffer.
        assert_eq!(app.docs.len(), 3);
        // open_path canonicalizes; project diagnostics use canonical paths too.
        let error = Diagnostic {
            message: "unknown name `missing`".into(),
            path: Some(child.canonicalize().expect("canonical")),
            span: 0..1,
            line: 1,
            column: 1,
        };
        app.apply_analysis(1, 0, Err(error));
        assert!(app.docs[0].diagnostic.is_none(), "sample stays clean");
        assert!(app.docs[1].diagnostic.is_none(), "entry stays clean");
        let routed = app.docs[2]
            .diagnostic
            .as_ref()
            .expect("routed to helper.ae");
        assert_eq!(routed.message, "unknown name `missing`");
        app.apply_analysis(1, 0, Ok(Vec::new()));
        assert!(app.docs.iter().all(|doc| doc.diagnostic.is_none()));
    }
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
        theme::install(&ctx);
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
