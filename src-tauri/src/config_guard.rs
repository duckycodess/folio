//! Checks on the shipped desktop configuration.
//!
//! The webview must never be handed general filesystem access, and the app must
//! not reach the network for fonts, analytics or hosted inference. Both are
//! configuration decisions, so they are checked as configuration.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::Value;

    fn read(relative: &str) -> Value {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative);
        let text =
            std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing {}", path.display()));
        serde_json::from_str(&text).expect("valid JSON")
    }

    #[test]
    fn the_webview_capability_grants_no_filesystem_or_shell_access() {
        let capability = read("capabilities/default.json");
        let permissions: Vec<String> = capability["permissions"]
            .as_array()
            .expect("the capability lists its permissions")
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .or_else(|| value["identifier"].as_str().map(str::to_string))
                    .expect("each permission is an identifier")
            })
            .collect();
        assert!(!permissions.is_empty());
        for permission in &permissions {
            let forbidden = permission.starts_with("fs:")
                || permission.starts_with("shell:")
                || permission.starts_with("http:")
                || permission.starts_with("dialog:")
                || permission.starts_with("process:")
                || permission.starts_with("upload:");
            assert!(
                !forbidden,
                "'{permission}' would give the webview access that belongs to native commands"
            );
        }
        assert_eq!(
            capability["windows"].as_array().unwrap(),
            &vec![Value::String("main".into())]
        );
    }

    #[test]
    fn the_content_security_policy_stays_on_the_device() {
        let config = read("tauri.conf.json");
        let security = &config["app"]["security"];
        for key in ["csp", "devCsp"] {
            let policy = security[key]
                .as_str()
                .unwrap_or_else(|| panic!("{key} is configured"));
            assert!(policy.contains("default-src 'self'"), "{key}: {policy}");
            for origin in policy.split_whitespace() {
                if origin.starts_with("http://") || origin.starts_with("ws://") {
                    let local = origin.contains("127.0.0.1")
                        || origin.contains("ipc.localhost")
                        || origin.contains("localhost");
                    assert!(local, "{key} allows a remote origin: {origin}");
                }
                assert!(
                    !origin.starts_with("https://"),
                    "{key} allows a remote origin: {origin}"
                );
            }
        }
    }

    #[test]
    fn the_window_has_a_minimum_size_for_long_filipino_paths() {
        let config = read("tauri.conf.json");
        let window = &config["app"]["windows"][0];
        assert!(window["minWidth"].as_u64().unwrap() >= 640);
        assert!(window["minHeight"].as_u64().unwrap() >= 480);
    }
}
