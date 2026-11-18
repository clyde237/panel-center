use super::ProviderInventory;
use crate::domain::{AppError, ApplicationInstallation, PackageFormat};
use serde_json::Value;
use std::io::{Read, Write};
use std::time::Duration;

pub fn inventory() -> Result<ProviderInventory, AppError> {
    #[cfg(unix)]
    {
        use std::os::unix::net::UnixStream;
        let mut socket = UnixStream::connect("/run/snapd.socket").map_err(|_| {
            AppError::new(
                "SNAP_UNAVAILABLE",
                "Le service local est absent ou inaccessible.",
            )
        })?;
        socket.set_read_timeout(Some(Duration::from_secs(8))).ok();
        socket.set_write_timeout(Some(Duration::from_secs(2))).ok();
        // HTTP/1.0 impose une réponse délimitée par fermeture, sans chunks HTTP/1.1.
        socket
            .write_all(b"GET /v2/snaps HTTP/1.0\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .map_err(|_| {
                AppError::new("SNAP_ACCESS", "La lecture du service local a été refusée.")
            })?;
        let mut response = Vec::new();
        socket
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut response)
            .map_err(|_| {
                AppError::new(
                    "SNAP_TIMEOUT",
                    "Le service local n’a pas répondu dans le délai prévu.",
                )
            })?;
        if response.len() > 16 * 1024 * 1024 {
            return Err(AppError::new(
                "SNAP_OUTPUT",
                "La réponse Snap est trop volumineuse.",
            ));
        }
        let separator = response
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .ok_or_else(|| {
                AppError::new("SNAP_HTTP", "La réponse du service Snap est invalide.")
            })?;
        let header = String::from_utf8_lossy(&response[..separator]);
        if header
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            != Some("200")
        {
            return Err(AppError::new(
                "SNAP_HTTP",
                "Le service Snap a refusé la lecture de l’inventaire.",
            ));
        }
        parse(&response[separator + 4..])
    }
    #[cfg(not(unix))]
    Err(AppError::new(
        "SNAP_UNAVAILABLE",
        "Snap nécessite un système Linux compatible.",
    ))
}

fn parse(body: &[u8]) -> Result<ProviderInventory, AppError> {
    let data: Value = serde_json::from_slice(body)
        .map_err(|_| AppError::new("SNAP_JSON", "L’inventaire Snap est illisible."))?;
    let rows = data
        .get("result")
        .and_then(Value::as_array)
        .filter(|_| {
            data.get("type").and_then(Value::as_str) == Some("sync")
                && data.get("status-code").and_then(Value::as_u64) == Some(200)
        })
        .ok_or_else(|| {
            AppError::new("SNAP_RESPONSE", "Snap n’a pas fourni un inventaire valide.")
        })?;
    let mut result = ProviderInventory::default();
    for row in rows {
        let Some(name) = row
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
        else {
            return Err(AppError::new(
                "SNAP_ID",
                "Une installation Snap n’a pas d’identifiant valide.",
            ));
        };
        if !matches!(
            row.get("status").and_then(Value::as_str),
            Some("active" | "installed")
        ) {
            continue;
        }
        let mut item = ApplicationInstallation::read_only(
            PackageFormat::Snap,
            format!("snap:system:{name}"),
            name.into(),
            row.get("title")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
                .unwrap_or(name)
                .into(),
        );
        item.version = row
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into();
        item.description = row
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into();
        item.installed_size = row.get("installed-size").and_then(Value::as_u64);
        item.installation = Some("system".into());
        item.reference = row.get("revision").and_then(|value| match value {
            Value::String(text) => Some(text.clone()),
            Value::Number(number) => Some(number.to_string()),
            _ => None,
        });
        item.is_desktop_application = !matches!(
            row.get("type").and_then(Value::as_str),
            Some("base" | "core" | "kernel" | "gadget" | "snapd")
        );
        item.can_uninstall = false;
        item.unavailable_reason =
            "La désinstallation d’un paquet Snap requiert des privilèges administrateur.".into();
        result.installations.push(item);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_apps_and_bases_without_confusing_parallel_instances() {
        let body = br#"{"type":"sync","status-code":200,"result":[{"name":"editor","title":"Editor","status":"active","type":"app","revision":"12","installed-size":1024},{"name":"editor_beta","status":"active","type":"app","revision":13},{"name":"core24","status":"active","type":"base"}]}"#;
        let items = parse(body).unwrap().installations;
        assert_eq!(items.len(), 3);
        assert_ne!(items[0].id, items[1].id);
        assert_eq!(items[0].reference.as_deref(), Some("12"));
        assert_eq!(items[1].reference.as_deref(), Some("13"));
        assert!(!items[2].is_desktop_application);
    }

    #[test]
    fn rejects_daemon_errors_and_missing_names() {
        assert!(
            parse(br#"{"type":"error","status-code":500,"result":{"message":"failure"}}"#).is_err()
        );
        assert!(parse(br#"{"type":"sync","status-code":200,"result":[{}]}"#).is_err());
    }
}
