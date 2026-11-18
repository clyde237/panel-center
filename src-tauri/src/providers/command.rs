use crate::domain::AppError;
use std::{
    io::{self, Read},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const MAX_OUTPUT: u64 = 16 * 1024 * 1024;

// Réservé aux exécutables et arguments fixes des connecteurs, jamais exposé par IPC.
pub fn read_command(program: &str, args: &[&str]) -> Result<String, AppError> {
    let mut child = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .env("FLATPAK_FANCY_OUTPUT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            AppError::new(
                if error.kind() == io::ErrorKind::NotFound {
                    "MANAGER_UNAVAILABLE"
                } else {
                    "MANAGER_ACCESS"
                },
                "Le gestionnaire n’est pas disponible ou son accès est refusé.",
            )
        })?;
    let stdout = child.stdout.take().expect("stdout redirigée");
    let stderr = child.stderr.take().expect("stderr redirigée");
    let read = |stream: Box<dyn Read + Send>| {
        thread::spawn(move || {
            let mut data = Vec::new();
            stream
                .take(MAX_OUTPUT + 1)
                .read_to_end(&mut data)
                .map(|_| data)
        })
    };
    let out = read(Box::new(stdout));
    let err = read(Box::new(stderr));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if start.elapsed() < Duration::from_secs(8) => {
                thread::sleep(Duration::from_millis(15))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(AppError::new(
                    "MANAGER_TIMEOUT",
                    "Le gestionnaire n’a pas répondu dans le délai prévu.",
                ));
            }
        }
    };
    let out = out.join().ok().and_then(Result::ok);
    let _ = err.join();
    if !status?.success() {
        return Err(AppError::new(
            "MANAGER_QUERY_FAILED",
            "Le gestionnaire a refusé la lecture de l’inventaire.",
        ));
    }
    let out = out
        .filter(|bytes| bytes.len() as u64 <= MAX_OUTPUT)
        .ok_or_else(|| {
            AppError::new(
                "MANAGER_OUTPUT",
                "La réponse du gestionnaire est trop volumineuse ou illisible.",
            )
        })?;
    String::from_utf8(out).map_err(|_| {
        AppError::new(
            "MANAGER_ENCODING",
            "L’encodage de l’inventaire est inattendu.",
        )
    })
}
