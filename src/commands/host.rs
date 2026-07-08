//! `faber host` — script kernel introspection for agents.

use clap::Args;
use radix::kernel::{kernel_manifest_entries, KERNEL_MANIFEST_VERSION};
use serde::Serialize;

/// Arguments for `faber host`.
#[derive(Args, Debug)]
pub struct HostArgs {
    #[command(subcommand)]
    pub command: HostCommand,
}

/// `faber host` subcommands.
#[derive(clap::Subcommand, Debug)]
pub enum HostCommand {
    /// List v1 kernel modules and verbs from the compile-time manifest.
    Manifest(ManifestArgs),
}

/// Arguments for `faber host manifest`.
#[derive(Args, Debug)]
pub struct ManifestArgs {
    /// Emit stable JSON for agents.
    #[arg(long)]
    pub json: bool,
}

#[derive(Serialize)]
struct ManifestModule {
    name: &'static str,
    import_path: &'static str,
    provider_module: &'static str,
    verbs: &'static [&'static str],
    since: &'static str,
}

#[derive(Serialize)]
struct HostManifest {
    version: &'static str,
    modules: Vec<ManifestModule>,
}

pub(super) fn cmd_host(command: HostCommand) {
    match command {
        HostCommand::Manifest(args) => cmd_host_manifest(args),
    }
}

fn cmd_host_manifest(args: ManifestArgs) {
    let manifest = HostManifest {
        version: KERNEL_MANIFEST_VERSION,
        modules: kernel_manifest_entries()
            .iter()
            .map(|entry| ManifestModule {
                name: entry.module.name(),
                import_path: entry.import_path,
                provider_module: entry.provider_module,
                verbs: entry.verbs,
                since: entry.since,
            })
            .collect(),
    };

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&manifest).expect("serialize host manifest")
        );
        return;
    }

    println!("faber script kernel manifest (v{})", manifest.version);
    for module in &manifest.modules {
        println!("  {}", module.import_path);
        println!("    provider: {}", module.provider_module);
        println!("    since: {}", module.since);
        println!("    verbs: {}", module.verbs.join(", "));
    }
}
