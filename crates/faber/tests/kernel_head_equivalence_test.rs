//! Corpus-wide equivalence gate for the kernel declaration head (operator
//! ruling 2026-10-30): every `.fab` file under `corpus/`, `corpus-product/`
//! and `acceptance/` that authors a kernel with the `@ kernel` / `@ nucleum`
//! annotation is rewritten to the head form (`kernel name(…) [mode] …`), and
//! both forms must agree:
//!
//! 1. `faber check --no-diagnostics` exits with the same status and the same
//!    `CODE:issue` identity multiset (locations shift by the removed
//!    annotation line, identities must not);
//! 2. for single-file cases, where the annotation form formats, `faber
//!    format --stdout` output is byte-identical — the HIR-driven emitter is
//!    the identity witness.
//!
//! Standalone files are checked in file mode; package members are checked by
//! copying the whole package and rewriting the copies in place, so relative
//! imports resolve in both runs.
//!
//! Identity assertions never match rendered prose.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

const FAMILY_WORDS: [&str; 2] = ["kernel", "nucleum"];
const MODE_WORDS: [&str; 5] = ["relaxed", "unsafe", "laxus", "intutus", "fragment"];
const FN_HEAD_WORDS: [&str; 2] = ["fn", "functio"];
const MODIFIER_WORDS: [&str; 12] = [
    "errata", "errors", "exitus", "exit", "immutata", "readonly", "iacit", "throws", "optiones",
    "options", "argumenta", "args",
];
const MODIFIERS_WITH_ARGUMENT: [&str; 8] = [
    "errata", "errors", "exitus", "exit", "optiones", "options", "argumenta", "args",
];

#[path = "support/temp.rs"]
mod temp;
use temp::TempDir;

fn faber() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_faber"));
    command.stdin(std::process::Stdio::null());
    command
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// One annotation-form kernel site.
struct KernelSite {
    marker_line: usize,
    family_word: String,
    mode: Option<String>,
}

enum Rewrite {
    /// The file carries no annotation-form kernel site.
    NoSite,
    /// The file carries a site this rewriter refuses (its own diagnostics
    /// exempla: an unknown or doubled tier word, a lane binding, a tier on a
    /// non-function declaration, …).
    Refused,
    /// The rewritten source and the number of sites rewritten.
    Done { source: String, sites: usize },
}

/// Classify one annotation line as a kernel marker line.
fn marker_on_line(trimmed: &str) -> Option<Result<(String, Option<String>), ()>> {
    let rest = trimmed.strip_prefix('@')?.trim_start();
    let family = FAMILY_WORDS.iter().find(|word| {
        rest.starts_with(*word) && !rest[(*word).len()..].chars().next().is_some_and(is_word_char)
    })?;
    let tail = rest[family.len()..].trim_start();
    if tail.is_empty() {
        return Some(Ok(((*family).to_owned(), None)));
    }
    if tail.starts_with('#') {
        // A trailing comment rides the annotation line; keep the site
        // untouched rather than guess where the comment belongs.
        return Some(Err(()));
    }
    if let Some(braced) = tail.strip_prefix('{') {
        let Some(body_end) = braced.find('}') else {
            return Some(Err(()));
        };
        let mode = match braced[..body_end].trim() {
            "" => None,
            "fragment = verum" | "fragment = true" => Some("fragment".to_owned()),
            "fragment = falsum" | "fragment = false" => None,
            _ => return Some(Err(())),
        };
        let after = braced[body_end + 1..].trim();
        if !after.is_empty() && !after.starts_with('#') {
            return Some(Err(()));
        }
        return Some(Ok(((*family).to_owned(), mode)));
    }
    let word_len = tail.find(|ch: char| !is_word_char(ch)).unwrap_or(tail.len());
    let word = &tail[..word_len];
    let after = tail[word_len..].trim_start();
    if !after.is_empty() && !after.starts_with('#') {
        return Some(Err(()));
    }
    if MODE_WORDS.contains(&word) {
        return Some(Ok(((*family).to_owned(), Some(word.to_owned()))));
    }
    Some(Err(()))
}

/// The line index of the `fn`/`functio` head the marker at `marker_line`
/// annotates, walking past stacked annotations and comments.
fn head_line_of(lines: &[&str], marker_line: usize) -> Option<usize> {
    let mut k = marker_line + 1;
    loop {
        let line = *lines.get(k)?;
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('@') {
            k += 1;
            continue;
        }
        break;
    }
    let head = lines[k].trim_start();
    let is_head = FN_HEAD_WORDS.iter().any(|word| {
        head.starts_with(word) && !head[(*word).len()..].chars().next().is_some_and(is_word_char)
    });
    is_head.then_some(k)
}

