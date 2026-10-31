use std::collections::{HashMap, HashSet};
use std::path::Path;

#[path = "../src/backend/reverse_rpc.rs"]
mod reverse_rpc;

fn methods_in_calls(directory: &Path, methods: &mut HashSet<String>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            methods_in_calls(&path, methods);
        } else if path.extension().is_some_and(|extension| extension == "ts") {
            let text = std::fs::read_to_string(&path).unwrap();
            for call in text.split("requestClient(").skip(1) {
                let argument = call.split(',').next().unwrap().trim();
                let argument = argument.split(')').next().unwrap().trim();
                if let Some(id) = argument.strip_prefix("zcodeProtocolMethods.") {
                    assert!(
                        id.chars().all(|c| c.is_alphanumeric() || c == '_'),
                        "unclassified call in {}",
                        path.display()
                    );
                    methods.insert(id.into());
                } else if path.file_name().is_some_and(|name| {
                    name == "server.ts" || name == "automation-port.ts" || name == "offpeak-port.ts"
                }) {
                    continue;
                } else {
                    panic!(
                        "Reverse RPC must use explicit protocol registry method: {}: {argument}",
                        path.display()
                    );
                }
            }
        }
    }
}

#[test]
fn current_cli_reverse_rpc_calls_have_explicit_gpui_responses() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let registry =
        std::fs::read_to_string(root.join("packages/shared/src/zcode-protocol/index.ts")).unwrap();
    let registry = registry
        .split("export const zcodeProtocolMethods = {")
        .nth(1)
        .unwrap()
        .split("} as const;")
        .next()
        .unwrap();
    let mapping: HashMap<&str, &str> = registry
        .lines()
        .filter_map(|line| {
            let (key, value) = line.trim().split_once(": \"")?;
            Some((key, value.split('"').next()?))
        })
        .collect();
    let mut methods = HashSet::new();
    methods_in_calls(
        &root.join("apps/zcode-cli/packages/bootstrap/src"),
        &mut methods,
    );
    assert!(
        methods.len() >= 14,
        "inventory unexpectedly empty or truncated"
    );
    for id in methods {
        let method = mapping
            .get(id.as_str())
            .unwrap_or_else(|| panic!("Unknown protocol constant: {id}"));
        match reverse_rpc::dispatch_reverse_rpc(
            &serde_json::json!(1),
            method,
            &serde_json::json!({}),
            reverse_rpc::HostPrefs::default(),
        ) {
            reverse_rpc::ReverseRpcAction::Raced => {}
            reverse_rpc::ReverseRpcAction::Respond(line) => {
                let response: serde_json::Value = serde_json::from_str(&line).unwrap();
                assert_ne!(
                    response["error"]["code"], -32601,
                    "Unclassified CLI reverse RPC: {method}"
                );
            }
        }
    }
}
