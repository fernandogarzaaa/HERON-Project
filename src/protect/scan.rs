use super::{lexical, SKIP_DIRS, WORKSPACE_NAMES};
use std::path::{Path, PathBuf};

/// Upper bound on directory entries the scan visits.
pub const SCAN_LIMIT: usize = 100_000;

/// One repository the scan found: the `.git` entry, the directory git keeps this checkout's own state
/// in, and the directory holding the hooks and the shared config.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Repo {
    pub entry: PathBuf,
    pub git_dir: PathBuf,
    pub common_dir: PathBuf,
}

impl Repo {
    pub fn from_entry(entry: PathBuf) -> Repo {
        // A `.git` folder can name another folder in `commondir` too: git then reads hooks and config
        // from there.
        let git_dir = if entry.is_dir() { entry.clone() } else { gitdir_of(&entry).unwrap_or_else(|| entry.clone()) };
        let common_dir = commondir_of(&git_dir).unwrap_or_else(|| git_dir.clone());
        Repo { entry, git_dir, common_dir }
    }

    /// The files and folders of this repository that run code: hooks and both configs, and the
    /// `commondir` and `gitdir` files, which move where git looks for them.
    pub fn parts(&self) -> Vec<PathBuf> {
        vec![
            self.common_dir.join("hooks"),
            self.common_dir.join("config"),
            self.git_dir.join("config.worktree"),
            self.git_dir.join("commondir"),
            self.git_dir.join("gitdir"),
        ]
    }

    /// The same redirect files for each linked worktree, kept in the shared folder: a worktree whose
    /// own `.git` file is out of the scan's reach (outside the workspace, or under `.claude/worktrees`)
    /// still takes its hooks from where these say.
    pub fn worktree_admin(&self) -> Vec<PathBuf> {
        let Ok(rd) = std::fs::read_dir(self.common_dir.join("worktrees")) else { return vec![] };
        let mut out = vec![];
        for e in rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())) {
            for file in ["commondir", "gitdir", "config.worktree"] {
                out.push(e.path().join(file));
            }
        }
        out.sort();
        out
    }
}

fn gitdir_of(entry: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(entry).ok()?;
    let target = text.lines().next()?.trim().strip_prefix("gitdir:")?.trim();
    let base = entry.parent()?;
    (!target.is_empty()).then(|| resolve(base, target))
}

fn commondir_of(git_dir: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(git_dir.join("commondir")).ok()?;
    let target = text.lines().next()?.trim();
    (!target.is_empty()).then(|| resolve(git_dir, target))
}