/// Rewrite every annotation-form kernel site of `source` into the head form.
fn rewrite_kernel_annotations(source: &str) -> Rewrite {
    let lines: Vec<&str> = source.lines().collect();
    let mut sites: Vec<KernelSite> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('@') {
            continue;
        }
        match marker_on_line(trimmed) {
            None => continue,
            Some(Err(())) => return Rewrite::Refused,
            Some(Ok((family_word, mode))) => sites.push(KernelSite {
                marker_line: index,
                family_word,
                mode,
            }),
        }
    }
    if sites.is_empty() {
        return Rewrite::NoSite;
    }

    // Head line -> (family word, mode): the site's marker claims it.
    let mut heads: BTreeMap<usize, (String, Option<String>)> = BTreeMap::new();
    for site in &sites {
        let Some(head) = head_line_of(&lines, site.marker_line) else {
            return Rewrite::Refused;
        };
        if heads
            .insert(head, (site.family_word.clone(), site.mode.clone()))
            .is_some()
        {
            return Rewrite::Refused;
        }
    }

    let line_start =
        |line: usize| -> usize { lines[..line].iter().map(|line| line.len() + 1).sum() };

    // Edits: the head word swap, and the mode word in its signature slot.
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for (&head, (family, mode)) in &heads {
        let head_line_text = lines[head];
        let indent = head_line_text.len() - head_line_text.trim_start().len();
        let head_start = line_start(head) + indent;
        let head_len = FN_HEAD_WORDS
            .iter()
            .find_map(|word| source[head_start..].starts_with(word).then(|| word.len()))
            .expect("head word");
        edits.push((head_start, head_start + head_len, family.clone()));
        if let Some(mode) = mode {
            let Some(insert_at) = mode_insert_offset(source, &lines, head) else {
                return Rewrite::Refused;
            };
            edits.push((insert_at, insert_at, format!(" {mode}")));
        }
    }

    // Apply the edits back-to-front; none of them touches a newline.
    edits.sort_by(|left, right| right.0.cmp(&left.0));
    let mut text = source.to_owned();
    for (start, end, replacement) in edits {
        text.replace_range(start..end, &replacement);
    }

    // Drop the marker lines: the edits left every line index unchanged.
    let mut dropped: Vec<usize> = sites.iter().map(|site| site.marker_line).collect();
    dropped.sort_unstable();
    let mut drop_next = dropped.iter().copied().peekable();
    let mut out = String::with_capacity(source.len());
    let mut first = true;
    for (index, line) in text.lines().enumerate() {
        if drop_next.peek().is_some_and(|drop| *drop == index) {
            drop_next.next();
            continue;
        }
        if !first {
            out.push('\n');
        }
        out.push_str(line);
        first = false;
    }
    if source.ends_with('\n') {
        out.push('\n');
    }
    Rewrite::Done {
        source: out,
        sites: sites.len(),
    }
}

/// Offset of the mode slot: just past the parameter list and its modifier
/// run, per the head grammar `")" func_modifier* kernel_mode?`.
fn mode_insert_offset(source: &str, lines: &[&str], head: usize) -> Option<usize> {
    let head_line_text = lines[head];
    let indent = head_line_text.len() - head_line_text.trim_start().len();
    let mut offset = lines[..head].iter().map(|line| line.len() + 1).sum::<usize>() + indent;
    let mut rest = &source[offset..];
    let head_len = FN_HEAD_WORDS
        .iter()
        .find_map(|word| rest.starts_with(word).then(|| word.len()))?;
    rest = &rest[head_len..];
    offset += head_len;
    let mut skip_ws = |rest: &mut &str, offset: &mut usize| {
        let skipped = rest.len() - rest.trim_start().len();
        *rest = &rest[skipped..];
        *offset += skipped;
    };
    skip_ws(&mut rest, &mut offset);
    // The declaration name.
    let name_len = rest.find(|ch: char| !is_word_char(ch)).unwrap_or(rest.len());
    rest = &rest[name_len..];
    offset += name_len;
    skip_ws(&mut rest, &mut offset);
    // Optional generic parameter list.
    if rest.starts_with('<')
        && let Some(close) = balanced(rest, '<', '>')
    {
        rest = &rest[close..];
        offset += close;
        skip_ws(&mut rest, &mut offset);
    }
    if !rest.starts_with('(') {
        return None;
    }
    let close = balanced(rest, '(', ')')?;
    rest = &rest[close..];
    offset += close;
    // The modifier run stays on the parameter list's own line.
    loop {
        let same_line = rest.split('\n').next().unwrap_or("");
        let trimmed = same_line.trim_start();
        if trimmed.is_empty() {
            break;
        }
        let word_len = trimmed.find(|ch: char| !is_word_char(ch)).unwrap_or(trimmed.len());
        let word = &trimmed[..word_len];
        if !MODIFIER_WORDS.contains(&word) {
            break;
        }
        let consumed = same_line.len() - trimmed.len() + word_len;
        rest = &rest[consumed..];
        offset += consumed;
        if MODIFIERS_WITH_ARGUMENT.contains(&word) {
            skip_ws(&mut rest, &mut offset);
            let arg_line = rest.split('\n').next().unwrap_or("");
            if !arg_line.trim().is_empty() {
                let arg_len = arg_line
                    .find(|ch: char| !is_word_char(ch))
                    .unwrap_or(arg_line.len());
                rest = &rest[arg_len..];
                offset += arg_len;
            }
        }
    }
    Some(offset)
}

