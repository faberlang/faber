//! Case selection for `faber corpus`: the union of `--domain`, `--category`,
//! `--case` and `--filter`.
//!
//! The selectors only ever add. A case is selected when any one selector names
//! it; there is no intersection and no exclusion. With no selector, every case is
//! selected. A suite carries its own partition in `corpus.toml` `[domains]`:
//! each domain lists its categories, and each category belongs to exactly one
//! domain, so the domains together select exactly the suite.

use super::Case;
use std::collections::{BTreeMap, BTreeSet};

/// A suite's closed domain table: domain name to its categories.
pub(super) struct DomainTable {
    domains: BTreeMap<String, Vec<String>>,
    category_domain: BTreeMap<String, String>,
}

impl DomainTable {
    /// Validate the `[domains]` table. An empty table is no table.
    pub(super) fn from_manifest(
        domains: BTreeMap<String, Vec<String>>,
    ) -> Result<Option<Self>, String> {
        if domains.is_empty() {
            return Ok(None);
        }
        let mut category_domain = BTreeMap::new();
        for (domain, categories) in &domains {
            if categories.is_empty() {
                return Err(format!("domain {domain:?} lists no categories"));
            }
            for category in categories {
                if let Some(previous) = category_domain.insert(category.clone(), domain.clone()) {
                    return Err(format!(
                        "category {category:?} is in more than one domain: {previous:?} and {domain:?}"
                    ));
                }
            }
        }
        Ok(Some(Self {
            domains,
            category_domain,
        }))
    }

    #[cfg(test)]
    pub(super) fn domain_names(&self) -> Vec<String> {
        self.domains.keys().cloned().collect()
    }

    pub(super) fn has_category(&self, category: &str) -> bool {
        self.category_domain.contains_key(category)
    }

    /// Check every case's tags against the table and fill each case's domain.
    pub(super) fn tag_cases(table: Option<&Self>, cases: &mut [Case]) -> Result<(), String> {
        for case in cases {
            let Some(table) = table else {
                if case.category.is_some() || case.domain.is_some() {
                    return Err(format!(
                        "{}: category/domain need a [domains] table in corpus.toml",
                        case.id
                    ));
                }
                continue;
            };
            let category = case
                .category
                .as_deref()
                .ok_or_else(|| format!("{}: case has no category", case.id))?;
            let domain = table
                .category_domain
                .get(category)
                .ok_or_else(|| format!("{}: category {category:?} is in no domain", case.id))?;
            if case
                .domain
                .as_deref()
                .is_some_and(|written| written != domain)
            {
                return Err(format!(
                    "{}: domain {:?} disagrees with category {category:?}, which is in {domain:?}",
                    case.id,
                    case.domain.as_deref().unwrap_or_default()
                ));
            }
            case.domain = Some(domain.clone());
        }
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct Selectors {
    pub domains: Vec<String>,
    pub categories: Vec<String>,
    pub cases: Vec<String>,
    pub filter: Option<String>,
}

impl Selectors {
    pub(super) fn is_empty(&self) -> bool {
        self.domains.is_empty()
            && self.categories.is_empty()
            && self.cases.is_empty()
            && self.filter.is_none()
    }
}

pub(super) enum SelectionError {
    /// A selector the suite cannot honour: a bad argument.
    Usage(String),
    /// Every selector was valid but together they named no case.
    Empty(String),
}

/// Select the union of the named domains, categories, case ids and filter
/// substring, keeping suite order.
pub(super) fn select(
    cases: Vec<Case>,
    table: Option<&DomainTable>,
    selectors: &Selectors,
) -> Result<Vec<Case>, SelectionError> {
    if selectors.is_empty() {
        return Ok(cases);
    }
    if !(selectors.domains.is_empty() && selectors.categories.is_empty()) && table.is_none() {
        return Err(SelectionError::Usage(
            "--domain and --category need a suite whose corpus.toml defines [domains]".to_owned(),
        ));
    }
    if let Some(table) = table {
        for domain in &selectors.domains {
            if !table.domains.contains_key(domain) {
                return Err(SelectionError::Usage(format!("unknown domain {domain:?}")));
            }
        }
        for category in &selectors.categories {
            if !table.category_domain.contains_key(category) {
                return Err(SelectionError::Usage(format!(
                    "unknown category {category:?}"
                )));
            }
        }
    }
    let known: BTreeSet<&str> = cases.iter().map(|case| case.id.as_str()).collect();
    for id in &selectors.cases {
        if !known.contains(id.as_str()) {
            return Err(SelectionError::Usage(format!("unknown case id {id:?}")));
        }
    }
    let wanted_domains: BTreeSet<&str> = selectors.domains.iter().map(String::as_str).collect();
    let wanted_categories: BTreeSet<&str> =
        selectors.categories.iter().map(String::as_str).collect();
    let wanted_cases: BTreeSet<&str> = selectors.cases.iter().map(String::as_str).collect();
    let selected: Vec<Case> = cases
        .into_iter()
        .filter(|case| {
            case.domain
                .as_deref()
                .is_some_and(|domain| wanted_domains.contains(domain))
                || case
                    .category
                    .as_deref()
                    .is_some_and(|category| wanted_categories.contains(category))
                || wanted_cases.contains(case.id.as_str())
                || selectors
                    .filter
                    .as_deref()
                    .is_some_and(|filter| case.id.contains(filter))
        })
        .collect();
    if selected.is_empty() {
        return Err(SelectionError::Empty(
            "no cases match the selectors".to_owned(),
        ));
    }
    Ok(selected)
}
