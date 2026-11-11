//! Paths a shell command creates, read from the command line an agent ran.

use std::path::{Path, PathBuf};

/// Split a command line into words, honouring quotes. Operators that end a
/// simple command become words of their own.
fn shell_words(command: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut chars = command.chars().peekable();
    let flush = |word: &mut String, words: &mut Vec<String>| {
        if !word.is_empty() {
            words.push(std::mem::take(word));
        }
    };
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '\'' | '"') => quote = Some(c),
            (None, '\\') => {
                if let Some(next) = chars.next() {
                    word.push(next);
                }
            }
            (None, c) if c.is_whitespace() => flush(&mut word, &mut words),
            (None, ';' | '|' | '&' | '(' | ')') => {
                flush(&mut word, &mut words);
                words.push(";".to_owned());
            }
            (None, '>') => {
                flush(&mut word, &mut words);
                while chars.peek() == Some(&'>') {
                    chars.next();
                }
                words.push(">".to_owned());
            }
            (_, c) => word.push(c),
        }
    }
    flush(&mut word, &mut words);
    words
}

pub fn expand(word: &str, cwd: Option<&Path>) -> Option<PathBuf> {
    let home = dirs::home_dir();
    let expanded = if let Some(rest) = word.strip_prefix("~/") {
        home?.join(rest)
    } else if let Some(rest) = word
        .strip_prefix("$HOME/")
        .or_else(|| word.strip_prefix("${HOME}/"))
    {
        home?.join(rest)
    } else if let Some(rest) = word
        .strip_prefix("$TMPDIR/")
        .or_else(|| word.strip_prefix("${TMPDIR}/"))
    {
        PathBuf::from(std::env::var_os("TMPDIR")?).join(rest)
    } else if word.starts_with('$') || word.contains("$(") {
        return None;
    } else {
        let path = PathBuf::from(word);
        if path.is_absolute() {
            path
        } else {
            cwd?.join(path)
        }
    };
    Some(expanded)
}

/// Options of `git clone` and `git worktree add` whose value is the next word.
const FLAGS_WITH_VALUES: &[&str] = &[
    "--depth",
    "-b",
    "-B",
    "--branch",
    "--origin",
    "--reference",
    "--filter",
    "-c",
    "--config",
    "-j",
    "--jobs",
    "--template",
    "--separate-git-dir",
    "--shallow-since",
    "--shallow-exclude",
    "--reason",
];

/// Paths a shell command creates: `mkdir` operands, clone and worktree
/// destinations, build output directories, copy targets and redirections.
pub fn created_by_command(command: &str, cwd: Option<&Path>) -> Vec<PathBuf> {
    const VALUE_FLAGS: &[&str] = &["--target-dir", "--out-dir", "--output", "-o", "--prefix"];
    let words = shell_words(command);
    let mut created = Vec::new();
    let mut push = |word: &str| {
        if let Some(path) = expand(word, cwd) {
            created.push(path);
        }
    };

    for simple in words.split(|w| w == ";") {
        let mut i = 0;
        while i < simple.len() {
            let word = simple[i].as_str();
            if word == ">" {
                if let Some(target) = simple.get(i + 1) {
                    push(target);
                }
                i += 2;
                continue;
            }
            if let Some(value) = word
                .strip_prefix("CARGO_TARGET_DIR=")
                .or_else(|| word.strip_prefix("--target-dir="))
                .or_else(|| word.strip_prefix("--out-dir="))
            {
                push(value);
            } else if VALUE_FLAGS.contains(&word)
                && let Some(value) = simple.get(i + 1)
            {
                push(value);
                i += 1;
            }
            i += 1;
        }

        let mut positional: Vec<&str> = Vec::new();
        let mut words = simple.iter().map(String::as_str).take_while(|w| *w != ">");
        while let Some(word) = words.next() {
            if FLAGS_WITH_VALUES.contains(&word) {
                words.next();
            } else if !word.starts_with('-') && !word.contains('=') {
                positional.push(word);
            }
        }
        match positional.as_slice() {
            ["mkdir", dirs @ ..] => dirs.iter().for_each(|d| push(d)),
            ["git", "clone", _, dest, ..]
            | ["git", "worktree", "add", dest, ..]
            | ["cp" | "rsync" | "mv" | "ln", .., _, dest] => push(dest),
            ["tee", files @ ..] => files.iter().for_each(|f| push(f)),
            _ => {}
        }
    }
    created
}

