//! `faber explain` — language reference lookup.

use crate::cli::ExplainArgs;
use faber_cli::diagnostic_explain;
use faber_cli::explain::{self, Registry};

/// Renders explain-corpus entries with CLI policies around mutually exclusive modes.
pub(super) fn cmd_explain(args: ExplainArgs) {
    if args.list {
        let registry = load_registry(args.locale.as_deref());
        if args.json {
            match explain::render_list_json(&registry) {
                Ok(json) => println!("{json}"),
                Err(err) => {
                    eprintln!("error: {err}");
                    faber_cli::exit_status::exit_software();
                }
            }
        } else {
            print!("{}", explain::render_list(&registry));
        }
        return;
    }

    if let Some(category) = args.category {
        let registry = load_registry(args.locale.as_deref());
        if let Some(output) = explain::render_category(&registry, &category) {
            print!("{output}");
        } else {
            eprintln!("error: no explanations found in category '{category}'");
            let categories = registry
                .categories()
                .into_iter()
                .collect::<Vec<_>>()
                .join(", ");
            eprintln!("hint: available categories: {categories}");
            faber_cli::exit_status::exit_usage();
        }
        return;
    }

    if let Some(query) = args.search {
        let registry = load_registry(args.locale.as_deref());
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
        eprintln!("    faber explain --locale <locale> <term>");
        eprintln!("    faber explain --list");
        eprintln!("    faber explain --category <category>");
        eprintln!("    faber explain --search <query>");
        eprintln!();
        eprintln!("Examples:");
        eprintln!("    faber explain fn");
        eprintln!("    faber explain ≡");
        eprintln!("    faber explain ==");
        eprintln!("    faber explain --list");
        faber_cli::exit_status::exit_usage();
    };

    if diagnostic_explain::is_diagnostic_query(&term) {
        match diagnostic_explain::lookup_embedded_diagnostic(
            &term,
            args.diagnostics_locale.as_deref(),
        ) {
            Ok(Some(explanation)) if args.json => {
                match diagnostic_explain::render_json(&explanation) {
                    Ok(json) => {
                        println!("{json}");
                        return;
                    }
                    Err(err) => {
                        eprintln!("error: {err}");
                        faber_cli::exit_status::exit_software();
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
                faber_cli::exit_status::exit_usage();
            }
            Err(err) => {
                eprintln!("error: failed to load diagnostic explanations: {err}");
                faber_cli::exit_status::exit_software();
            }
        }
    }

    let registry = load_registry(args.locale.as_deref());
    let Some(lookup) = registry.lookup(&term) else {
        eprintln!("error: no explanation found for '{term}'");
        if let Some(spelling) = registry.spelling_of_identity(&term) {
            eprintln!("hint: in this reader locale '{term}' is spelled '{spelling}'");
        }
        eprintln!("hint: run `faber explain --list`");
        faber_cli::exit_status::exit_usage();
    };

    if args.json {
        match explain::render_json(&lookup) {
            Ok(json) => println!("{json}"),
            Err(err) => {
                eprintln!("error: {err}");
                faber_cli::exit_status::exit_software();
            }
        }
    } else {
        print!("{}", explain::render_plain(&lookup));
    }
}

fn load_registry(locale: Option<&str>) -> Registry {
    match Registry::load_for_locale(locale) {
        Ok(registry) => registry,
        Err(err) => {
            eprintln!("error: failed to load the embedded reference: {err}");
            faber_cli::exit_status::exit_software();
        }
    }
}