/// Length of the span from `open` to its balanced `close` (inclusive).
fn balanced(text: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 0isize;
    for (index, ch) in text.char_indices() {
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                return Some(index + ch.len_utf8());
            }
        }
    }
    None
}

fn collect_fab_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let mut entries: Vec<_> = entries
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
        if path.is_dir() {
            if name == ".git" || name == "node_modules" || name == "target" {
                continue;
            }
            collect_fab_files(&path, out);
        } else if name.ends_with(".fab") {
            out.push(path);
        }
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The nearest ancestor directory holding a `faber.toml`.
fn package_root(path: &Path) -> Option<PathBuf> {
    let mut dir = path.parent();
    while let Some(current) = dir {
        if current.join("faber.toml").is_file() {
            return Some(current.to_path_buf());
        }
        dir = current.parent();
    }
    None
}

/// `faber check --no-diagnostics` identity: exit status plus the sorted
/// multiset of `CODE:issue` labels from the compact rows.
fn check_identity(path: &Path) -> (Option<i32>, Vec<String>) {
    let output = faber()
        .arg("check")
        .arg("--no-diagnostics")
        .arg(path)
        .output()
        .expect("run faber check");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let mut labels: Vec<String> = Vec::new();
    for line in text.lines() {
        let Some(open) = line.find('[') else {
            continue;
        };
        let Some(close) = line[open..].find(']').map(|index| open + index) else {
            continue;
        };
        labels.push(line[open + 1..close].to_owned());
    }
    labels.sort();
    (output.status.code(), labels)
}

fn format_document(path: &Path) -> Result<String, Option<i32>> {
    let output = faber()
        .arg("format")
        .arg("--stdout")
        .arg(path)
        .output()
        .expect("run faber format");
    if !output.status.success() {
        return Err(output.status.code());
    }
    Ok(String::from_utf8(output.stdout).expect("utf8 stdout"))
}

/// A single-file case: check identity parity, and byte-identical formatted
/// documents where the annotation form formats. Both forms live at the same
/// file name in sibling directories so no output names a path.
fn single_file_equivalence(label: &str, original: &str, rewritten: &str) -> Result<(), String> {
    let original_dir = TempDir::new("faber-kernel-head", "annotation");
    let head_dir = TempDir::new("faber-kernel-head", "head");
    let original_path = original_dir.join("case.fab");
    let head_path = head_dir.join("case.fab");
    fs::write(&original_path, original).map_err(|err| err.to_string())?;
    fs::write(&head_path, rewritten).map_err(|err| err.to_string())?;

    let (original_status, original_ids) = check_identity(&original_path);
    let (head_status, head_ids) = check_identity(&head_path);
    if original_status != head_status {
        return Err(format!(
            "{label}: check exit status {original_status:?} vs {head_status:?}"
        ));
    }
    if original_ids != head_ids {
        return Err(format!(
            "{label}: check identities {:?} vs {:?}",
            original_ids, head_ids
        ));
    }

    if let Ok(original_formatted) = format_document(&original_path) {
        match format_document(&head_path) {
            Ok(head_formatted) if original_formatted == head_formatted => Ok(()),
            Ok(_) => Err(format!("{label}: formatted documents differ")),
            Err(code) => Err(format!("{label}: head form must format (exit {code:?})")),
        }
    } else {
        Ok(())
    }
}

