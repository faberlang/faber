//! `faber explain` — language reference lookup.

use crate::cli::ExplainArgs;
use faber_cli::diagnostic_explain;
use faber_cli::explain::{self, Registry};

/// Renders explain-corpus entries with CLI policies around mutually exclusive modes.
pub(super) fn cmd_explain(args: ExplainArgs) {
    if args.list {
        let registry = load_registry();
        print!("{}", explain::render_list(&registry));
        return;
    }

    if let Some(category) = args.category {
        let registry = load_registry();
        match explain::render_category(&registry, &category) {
            Some(output) => print!("{output}"),
            None => {
                eprintln!("error: no explanations found in category '{category}'");
                let categories = registry
                    .categories()
                    .into_iter()
                    .collect::<Vec<_>>()
                    .join(", ");
                eprintln!("hint: available categories: {categories}");
                std::process::exit(1);
            }
        }
        return;
    }

    if let Some(query) = args.search {
        if args.json {
            eprintln!("error: --json cannot be combined with --search");
            std::process::exit(2);
        }
        if args.term.is_some() {
            eprintln!("error: --search cannot be combined with a term");
            std::process::exit(2);
        }

        let registry = load_registry();
        let hits = registry.search(&query);
        if hits.is_empty() {
            eprintln!("error: no matches found for '{query}'");
            eprintln!("hint: run `faber explain --list`");
            std::process::exit(1);
        }

        print!("{}", explain::render_search(&query, &hits));
        return;
    }

    let Some(term) = args.term else {
        eprintln!("error: no explain query was provided");
        eprintln!();
        eprintln!("Usage:");
        eprintln!("    faber explain <term>");
        eprintln!("    faber explain --list");
        eprintln!("    faber explain --category <category>");
        eprintln!("    faber explain --search <query>");
        eprintln!();
        eprintln!("Examples:");
        eprintln!("    faber explain functio");
        eprintln!("    faber explain ≡");
        eprintln!("    faber explain ==");
        eprintln!("    faber explain --list");
        std::process::exit(2);
    };

    if diagnostic_explain::is_diagnostic_query(&term) {
        match diagnostic_explain::lookup_installed_diagnostic(&term, args.reader_locale.as_deref())
        {
            Ok(Some(explanation)) if args.json => {
                match diagnostic_explain::render_json(&explanation) {
                    Ok(json) => {
                        println!("{json}");
                        return;
                    }
                    Err(err) => {
                        eprintln!("error: {err}");
                        std::process::exit(1);
                    }
                }
            }
            Ok(Some(explanation)) => {
                print!("{}", diagnostic_explain::render_plain(&explanation));
                return;
            }
            Ok(None) => {
                eprintln!("error: no diagnostic explanation found for '{term}'");
                eprintln!("hint: check the diagnostic code and issue spelling");
                std::process::exit(1);
            }
            Err(err) => {
                eprintln!("error: failed to load diagnostic explanations: {err}");
                std::process::exit(1);
            }
        }
    }

    let registry = load_registry();
    let Some(lookup) = registry.lookup(&term) else {
        eprintln!("error: no explanation found for '{term}'");
        eprintln!("hint: run `faber explain --list`");
        std::process::exit(1);
    };

    if args.json {
        match explain::render_json(&lookup) {
            Ok(json) => println!("{json}"),
            Err(err) => {
                eprintln!("error: {err}");
                std::process::exit(1);
            }
        }
    } else {
        print!("{}", explain::render_plain(&lookup));
    }
}

fn load_registry() -> Registry {
    let registry = match Registry::load_from_disk() {
        Ok(registry) => registry,
        Err(err) => {
            eprintln!("error: failed to load reference pack: {err}");
            std::process::exit(1);
        }
    };

    if let Some(warning) = registry.version_warning() {
        eprintln!("warning: {warning}");
    }

    registry
}