fn resolve(base: &Path, target: &str) -> PathBuf {
    lexical(&base.join(target))
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Scan {
    pub repos: Vec<Repo>,
    /// Entries with a protected name.
    pub named: Vec<PathBuf>,
    pub truncated: bool,
}

/// Find protected entries in the workspace without following symlinks.
pub fn scan(workspace: &Path) -> Scan {
    scan_limited(workspace, SCAN_LIMIT)
}

pub(super) fn scan_limited(workspace: &Path, limit: usize) -> Scan {
    let mut out = Scan::default();
    let mut entries = vec![];
    let mut stack = vec![workspace.to_path_buf()];
    let mut seen = 0usize;
    'walk: while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            seen += 1;
            if seen > limit {
                out.truncated = true;
                break 'walk;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            let path = e.path();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if name == ".git" {
                entries.push(path);
            } else if WORKSPACE_NAMES.contains(&name.as_str()) {
                out.named.push(path);
            } else if is_dir && !SKIP_DIRS.contains(&name.as_str()) {
                stack.push(path);
            }
        }
    }
    entries.sort();
    out.repos = entries.into_iter().map(Repo::from_entry).collect();
    out.named.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::super::tests::tree;
    use super::*;

    fn rel(d: &Path, v: &[PathBuf]) -> Vec<PathBuf> {
        v.iter().map(|p| p.strip_prefix(d).unwrap().to_path_buf()).collect()
    }

    #[test]
    fn scan_finds_repos_and_names_at_any_depth_but_skips_big_dirs() {
        let d = tree(&[
            ".git/config",
            ".git/hooks/",
            "sub/deep/.envrc",
            "sub/.vscode/settings.json",
            "wt/.git",
            "node_modules/pkg/.mcp.json",
            "target/.claude/x",
            "src/main.rs",
        ]);
        let s = scan(d.path());
        let entries: Vec<PathBuf> = s.repos.iter().map(|r| r.entry.clone()).collect();
        assert_eq!(rel(d.path(), &entries), vec![PathBuf::from(".git"), PathBuf::from("wt/.git")]);
        assert_eq!(rel(d.path(), &s.named), vec![PathBuf::from("sub/.vscode"), PathBuf::from("sub/deep/.envrc")]);
        assert!(!s.truncated);
    }

    #[test]
    fn scan_stops_at_the_limit_and_says_so() {
        let d = tree(&[
            ".git/config",
            ".git/hooks/",
            "sub/deep/.envrc",
            "sub/.vscode/settings.json",
            "wt/.git",
            "node_modules/pkg/.mcp.json",
            "target/.claude/x",
            "src/main.rs",
        ]);
        // 6 entries at the top, 2 in sub, 1 each in sub/deep, wt and src; node_modules and target are
        // counted, not entered.
        // The order a folder lists its entries in differs between file systems, so only the counts are
        // checked, never which entries a cut scan reached.
        let entries = 11;
        let full = scan_limited(d.path(), entries);
        assert!(!full.truncated, "exactly at the limit");
        assert_eq!((full.repos.len(), full.named.len()), (2, 2), "{full:?}");
        assert!(scan_limited(d.path(), entries - 1).truncated);
        let one = scan_limited(d.path(), 1);
        assert!(one.truncated);
        assert!(one.repos.len() + one.named.len() <= 1, "the scan went on past its limit: {one:?}");
        assert_eq!(SCAN_LIMIT, 100_000);
    }

    #[test]
    fn git_file_resolves_to_common_dir() {
        let d = tree(&["main/.git/worktrees/w1/commondir", "sub/.git/modules/m/HEAD", "wt/.git", "mod/.git", "odd/.git"]);
        let root = d.path();
        std::fs::write(root.join("main/.git/worktrees/w1/commondir"), "../..\n").unwrap();
        std::fs::write(root.join("wt/.git"), format!("gitdir: {}\n", root.join("main/.git/worktrees/w1").display())).unwrap();
        std::fs::write(root.join("mod/.git"), "gitdir: ../sub/.git/modules/m\n").unwrap();
        std::fs::write(root.join("odd/.git"), "not a pointer").unwrap();
        let s = scan(root);
        let find = |name: &str| s.repos.iter().find(|r| r.entry == root.join(name).join(".git")).unwrap().clone();

        let main = find("main");
        assert!(main.entry.is_dir());
        assert_eq!((main.git_dir.clone(), main.common_dir.clone()), (root.join("main/.git"), root.join("main/.git")));

        let wt = find("wt");
        assert!(wt.entry.is_file());
        assert_eq!(wt.git_dir, root.join("main/.git/worktrees/w1"));
        assert_eq!(wt.common_dir, root.join("main/.git"));
        assert_eq!(
            wt.parts(),
            vec![
                root.join("main/.git/hooks"),
                root.join("main/.git/config"),
                root.join("main/.git/worktrees/w1/config.worktree"),
                root.join("main/.git/worktrees/w1/commondir"),
                root.join("main/.git/worktrees/w1/gitdir")
            ]
        );

        let module = find("mod");
        assert_eq!(module.git_dir, root.join("sub/.git/modules/m"));
        assert_eq!(module.common_dir, module.git_dir, "a submodule has no commondir file");

        let odd = find("odd");
        assert_eq!((odd.git_dir.clone(), odd.common_dir.clone()), (odd.entry.clone(), odd.entry.clone()));
    }
}