/// Copy the package tree (sources, manifests and goldens only) into `to`.
fn copy_package(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if ["node_modules", "target", ".git"].contains(&name) {
                continue;
            }
            copy_package(&path, &to.join(name))?;
        } else if ["fab", "toml", "expected", "json"]
            .iter()
            .any(|extension| name.ends_with(extension))
        {
            fs::copy(&path, to.join(name))?;
        }
    }
    Ok(())
}

/// A package case: copy the tree, check it, rewrite the copies in place,
/// check again, and compare identities. Relative imports resolve the same
/// way in both runs because the paths never move.
fn package_equivalence(root: &Path, sources: &[(PathBuf, String, String)]) -> Result<(), String> {
    let label = root.display().to_string();
    let temp = TempDir::new("faber-kernel-head", "package");
    let copy_root = temp.join("pkg");
    copy_package(root, &copy_root).map_err(|err| format!("{label}: copy: {err}"))?;

    let (first_status, first_ids) = check_identity(&copy_root);

    for (relative, _, rewritten) in sources {
        let target = copy_root.join(relative);
        fs::write(&target, rewritten).map_err(|err| format!("{label}: rewrite {}: {err}", relative.display()))?;
    }

    let (second_status, second_ids) = check_identity(&copy_root);
    if first_status != second_status {
        return Err(format!(
            "{label}: check exit status {first_status:?} vs {second_status:?}"
        ));
    }
    if first_ids != second_ids {
        return Err(format!(
            "{label}: check identities {:?} vs {:?}",
            first_ids, second_ids
        ));
    }
    Ok(())
}

/// Run `tasks` on a small worker pool; each task reports its own failure.
fn run_parallel<T, F>(tasks: Vec<T>, run: F)
where
    T: Send,
    F: Fn(&T) -> Result<(), String> + Sync,
{
    let queue = Mutex::new(tasks);
    let failures: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism()
        .map(|count| count.get().min(8))
        .unwrap_or(4);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let next = queue.lock().expect("task queue").pop();
                let Some(task) = next else {
                    break;
                };
                if let Err(message) = run(&task) {
                    failures.lock().expect("failure list").push(message);
                }
            });
        }
    });
    let failures = failures.into_inner().expect("failure list");
    assert!(
        failures.is_empty(),
        "{} equivalence failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

enum Case {
    SingleFile { label: String, original: String, rewritten: String },
    Package { root: PathBuf, sources: Vec<(PathBuf, String, String)> },
}

#[test]
fn corpus_and_acceptance_kernel_cases_agree_between_both_heads() {
    let root = repo_root();
    let mut files = Vec::new();
    for tree in ["corpus", "corpus-product", "acceptance"] {
        collect_fab_files(&root.join(tree), &mut files);
    }

    let mut single: Vec<Case> = Vec::new();
    let mut packages: BTreeMap<PathBuf, Vec<(PathBuf, String, String)>> = BTreeMap::new();
    let (mut sites, mut files_rewritten) = (0usize, 0usize);
    for path in &files {
        let Ok(source) = fs::read_to_string(path) else {
            panic!("read {}", path.display());
        };
        let label = path
            .strip_prefix(&root)
            .expect("repo-relative path")
            .to_string_lossy()
            .replace('\\', "/");
        let Rewrite::Done {
            source: head_source,
            sites: site_count,
        } = rewrite_kernel_annotations(&source)
        else {
            // `NoSite` files carry no kernel; `Refused` files are the
            // annotation form's own negative exempla (an unknown or doubled
            // tier word, a lane binding, a tier on a non-function), whose
            // authored shape is the point of the case.
            continue;
        };
        sites += site_count;
        files_rewritten += 1;
        if let Some(package) = package_root(path)
            && package.starts_with(&root)
        {
            let relative = path
                .strip_prefix(&package)
                .expect("package-relative path")
                .to_path_buf();
            packages
                .entry(package)
                .or_default()
                .push((relative, source, head_source));
        } else {
            single.push(Case::SingleFile {
                label,
                original: source,
                rewritten: head_source,
            });
        }
    }

    let mut cases = single;
    for (package, sources) in packages {
        cases.push(Case::Package {
            root: package,
            sources,
        });
    }

    assert!(
        files_rewritten >= 100 && sites >= 380,
        "the gate must cover the corpus and acceptance kernel cases \
         (rewrote {files_rewritten} files, {sites} sites)"
    );

    run_parallel(cases, |case| match case {
        Case::SingleFile {
            label,
            original,
            rewritten,
        } => single_file_equivalence(label, original, rewritten),
        Case::Package { root, sources } => package_equivalence(root, sources),
    });
}
