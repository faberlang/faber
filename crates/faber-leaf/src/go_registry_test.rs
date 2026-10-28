use std::collections::BTreeSet;
use std::path::Path;

#[test]
fn go_registry_matches_leaf_routes() {
    let runtime_go = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../runtime/go");
    if !runtime_go.is_dir() {
        eprintln!(
            "skipping Go leaf registry parity: {} is absent",
            runtime_go.display()
        );
        return;
    }

    let go_files = [
        "leaf_stdio.go",
        "leaf_stdio_write.go",
        "leaf_process.go",
        "leaf_clock.go",
        "leaf_entropy.go",
        "leaf_fs.go",
        "leaf_fs_path.go",
        "leaf_tcp.go",
        "leaf_http_client.go",
        "leaf_http_server.go",
        "leaf_websocket.go",
        "leaf_machine.go",
    ];
    let mut go_routes = BTreeSet::new();
    for file in go_files {
        let path = runtime_go.join(file);
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        for line in source.lines() {
            if let Some(route) = line
                .trim()
                .strip_prefix("registerLeaf(\"")
                .and_then(|registration| registration.split_once('"').map(|(route, _)| route))
            {
                go_routes.insert(route.to_owned());
            }
        }
    }

    let rust_routes = crate::stdio::ROUTES
        .iter()
        .chain(crate::process::ROUTES)
        .chain(crate::clock::ROUTES)
        .chain(crate::entropy::ROUTES)
        .chain(crate::fs::ROUTES)
        .chain(crate::tcp::ROUTES)
        .chain(crate::http::ROUTES)
        .chain(crate::websocket::ROUTES)
        .chain(crate::machine::ROUTES)
        .map(|route| (*route).to_owned())
        .collect::<BTreeSet<_>>();

    assert_eq!(go_routes, rust_routes);
}