/// Paths an `apply_patch` envelope adds or moves files to.
///
/// Codex, opencode and Cline all use the same grammar: `*** Add File: <path>`
/// and `*** Move to: <path>` name what the patch creates.
pub fn patch_targets(patch: &str) -> impl Iterator<Item = &str> {
    patch.lines().filter_map(|line| {
        line.strip_prefix("*** Add File: ")
            .or_else(|| line.strip_prefix("*** Move to: "))
            .map(str::trim)
    })
}

/// One command line from an argv: the script of `sh -c` / `bash -lc`, or the
/// words joined.
pub fn argv_command(argv: &[String]) -> String {
    match argv {
        [shell, flag, script, ..]
            if shell
                .rsplit('/')
                .next()
                .is_some_and(|s| matches!(s, "sh" | "bash" | "zsh" | "dash" | "fish"))
                && matches!(flag.as_str(), "-c" | "-lc" | "-ic" | "-lic") =>
        {
            script.clone()
        }
        _ => argv.join(" "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn created(command: &str) -> Vec<String> {
        created_by_command(command, Some(Path::new("/work/proj")))
            .into_iter()
            .map(|p| p.display().to_string())
            .collect()
    }

    #[test]
    fn creating_commands_name_what_they_create() {
        assert_eq!(
            created("mkdir -p /tmp/a /tmp/b && cd /tmp/a"),
            ["/tmp/a", "/tmp/b"]
        );
        assert_eq!(
            created("git clone --depth 1 https://example.com/acme.git /tmp/acme"),
            ["/tmp/acme"]
        );
        assert_eq!(
            created("git worktree add -b feature /tmp/wt main"),
            ["/tmp/wt"]
        );
        assert_eq!(
            created("CARGO_TARGET_DIR=/tmp/t cargo build --release"),
            ["/tmp/t"]
        );
        assert_eq!(created("cargo build --target-dir /tmp/t2"), ["/tmp/t2"]);
        assert_eq!(created("cp -r src /tmp/copy"), ["/tmp/copy"]);
        assert_eq!(created("just test > /tmp/out.log 2>&1"), ["/tmp/out.log"]);
        assert_eq!(created("echo 'a > b' | tee \"/tmp/x y\""), ["/tmp/x y"]);
        assert_eq!(created("mkdir build"), ["/work/proj/build"]);
    }

    #[test]
    fn reading_commands_create_nothing() {
        assert_eq!(created("ls -la /tmp/a"), Vec::<String>::new());
        assert_eq!(created("cat /tmp/a | grep x"), Vec::<String>::new());
        assert_eq!(created("mkdir \"$(mktemp -d)\""), Vec::<String>::new());
    }

    #[test]
    fn patches_name_the_files_they_add_or_move() {
        let patch = "*** Begin Patch\n*** Add File: /tmp/a/new.txt\n+hi\n*** Update File: src/lib.rs\n*** Move to: /tmp/b/moved.rs\n*** End Patch";
        assert_eq!(
            patch_targets(patch).collect::<Vec<_>>(),
            ["/tmp/a/new.txt", "/tmp/b/moved.rs"]
        );
    }

    #[test]
    fn a_shell_wrapper_yields_its_script() {
        let argv = |words: &[&str]| words.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            argv_command(&argv(&["/bin/zsh", "-lc", "mkdir /tmp/x"])),
            "mkdir /tmp/x"
        );
        assert_eq!(
            argv_command(&argv(&["mkdir", "-p", "/tmp/y"])),
            "mkdir -p /tmp/y"
        );
    }
}
