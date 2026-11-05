use crate::Event;
use std::{
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
    thread,
    time::{Duration, Instant},
};

pub fn run(path: PathBuf, id: u64, stop: Arc<AtomicBool>, events: SyncSender<Event>) {
    thread::spawn(move || {
        let mut command = Command::new(&path);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(parent) = path.parent() {
            command.current_dir(parent);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = match command.spawn() {
            Ok(c) => c,
            Err(e) => {
                let _ = events.send(Event::Finished(id, Err(e.to_string())));
                return;
            }
        };
        let readers: Vec<_> = [
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
        ]
        .into_iter()
        .flatten()
        .map(|mut reader| {
            let events = events.clone();
            thread::spawn(move || {
                let mut chunk = [0u8; 4096];
                loop {
                    match reader.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if events.send(Event::Output(id, chunk[..n].to_vec())).is_err() {
                                break;
                            }
                        }
                    }
                }
            })
        })
        .collect();
        let start = Instant::now();
        let result = loop {
            if stop.load(Ordering::Relaxed) {
                let _ = child.kill();
            }
            match child.try_wait() {
                Ok(Some(status)) => break Ok((status.code().unwrap_or(-1), start.elapsed())),
                Ok(None) => thread::sleep(Duration::from_millis(15)),
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(e.to_string());
                }
            }
        };
        for reader in readers {
            let _ = reader.join();
        }
        let _ = events.send(Event::Finished(id, result));
    });
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn program(source: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("directory");
        let path = dir.path().join("runner.exe");
        let build = aegis_lang::compile(source).expect("compile");
        aegis_lang::storage::atomic_write(&path, &build.executable).expect("write");
        (dir, path)
    }
    #[test]
    fn captures_output_and_exit_code_259() {
        let (_dir, path) = program("fn main() { printi(42); return 259; }");
        let (send, receive) = mpsc::sync_channel(64);
        let stop = Arc::new(AtomicBool::new(false));
        run(path, 7, stop, send);
        let mut output = Vec::new();
        loop {
            match receive
                .recv_timeout(Duration::from_secs(10))
                .expect("runner completion")
            {
                Event::Output(7, bytes) => output.extend_from_slice(&bytes),
                Event::Finished(7, Ok((code, _))) => {
                    assert_eq!(code, 259);
                    break;
                }
                Event::Finished(_, Err(e)) => panic!("runner failed: {e}"),
                _ => {}
            }
        }
        assert_eq!(output, b"42\n");
    }
    #[test]
    fn stops_its_own_running_child() {
        let (_dir, path) = program("fn main() { while(1) {} return 0; }");
        let (send, receive) = mpsc::sync_channel(64);
        let stop = Arc::new(AtomicBool::new(true));
        run(path, 8, stop, send);
        loop {
            match receive
                .recv_timeout(Duration::from_secs(10))
                .expect("stop completion")
            {
                Event::Finished(8, Ok(_)) => break,
                Event::Finished(_, Err(e)) => panic!("runner failed: {e}"),
                _ => {}
            }
        }
    }
}
