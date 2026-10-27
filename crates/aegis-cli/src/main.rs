use aegis_lang::{codec, compile, storage};
use std::{
    ffi::OsString,
    io::Write,
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

fn main() {
    if let Err(error) = execute(std::env::args_os().skip(1).collect()) {
        eprintln!("{error}");
        std::process::exit(2);
    }
}
fn usage() -> String {
    "AEGIS 0.2\n  aegis-cli build <source.ae|source.aeg> [out.exe] [-k keyfile -c dialect]\n  aegis-cli check <source.ae>\n  aegis-cli enc <source.ae> <out.aeg> -k keyfile -c dialect\n  aegis-cli dec <source.aeg> <out.ae> -k keyfile -c dialect\n  aegis-cli keygen <new-key-file>\n  dialect: decimal or 0x000..0xFFF".into()
}
fn execute(args: Vec<OsString>) -> Result<(), String> {
    if args.is_empty() || args[0] == "--help" || args[0] == "help" {
        println!("{}", usage());
        return Ok(());
    }
    let command = args[0].to_str().ok_or_else(usage)?;
    let mut paths = Vec::new();
    let mut key_path = None;
    let mut dialect = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].to_str() {
            Some("-k") => {
                i += 1;
                key_path = Some(PathBuf::from(args.get(i).ok_or_else(usage)?));
            }
            Some("-c") => {
                i += 1;
                let text = args.get(i).and_then(|s| s.to_str()).ok_or_else(usage)?;
                let parsed = if text.starts_with("0x") || text.starts_with("0X") {
                    u16::from_str_radix(&text[2..], 16)
                } else {
                    text.parse()
                };
                dialect = Some(
                    parsed
                        .ok()
                        .filter(|&n| n < 4096)
                        .ok_or("dialect must be in 0..4095")?,
                );
            }
            Some(s) if s.starts_with('-') => return Err(usage()),
            _ => paths.push(PathBuf::from(&args[i])),
        }
        i += 1;
    }
    if command == "keygen" {
        if paths.len() != 1 || key_path.is_some() || dialect.is_some() {
            return Err(usage());
        }
        let key = codec::generate_key()?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&paths[0])
            .map_err(|e| e.to_string())?;
        file.write_all(&*key)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        println!("OK: new 32-byte key created");
        return Ok(());
    }
    if !matches!(command, "build" | "check" | "enc" | "dec") || paths.is_empty() || paths.len() > 2
    {
        return Err(usage());
    }
    if (command == "enc" || command == "dec") && paths.len() != 2 {
        return Err(usage());
    }
    if command == "check" && paths.len() != 1 {
        return Err(usage());
    }
    let input = Zeroizing::new(
        storage::read_bounded(&paths[0], aegis_lang::MAX_SOURCE + codec::OVERHEAD)
            .map_err(|e| e.to_string())?,
    );
    let key = match key_path {
        Some(path) => Some(Zeroizing::new(
            storage::read_bounded(&path, codec::MAX_KEY).map_err(|e| e.to_string())?,
        )),
        None => None,
    };
    let credentials = || -> Result<(&[u8], u16), String> {
        Ok((
            key.as_deref()
                .map(|v| v.as_slice())
                .ok_or("key file required (-k)")?,
            dialect.ok_or("dialect required (-c)")?,
        ))
    };
    let source = if command == "dec" || (command != "enc" && codec::is_protected(&input)) {
        let (key, dialect) = credentials()?;
        codec::open(&input, key, dialect)?
    } else {
        if input.starts_with(b"AEG0") {
            return Err("legacy AEG0: decode with the legacy compiler before migrating".into());
        }
        input
    };
    let output = match command {
        "build" => {
            let text = std::str::from_utf8(&source).map_err(|_| "source must be UTF-8")?;
            let build = compile(text).map_err(|e| e.to_string())?;
            println!(
                "Compiled {} functions, {} bytes of native code",
                build.functions.len(),
                build.code_bytes
            );
            build.executable
        }
        "check" => {
            compile(std::str::from_utf8(&source).map_err(|_| "source must be UTF-8")?)
                .map_err(|e| e.to_string())?;
            println!("OK: source checked");
            return Ok(());
        }
        "enc" => {
            let (key, dialect) = credentials()?;
            codec::seal(&source, key, dialect)?
        }
        "dec" => source.to_vec(),
        _ => return Err(usage()),
    };
    let destination = paths
        .get(1)
        .cloned()
        .unwrap_or_else(|| paths[0].with_extension("exe"));
    // Never allow a build or conversion to replace the input by accident.
    if same_file(&paths[0], &destination) {
        return Err("output path must differ from input".into());
    }
    storage::atomic_write(&destination, &output).map_err(|e| e.to_string())?;
    println!("OK: {}", destination.display());
    Ok(())
}
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}
