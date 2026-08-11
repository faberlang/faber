use std::collections::BTreeMap;

pub(super) fn assemble_crate(entry_code: &str, module_code: &str) -> String {
    if module_code.trim().is_empty() {
        return ensure_trailing_newline(entry_code);
    }

    let mut output = String::new();
    let lines = entry_code.lines().collect::<Vec<_>>();
    let insert_after = leading_crate_attribute_end(&lines);

    for (idx, line) in lines.iter().enumerate() {
        output.push_str(line);
        output.push('\n');

        if idx + 1 == insert_after {
            output.push('\n');
            output.push_str(module_code);
            if !module_code.ends_with('\n') {
                output.push('\n');
            }
            output.push('\n');
        }
    }

    if insert_after == 0 {
        output.push('\n');
        output.push_str(module_code);
        if !module_code.ends_with('\n') {
            output.push('\n');
        }
    }

    output
}

fn ensure_trailing_newline(code: &str) -> String {
    if code.ends_with('\n') {
        code.to_owned()
    } else {
        format!("{code}\n")
    }
}

fn leading_crate_attribute_end(lines: &[&str]) -> usize {
    let mut last_attr = 0;
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        if trimmed.starts_with("#![") {
            last_attr = idx + 1;
            continue;
        }
        break;
    }
    last_attr
}

#[derive(Default)]
pub(super) struct ModuleNode {
    code: Option<String>,
    children: BTreeMap<String, ModuleNode>,
}

impl ModuleNode {
    pub(super) fn insert(&mut self, path: &[String], code: String) {
        if path.is_empty() {
            self.code = Some(code);
            return;
        }

        let child = self.children.entry(path[0].clone()).or_default();
        child.insert(&path[1..], code);
    }

    pub(super) fn render(&self, indent: usize) -> String {
        let mut rendered = String::new();

        if let Some(code) = &self.code {
            for line in code.lines() {
                rendered.push_str(&" ".repeat(indent));
                rendered.push_str(line);
                rendered.push('\n');
            }
        }

        for (name, child) in &self.children {
            rendered.push_str(&" ".repeat(indent));
            rendered.push_str("pub mod ");
            rendered.push_str(name);
            rendered.push_str(" {\n");
            rendered.push_str(&child.render(indent + 4));
            rendered.push_str(&" ".repeat(indent));
            rendered.push_str("}\n");
        }

        rendered
    }
}
