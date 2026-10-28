//! TypeScript build recipe: data owned by the backend, changed by the project.
//!
//! The backend ships its default recipe as `recipe.toml` beside its runtime
//! sources, inside the embedded runtime. A project's `faber.toml` `[target.ts]` states
//! only its changes: `build` and `run` replace a command line, and each
//! `compiler_options` key replaces the same key of the recipe. `faber` runs
//! the merged recipe without knowing the target. Tools come from `PATH`; a
//! missing one fails by name.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use radix::module::diagnostics::Diagnostic;
use radix::program::manifest::ManifestTarget;
use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipeFile {
    build: Vec<String>,
    run: Vec<String>,
    #[serde(default)]
    compiler_options: Map<String, Value>,
}

/// The merged build recipe of one project.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TsRecipe {
    /// Build command line, run in the project root.
    pub build: Vec<String>,
    /// Run command line; `{launcher}` names the generated `bin/<name>.js`.
    pub run: Vec<String>,
    /// `compilerOptions` of the generated `tsconfig.json`.
    pub compiler_options: Map<String, Value>,
}

fn recipe_error(message: impl Into<String>) -> Diagnostic {
    crate::package_diagnostic_error(message).with_arg("issue", "package_ts_recipe_invalid")
}

impl TsRecipe {
    /// The backend default recipe from a repository-shaped embedded runtime root.
    pub(crate) fn backend_default(runtime_root: &Path) -> Result<Self, Diagnostic> {
        let path = super::ts_runtime::sources_dir(runtime_root).join("recipe.toml");
        let text = fs::read_to_string(&path).map_err(|err| {
            recipe_error(format!(
                "cannot read the TypeScript build recipe '{}': {err}",
                path.display()
            ))
        })?;
        Self::parse(&text).map_err(|message| recipe_error(format!("{}: {message}", path.display())))
    }

    fn parse(text: &str) -> Result<Self, String> {
        let file: RecipeFile = toml::from_str(text).map_err(|err| err.to_string())?;
        if file.build.is_empty() || file.run.is_empty() {
            return Err("`build` and `run` must name a command".to_owned());
        }
        Ok(Self {
            build: file.build,
            run: file.run,
            compiler_options: file.compiler_options,
        })
    }

    /// Apply a project's `[target.ts]` changes: only the keys it states move.
    pub(crate) fn with_project(mut self, project: Option<&ManifestTarget>) -> Self {
        let Some(project) = project else {
            return self;
        };
        if let Some(build) = &project.build {
            self.build.clone_from(build);
        }
        if let Some(run) = &project.run {
            self.run.clone_from(run);
        }
        for (key, value) in &project.compiler_options {
            self.compiler_options.insert(key.clone(), value.clone());
        }
        self
    }

    /// The project `tsconfig.json` text for this recipe.
    pub(crate) fn tsconfig(&self) -> String {
        let mut config = Map::new();
        config.insert(
            "compilerOptions".to_owned(),
            Value::Object(self.compiler_options.clone()),
        );
        config.insert("include".to_owned(), Value::from(vec!["src"]));
        let mut text = serde_json::to_string_pretty(&Value::Object(config))
            .unwrap_or_else(|_| "{}".to_owned());
        text.push('\n');
        text
    }

    /// The run command line with `{launcher}` replaced.
    pub(crate) fn run_argv(&self, launcher: &Path) -> Vec<String> {
        let launcher = launcher.display().to_string();
        self.run
            .iter()
            .map(|part| part.replace("{launcher}", &launcher))
            .collect()
    }
}

/// Find `tool` on `path` (the value of a `PATH` variable).
pub(crate) fn find_tool(tool: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    if tool.contains(std::path::MAIN_SEPARATOR) {
        let direct = PathBuf::from(tool);
        return direct.is_file().then_some(direct);
    }
    std::env::split_paths(path?)
        .map(|directory| directory.join(tool))
        .find(|candidate| candidate.is_file())
}

/// Resolve the program of a command line from `PATH`, or fail naming it.
pub(crate) fn require_tool(argv: &[String]) -> Result<PathBuf, Diagnostic> {
    require_tool_on(argv, std::env::var_os("PATH").as_deref())
}

/// [`require_tool`] against an explicit `PATH` value.
pub(crate) fn require_tool_on(
    argv: &[String],
    path: Option<&OsStr>,
) -> Result<PathBuf, Diagnostic> {
    let Some(tool) = argv.first() else {
        return Err(recipe_error("the TypeScript recipe has an empty command"));
    };
    find_tool(tool, path).ok_or_else(|| {
        crate::package_diagnostic_error(format!(
            "TypeScript programs require `{tool}` on PATH, and it was not found"
        ))
        .with_arg("issue", "package_ts_tool_missing")
        .with_arg("tool", tool.clone())
    })
}

#[cfg(test)]
#[path = "ts_recipe_test.rs"]
mod tests;
