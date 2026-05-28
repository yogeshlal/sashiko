// Copyright 2026 The Sashiko Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::ai::AiTool;
use crate::ai::truncator::Truncator;
use anyhow::{Result, anyhow, ensure};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

pub struct ToolBox {
    worktree_path: PathBuf,
    prompts_path: Option<PathBuf>,
}

impl ToolBox {
    pub fn new(worktree_path: PathBuf, prompts_path: Option<PathBuf>) -> Self {
        Self {
            worktree_path,
            prompts_path,
        }
    }

    pub fn get_worktree_path(&self) -> &Path {
        &self.worktree_path
    }

    /// Returns generic tool declarations.
    pub fn get_declarations_generic(&self) -> Vec<AiTool> {
        let mut decls = vec![
            AiTool {
                name: "read_files".to_string(),
                description: "Read the content of one or more files at a specific Git revision. In 'smart' mode, it collapses irrelevant code around the focus lines."
                    .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "revision": { "type": "string", "description": "The Git commit SHA or reference (e.g., HEAD, baseline SHA, or target commit SHA) to read from." },
                        "files": {
                            "type": "array",
                            "description": "List of files to read.",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "path": { "type": "string", "description": "Relative path to the file." },
                                    "start_line": { "type": "integer", "description": "1-based start line (optional). In smart mode, this is the start of the focus area." },
                                    "end_line": { "type": "integer", "description": "1-based end line (optional). In smart mode, this is the end of the focus area." }
                                },
                                "required": ["path"]
                            }
                        },
                        "mode": { "type": "string", "enum": ["raw", "smart"], "description": "Read mode. Defaults to 'raw'." }
                    },
                    "required": ["revision", "files"]
                }),
            },
            AiTool {
                name: "git_blame".to_string(),
                description: "Show what revision and author last modified each line of a file."
                    .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "revision": { "type": "string", "description": "The Git commit SHA or reference to blame from." },
                        "path": { "type": "string", "description": "Relative path to the file." },
                        "start_line": { "type": "integer", "description": "1-based start line (optional)." },
                        "end_line": { "type": "integer", "description": "1-based end line (optional)." }
                    },
                    "required": ["revision", "path"]
                }),
            },
            AiTool {
                name: "git_diff".to_string(),
                description: "Show changes between two commits or revisions."
                    .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "base_revision": { "type": "string", "description": "The baseline commit SHA or revision reference." },
                        "target_revision": { "type": "string", "description": "The target commit SHA or revision reference to compare against." }
                    },
                    "required": ["base_revision", "target_revision"]
                }),
            },
            AiTool {
                name: "git_show".to_string(),
                description: "Show various types of objects (blobs, trees, tags and commits). Supports line filtering for blobs and diff suppression for commits."
                    .to_string(),
                parameters: json!({
                        "type": "object",
                        "properties": {
                            "object": { "type": "string", "description": "The object to show (e.g. 'HEAD:README.md' or 'HEAD')." },
                            "suppress_diff": { "type": "boolean", "description": "If true, suppresses the diff output for commits (shows only metadata). Useful for checking commit details cheaply." },
                            "start_line": { "type": "integer", "description": "1-based start line (optional). Useful for reading specific parts of a file (blob)." },
                            "end_line": { "type": "integer", "description": "1-based end line (optional)." }
                        },
                        "required": ["object"]
                }),
            },
            AiTool {
                name: "git_log".to_string(),
                description: "Show commit logs in a specific range or revision history.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "range": { "type": "string", "description": "The commit range or reference to view logs for (e.g., 'baseline..HEAD' or 'HEAD')." },
                        "limit": { "type": "integer", "description": "Limit the number of commits returned (defaults to 10, max 100)." }
                    },
                    "required": ["range"]
                }),
            },
            AiTool {
                name: "list_dir".to_string(),
                description: "List files in a directory at a specific Git revision.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "revision": { "type": "string", "description": "The Git commit SHA or reference to list from." },
                        "path": { "type": "string", "description": "Relative path to the directory (e.g., '.' or 'src/')." }
                    },
                    "required": ["revision", "path"]
                }),
            },
            AiTool {
                name: "search_file_content".to_string(),
                description: "Search for a pattern in files using git grep at a specific Git revision. Returns matching lines with context.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "revision": { "type": "string", "description": "The Git commit SHA or reference to search at." },
                        "pattern": { "type": "string", "description": "Regex pattern to search for." },
                        "path": { "type": "string", "description": "Relative path directory or file to limit the search (optional)." },
                        "context_lines": { "type": "integer", "description": "Number of context lines to show (default 0)." }
                    },
                    "required": ["revision", "pattern"]
                }),
            },
            AiTool {
                name: "find_files".to_string(),
                description: "Find files matching a glob pattern in a specific Git revision.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "revision": { "type": "string", "description": "The Git commit SHA or reference to search in." },
                        "pattern": { "type": "string", "description": "Glob pattern to match (e.g., '*.rs' or 'src/**/mod.rs')." }
                    },
                    "required": ["revision", "pattern"]
                }),
            },

        ];

        if self.prompts_path.is_some() {
            decls.push(AiTool {
                name: "read_prompt".to_string(),
                description: "Read a specific prompt file from the prompt registry (e.g., 'mm.md', 'locking.md').".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Name of the prompt file (e.g., 'patterns/BPF-001.md')." }
                    },
                    "required": ["name"]
                }),
            });
        }

        decls
    }

    pub async fn call(&self, name: &str, args: Value) -> Result<Value> {
        let name_normalized = name.trim().to_lowercase();
        match name_normalized.as_str() {
            "read_files" => self.read_files(args).await,
            "git_blame" => self.git_blame(args).await,
            "git_diff" => self.git_diff(args).await,
            "git_show" => self.git_show(args).await,
            "git_log" => self.git_log(args).await,
            "list_dir" => self.list_dir(args).await,
            "search_file_content" => self.search_file_content(args).await,
            "find_files" => self.find_files(args).await,

            "read_prompt" => self.read_prompt(args).await,
            _ => Err(anyhow!("Unknown tool: {}", name)),
        }
    }

    fn truncate_output(&self, output: String) -> String {
        Truncator::truncate_diff(&output, 10_000)
    }

    async fn read_prompt(&self, args: Value) -> Result<Value> {
        let prompts_path = self
            .prompts_path
            .as_ref()
            .ok_or_else(|| anyhow!("read_prompt tool is not available"))?;
        let name = args["name"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing prompt name"))?;

        let path = self.validate_path(name, prompts_path)?;
        let content = fs::read_to_string(path).await?;

        Ok(json!({ "content": content }))
    }

    async fn read_files(&self, args: Value) -> Result<Value> {
        let revision = args["revision"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing revision"))?;
        let files = args["files"]
            .as_array()
            .ok_or_else(|| anyhow!("Missing files"))?;
        let mode = args["mode"].as_str().unwrap_or("raw");

        let mut results = Vec::new();

        for file_args in files {
            let path_str = file_args["path"].as_str().unwrap_or_default();
            if path_str.is_empty() {
                results.push(json!({ "error": "Missing path" }));
                continue;
            }

            let start_line = file_args["start_line"].as_u64().map(|v| v as usize);
            let end_line = file_args["end_line"].as_u64().map(|v| v as usize);

            match self
                .read_single_file(revision, path_str, start_line, end_line, mode)
                .await
            {
                Ok(mut val) => {
                    if let Some(obj) = val.as_object_mut() {
                        obj.insert("path".to_string(), json!(path_str));
                    }
                    results.push(val);
                }
                Err(e) => {
                    results.push(json!({
                        "path": path_str,
                        "error": e.to_string()
                    }));
                }
            }
        }

        Ok(json!({ "results": results }))
    }

    async fn read_single_file(
        &self,
        revision: &str,
        path_str: &str,
        start_line: Option<usize>,
        end_line: Option<usize>,
        mode: &str,
    ) -> Result<Value> {
        if path_str.starts_with('-') {
            return Err(anyhow!("Invalid path name: {}", path_str));
        }

        let mut cmd = Command::new("git");
        cmd.current_dir(&self.worktree_path)
            .args(["show", &format!("{}:{}", revision, path_str)]);

        let output = cmd.output().await?;
        if !output.status.success() {
            return Err(anyhow!(
                "git show failed to read file {} at {}: {}",
                path_str,
                revision,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

        let content = String::from_utf8_lossy(&output.stdout).to_string();

        if let (Some(s), Some(e)) = (start_line, end_line) {
            ensure!(s <= e, "Invalid range: start_line ({s}) > end_line ({e})");
        }

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        let start_line = start_line.map(|s| s.clamp(1, total_lines));
        let end_line = end_line.map(|e| e.clamp(1, total_lines));

        if mode == "smart" {
            let focus = match (start_line, end_line) {
                (Some(s), Some(e)) => Some(s..e),
                (Some(s), None) => Some(s..s + 1),
                (None, Some(e)) => Some(1..e),
                (None, None) => None,
            };

            let truncated = Truncator::truncate_code(&content, focus, 20_000);

            return Ok(json!({
                "content": truncated,
                "total_lines": total_lines,
                "mode": "smart"
            }));
        }

        let (start, end) = match (start_line, end_line) {
            (Some(s), Some(e)) => (s.max(1) - 1, e.min(total_lines)),
            (Some(s), None) => (s.max(1) - 1, total_lines),
            (None, Some(e)) => (0, e.min(total_lines)),
            (None, None) => (0, total_lines),
        };

        let start = start.min(total_lines);
        let end = end.clamp(start, total_lines);

        if start >= total_lines {
            return Ok(json!({ "content": "", "lines_read": 0, "total_lines": total_lines }));
        }

        let slice = &lines[start..end];
        let result = slice.join("\n");
        let truncated = self.truncate_output(result);

        Ok(json!({
            "content": truncated,
            "lines_read": slice.len(),
            "total_lines": total_lines,
            "start_line": start + 1,
            "end_line": end
        }))
    }

    async fn git_blame(&self, args: Value) -> Result<Value> {
        let revision = args["revision"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing revision"))?;
        let path_str = args["path"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing path"))?;
        let start_line = args["start_line"].as_u64();
        let end_line = args["end_line"].as_u64();

        let mut cmd = Command::new("git");
        cmd.current_dir(&self.worktree_path).arg("blame");

        if let (Some(s), Some(e)) = (start_line, end_line) {
            cmd.arg(format!("-L{},{}", s, e));
        }

        cmd.arg(revision).arg("--").arg(path_str);

        let output = cmd.output().await?;
        if !output.status.success() {
            return Err(anyhow!(
                "git blame failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

        let content = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(json!({ "content": self.truncate_output(content) }))
    }

    async fn git_diff(&self, args: Value) -> Result<Value> {
        let base = args["base_revision"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing base_revision"))?;
        let target = args["target_revision"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing target_revision"))?;

        if base.starts_with('-') || target.starts_with('-') {
            return Err(anyhow!("Invalid revision names"));
        }

        let output = Command::new("git")
            .current_dir(&self.worktree_path)
            .args(["diff", "--diff-algorithm=histogram", base, target])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(anyhow!("git diff failed: {}", stderr));
        }

        let content = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(json!({ "content": Truncator::truncate_diff(&content, 10_000) }))
    }

    async fn git_log(&self, args: Value) -> Result<Value> {
        let range = args["range"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing range"))?;
        let limit = args["limit"].as_u64().unwrap_or(10).min(100) as usize;

        if range.starts_with('-') {
            return Err(anyhow!("Invalid range"));
        }

        let limit_str = limit.to_string();
        let mut cmd = Command::new("git");
        cmd.current_dir(&self.worktree_path)
            .args(["log", "-n", &limit_str, range])
            .kill_on_drop(true);

        let output = cmd.output().await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Ok(json!({ "error": format!("git log failed: {}", stderr) }));
        }

        Ok(
            json!({ "output": self.truncate_output(String::from_utf8_lossy(&output.stdout).to_string()) }),
        )
    }

    async fn git_show(&self, args: Value) -> Result<Value> {
        let object = args["object"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing object"))?;
        let suppress_diff = args["suppress_diff"].as_bool().unwrap_or(false);
        let start_line = args["start_line"].as_u64().map(|v| v as usize);
        let end_line = args["end_line"].as_u64().map(|v| v as usize);

        if object.starts_with('-') {
            return Err(anyhow!("Invalid object name: {}", object));
        }

        let mut cmd = Command::new("git");
        cmd.current_dir(&self.worktree_path).arg("show");

        if suppress_diff {
            cmd.arg("--no-patch");
        }

        cmd.arg(object);

        let output = cmd.output().await?;

        if !output.status.success() {
            return Err(anyhow!(
                "git show failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

        let content = String::from_utf8_lossy(&output.stdout).to_string();

        if start_line.is_some() || end_line.is_some() {
            let lines: Vec<&str> = content.lines().collect();
            let total_lines = lines.len();
            let (start, end) = match (start_line, end_line) {
                (Some(s), Some(e)) => (s.max(1) - 1, e.min(total_lines)),
                (Some(s), None) => (s.max(1) - 1, total_lines),
                (None, Some(e)) => (0, e.min(total_lines)),
                (None, None) => (0, total_lines),
            };

            let start = start.min(total_lines);
            let end = end.clamp(start, total_lines);

            if start >= total_lines {
                return Ok(json!({ "content": "", "lines_read": 0, "total_lines": total_lines }));
            }

            let slice = &lines[start..end];
            let result = slice.join("\n");
            return Ok(json!({
                "content": self.truncate_output(result),
                "total_lines": total_lines,
                "start_line": start + 1,
                "end_line": end
            }));
        }

        Ok(json!({ "content": self.truncate_output(content) }))
    }

    async fn list_dir(&self, args: Value) -> Result<Value> {
        let revision = args["revision"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing revision"))?;
        let path_str = args["path"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing path"))?;

        if revision.starts_with('-') || path_str.starts_with('-') {
            return Err(anyhow!("Invalid revision or path name"));
        }

        let tree_spec = if path_str.is_empty() || path_str == "." {
            revision.to_string()
        } else {
            format!("{}:{}", revision, path_str)
        };

        let mut cmd = Command::new("git");
        cmd.current_dir(&self.worktree_path)
            .args(["ls-tree", &tree_spec]);

        let output = cmd.output().await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Ok(
                json!({ "error": format!("git ls-tree failed for {}: {}", tree_spec, stderr) }),
            );
        }

        let content = String::from_utf8_lossy(&output.stdout).to_string();
        let mut entries = Vec::new();
        for line in content.lines() {
            let split_tab: Vec<&str> = line.split('\t').collect();
            if split_tab.len() >= 2 {
                let filename = split_tab[1];
                let metadata: Vec<&str> = split_tab[0].split_whitespace().collect();
                if metadata.len() >= 2 {
                    let ty = match metadata[1] {
                        "tree" => "dir",
                        _ => "file",
                    };
                    entries.push(json!({ "name": filename, "type": ty }));
                }
            }
        }

        if entries.len() > 1000 {
            entries.truncate(1000);
        }

        Ok(json!({ "entries": entries }))
    }

    async fn search_file_content(&self, args: Value) -> Result<Value> {
        let revision = args["revision"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing revision"))?;
        let pattern = args["pattern"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing pattern"))?;
        let path_str = args["path"].as_str();
        let context_lines = args["context_lines"].as_u64().unwrap_or(0) as usize;

        if revision.starts_with('-') || pattern.starts_with('-') {
            return Err(anyhow!("Invalid revision or pattern"));
        }

        let mut cmd = Command::new("git");
        cmd.current_dir(&self.worktree_path)
            .arg("grep")
            .arg("-n")
            .arg("-I")
            .arg("-P")
            .arg(format!("-C{}", context_lines))
            .arg(pattern)
            .arg(revision);

        if let Some(p) = path_str
            && p != "."
            && !p.is_empty()
        {
            cmd.arg("--").arg(p);
        }

        let output = cmd.output().await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if stderr.is_empty() {
                return Ok(json!({ "matches": [], "message": "No matches found." }));
            }
            return Ok(json!({ "error": format!("git grep failed: {}", stderr) }));
        }

        let content = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(json!({ "content": self.truncate_output(content) }))
    }

    async fn find_files(&self, args: Value) -> Result<Value> {
        let revision = args["revision"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing revision"))?;
        let pattern = args["pattern"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing pattern"))?;

        if revision.starts_with('-') {
            return Err(anyhow!("Invalid revision"));
        }

        let mut cmd = Command::new("git");
        cmd.current_dir(&self.worktree_path)
            .args(["ls-tree", "-r", "--name-only", revision]);

        let output = cmd.output().await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Ok(json!({ "error": format!("git ls-tree failed: {}", stderr) }));
        }

        let content = String::from_utf8_lossy(&output.stdout).to_string();
        let files: Vec<&str> = content.lines().collect();

        let regex = glob_to_regex(pattern)?;
        let mut matched_files = Vec::new();
        for f in files {
            if regex.is_match(f) {
                matched_files.push(f);
            }
        }

        if matched_files.len() > 1000 {
            let truncated = matched_files[..1000].join("\n");
            return Ok(json!({
                 "files": truncated,
                 "total_found": matched_files.len(),
                 "message": "Output truncated to 1000 files."
            }));
        }

        let content_matched = matched_files.join("\n");
        Ok(json!({ "files": content_matched }))
    }



    fn validate_path(&self, relative: &str, base: &Path) -> Result<PathBuf> {
        if relative.contains("..") || relative.starts_with("/") {
            return Err(anyhow!("Invalid path: {}", relative));
        }
        let full_path = base.join(relative);

        let canonical_base = base
            .canonicalize()
            .map_err(|e| anyhow!("Failed to canonicalize base path: {}", e))?;

        let canonical_full = match full_path.canonicalize() {
            Ok(p) => p,
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => {
                if let Some(parent) = full_path.parent() {
                    let canonical_parent = parent
                        .canonicalize()
                        .map_err(|e| anyhow!("Failed to canonicalize parent path: {}", e))?;
                    if !canonical_parent.starts_with(&canonical_base) {
                        return Err(anyhow!("Path traversal detected in parent: {:?}", parent));
                    }
                    full_path
                } else {
                    return Err(anyhow!("No parent directory for path: {:?}", full_path));
                }
            }
            Err(e) => return Err(anyhow!("Failed to canonicalize path: {}", e)),
        };

        if !canonical_full.starts_with(&canonical_base) {
            return Err(anyhow!("Path traversal detected: {:?}", canonical_full));
        }

        Ok(canonical_full)
    }
}

fn glob_to_regex(glob: &str) -> Result<regex::Regex> {
    let mut regex_str = String::new();
    regex_str.push('^');
    for c in glob.chars() {
        match c {
            '*' => regex_str.push_str(".*"),
            '?' => regex_str.push('.'),
            '.' | '+' | '(' | ')' | '|' | '^' | '$' | '[' | ']' | '{' | '}' | '\\' => {
                regex_str.push('\\');
                regex_str.push(c);
            }
            _ => regex_str.push(c),
        }
    }
    regex_str.push('$');
    regex::Regex::new(&regex_str)
        .map_err(|e| anyhow::anyhow!("Invalid glob converted to regex: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_search_file_content() -> Result<()> {
        let dir = tempdir()?;
        let repo_path = dir.path().to_path_buf();

        // Init git repo
        Command::new("git")
            .current_dir(&repo_path)
            .args(["init"])
            .output()
            .await?;

        Command::new("git")
            .current_dir(&repo_path)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .await?;
        Command::new("git")
            .current_dir(&repo_path)
            .args(["config", "user.name", "Test User"])
            .output()
            .await?;

        let file_path = repo_path.join("test.rs");
        let mut file = File::create(&file_path)?;
        writeln!(file, "fn main() {{")?;
        writeln!(file, "    println!(\"Hello World\");")?;
        writeln!(file, "    // TODO: fix this")?;
        writeln!(file, "}}")?;

        Command::new("git")
            .current_dir(&repo_path)
            .args(["add", "."])
            .output()
            .await?;
        Command::new("git")
            .current_dir(&repo_path)
            .args(["commit", "-m", "Initial"])
            .output()
            .await?;

        let toolbox = ToolBox::new(repo_path.clone(), None);

        // Test basic search
        let args = json!({
            "revision": "HEAD",
            "pattern": "println",
            "path": "."
        });
        let result = toolbox.call("search_file_content", args).await?;
        let content = result["content"].as_str().unwrap();

        assert!(content.contains("test.rs"));
        assert!(content.contains("2:    println!(\"Hello World\");"));

        // Test context
        let args = json!({
            "revision": "HEAD",
            "pattern": "TODO",
            "context_lines": 1
        });
        let result = toolbox.call("search_file_content", args).await?;
        let content = result["content"].as_str().unwrap();

        assert!(content.contains("2-    println!(\"Hello World\");"));
        assert!(content.contains("3:    // TODO: fix this"));
        assert!(content.contains("4-}"));

        Ok(())
    }

    #[tokio::test]
    async fn test_tool_normalization() -> Result<()> {
        let dir = tempdir()?;
        let prompt_path = dir.path().join("test.md");
        std::fs::write(&prompt_path, "prompt content")?;

        let toolbox = ToolBox::new(dir.path().to_path_buf(), Some(dir.path().to_path_buf()));

        let args = json!({
            "name": "test.md"
        });
        let result = toolbox.call("  Read_Prompt  ", args).await?;
        assert_eq!(result["content"].as_str().unwrap(), "prompt content");

        Ok(())
    }

    #[tokio::test]
    async fn test_git_tools() -> Result<()> {
        let dir = tempdir()?;
        let repo_path = dir.path().to_path_buf();
        let toolbox = ToolBox::new(repo_path.clone(), None);

        // Init repo
        Command::new("git")
            .current_dir(&repo_path)
            .args(["init"])
            .output()
            .await?;

        // Ensure we are on master
        let _ = Command::new("git")
            .current_dir(&repo_path)
            .args(["branch", "-m", "master"])
            .output()
            .await;

        Command::new("git")
            .current_dir(&repo_path)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .await?;
        Command::new("git")
            .current_dir(&repo_path)
            .args(["config", "user.name", "Test User"])
            .output()
            .await?;

        // Create a file and commit
        let file_path = repo_path.join("test.txt");
        let mut file = File::create(&file_path)?;
        writeln!(file, "Hello")?;

        Command::new("git")
            .current_dir(&repo_path)
            .args(["add", "."])
            .output()
            .await?;
        Command::new("git")
            .current_dir(&repo_path)
            .args(["commit", "-m", "Initial"])
            .output()
            .await?;

        // Test read_files
        let result = toolbox
            .call(
                "read_files",
                json!({
                    "revision": "HEAD",
                    "files": [{"path": "test.txt"}]
                }),
            )
            .await?;
        let content = result["results"][0]["content"].as_str().unwrap();
        assert!(content.contains("Hello"));

        // Test git_log
        let result = toolbox.call("git_log", json!({ "range": "HEAD" })).await?;
        let output = result["output"].as_str().unwrap();
        assert!(output.contains("Initial"));

        Ok(())
    }

    #[tokio::test]
    async fn test_validate_path_security() -> Result<()> {
        let dir = tempdir()?;
        let wt_path = dir.path().to_path_buf();
        let toolbox = ToolBox::new(wt_path.clone(), None);

        // Create a target file outside the worktree
        let outside_dir = tempdir()?;
        let outside_file = outside_dir.path().join("secret.txt");
        std::fs::write(&outside_file, "my secret key")?;

        // Create a target file inside the worktree
        let inside_file = wt_path.join("safe.txt");
        std::fs::write(&inside_file, "safe content")?;

        // 1. Test valid relative path inside
        let path = toolbox.validate_path("safe.txt", &wt_path);
        assert!(path.is_ok());
        assert_eq!(path.unwrap(), inside_file.canonicalize()?);

        // 2. Test path traversal attempt
        let path = toolbox.validate_path("../secret.txt", &wt_path);
        assert!(path.is_err());

        // 3. Test symlink pointing outside (should be blocked)
        #[cfg(unix)]
        {
            let symlink_outside = wt_path.join("link_outside");
            std::os::unix::fs::symlink(&outside_file, &symlink_outside)?;

            let path = toolbox.validate_path("link_outside", &wt_path);
            assert!(path.is_err(), "Symlink pointing outside should be blocked");
        }

        // 4. Test symlink pointing inside (should be allowed)
        #[cfg(unix)]
        {
            let symlink_inside = wt_path.join("link_inside");
            std::os::unix::fs::symlink(&inside_file, &symlink_inside)?;

            let path = toolbox.validate_path("link_inside", &wt_path);
            assert!(path.is_ok(), "Symlink pointing inside should be allowed");
            assert_eq!(path.unwrap(), inside_file.canonicalize()?);
        }

        // 5. Test non-existent file inside (should be allowed for creation)
        let path = toolbox.validate_path("new_file.txt", &wt_path);
        assert!(path.is_ok());
        assert_eq!(path.unwrap(), wt_path.join("new_file.txt"));

        // 6. Test non-existent file inside nested directory (should be allowed if parent is safe)
        let nested_dir = wt_path.join("nested");
        std::fs::create_dir(&nested_dir)?;
        let path = toolbox.validate_path("nested/new_file.txt", &wt_path);
        assert!(path.is_ok());
        assert_eq!(path.unwrap(), nested_dir.join("new_file.txt"));

        // 7. Test non-existent file in symlinked outside directory (should be blocked)
        #[cfg(unix)]
        {
            let symlink_dir_outside = wt_path.join("link_dir_outside");
            std::os::unix::fs::symlink(outside_dir.path(), &symlink_dir_outside)?;

            let path = toolbox.validate_path("link_dir_outside/new_file.txt", &wt_path);
            assert!(
                path.is_err(),
                "Creating file in symlinked outside directory should be blocked"
            );
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_git_tools_security() -> Result<()> {
        let dir = tempdir()?;
        let repo_path = dir.path().to_path_buf();
        let toolbox = ToolBox::new(repo_path.clone(), None);

        // Init repo so git commands work
        Command::new("git")
            .current_dir(&repo_path)
            .arg("init")
            .output()
            .await?;

        // 1. Test git_diff with safe args
        let args = json!({
            "base_revision": "HEAD^",
            "target_revision": "HEAD"
        });
        let res = toolbox.call("git_diff", args).await;
        if let Err(e) = res {
            assert!(!e.to_string().contains("Invalid revision names"));
        }

        // Test git_diff with forbidden args
        let args = json!({
            "base_revision": "--output=malicious.txt",
            "target_revision": "HEAD"
        });
        let res = toolbox.call("git_diff", args).await;
        assert!(res.is_err());
        assert!(
            res.unwrap_err()
                .to_string()
                .contains("Invalid revision names")
        );

        // 2. Test git_log with safe args
        let args = json!({
            "range": "HEAD"
        });
        let res = toolbox.call("git_log", args).await;
        if let Err(e) = res {
            assert!(!e.to_string().contains("Invalid range"));
        }

        // Test git_log with forbidden args
        let args = json!({
            "range": "--output=malicious.txt"
        });
        let res = toolbox.call("git_log", args).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("Invalid range"));

        // 3. Test git_show with safe object
        let args = json!({
            "object": "HEAD:README.md"
        });
        let res = toolbox.call("git_show", args).await;
        if let Err(e) = res {
            assert!(!e.to_string().contains("Invalid object name"));
        }

        // Test git_show with forbidden object (starts with -)
        let args = json!({
            "object": "--output=malicious.txt"
        });
        let res = toolbox.call("git_show", args).await;
        assert!(res.is_err());
        assert!(
            res.unwrap_err()
                .to_string()
                .contains("Invalid object name: --output=malicious.txt")
        );

        Ok(())
    }
}
