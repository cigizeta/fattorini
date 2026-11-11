//! `arena release <player>@<tuned|untuned> [--dry-run]`: a binary release, built here and uploaded
//! to GitHub as a draft with `gh`, the developer's own login. Hidden like `snapshot`: not in the usage.
//!
//! A release is a snapshot's build (`snapshot::build_embedded`, the snapshot's rule) made at a tag,
//! with three differences: the description says the tag where a snapshot says the commit; the C
//! runtime is linked statically, so no Visual C++ Redistributable is needed where it runs; and the
//! third-party notices are embedded, `fattorini licenses` printing them. Only the `.exe` is
//! uploaded; the release's text is `RELEASE-NOTES.md`, committed before the tag and passed as it is.
//!
//! The version is CalVer, `YYYY.M.N`: the year, the month without a leading zero (Cargo refuses
//! one), and N counting that month's releases from 0. It must be the current UTC month's, dry run
//! included, so a release prepared in one month and published in the next cannot carry the old one.
//!
//! Every check comes before the upload, each refusal saying why: the player; the version's month;
//! HEAD exactly at a tag,
//! the tree unmodified, the tag `v<version>` or a candidate `v<version>-rc.<n>` of the workspace's
//! version (so `id name` and the tag never disagree), the notes naming that version (so a release
//! never carries the previous one's); `cargo about` and `gh` there, no release of the tag yet; then
//! the build, the `.exe` importing no `VCRUNTIME140.dll`, its `about` saying the description and its
//! `bench` passing. `gh release create --verify-tag` refuses a tag not yet pushed, rather than
//! creating one from the default branch. A tag seen by anyone is never moved nor reused: a broken
//! candidate's draft is deleted and the next candidate tagged.
//!
//! `--dry-run` builds and checks everything it can with no tag and no upload, as `v0.0.0-dry-run`.

use std::process;

use super::match_player::{self, Engines};
use super::snapshot::{self, EmbeddedBuild};

/// Where a release is built, apart from the snapshots' `target/snapshot`: other flags (the static
/// runtime) would otherwise rebuild everything each time one follows the other.
const RELEASE_TARGET_DIR: &str = "target/release-build";

/// The release's text, at the repository's root (`bin/arena` runs from there).
const NOTES: &str = "RELEASE-NOTES.md";

/// What a dry run calls itself, in place of a tag.
const DRY_RUN_TAG: &str = "v0.0.0-dry-run";

/// The version every crate of the workspace shares (`[workspace.package]`), the engine's `id name`.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// `arena release <player>@<tuned|untuned> [--dry-run]`: makes the draft, or says why not and exits.
pub(crate) fn command(rest: &[String]) {
    let (name, dry_run) = match rest {
        [name] => (name, false),
        [name, flag] if flag == "--dry-run" => (name, true),
        _ => {
            eprintln!("usage: arena release <player>@<tuned|untuned> [--dry-run], the player pesto or a generation's winner");
            process::exit(2);
        }
    };
    if let Err(message) = release(name, dry_run) {
        eprintln!("{message}");
        process::exit(1);
    }
}

fn release(name: &str, dry_run: bool) -> Result<(), String> {
    // 1. The player, as a snapshot's.
    let Some((player, _)) = name.split_once('@') else {
        return Err(format!("{name:?}: say what it searches with, {name}@tuned or {name}@untuned"));
    };
    snapshot::may_snapshot(player)?;
    let entry = match_player::resolve_player(name, false, &Engines::load()?)?;

    // 2. The version's month, then the tag and the notes.
    let now = time::OffsetDateTime::now_utc();
    check_month(VERSION, now.year(), u8::from(now.month()))?;
    let notes = std::fs::read_to_string(NOTES).map_err(|error| format!("{NOTES}: {error}"))?;
    let tag = if dry_run {
        DRY_RUN_TAG.to_string()
    } else {
        let tag = output("git", &["describe", "--exact-match", "--tags", "HEAD"])
            .map_err(|error| format!("HEAD is at no tag, and a release is made only at one: {error}"))?;
        if fattorini::pipeline::protocol::current_commit().contains("modified") {
            return Err("the working tree is modified: a release is built from its tag alone".to_string());
        }
        check_tag(&tag, VERSION)?;
        if !names_version(&notes, VERSION) {
            return Err(format!("{NOTES} does not name {VERSION}: still the previous release's?"));
        }
        tag
    };

    // 3. The tools, and no release of the tag yet.
    output("cargo", &["about", "--version"]).map_err(|error| format!("cargo-about is needed for the notices: {error}"))?;
    if !dry_run {
        output("gh", &["--version"]).map_err(|error| format!("gh is needed for the upload: {error}"))?;
        if output("gh", &["release", "view", &tag]).is_ok() {
            return Err(format!("{tag} already has a release on GitHub: a tag is never reused"));
        }
    }

    // 4. The notices, `about.toml` saying which licenses are accepted, for the engine's crate alone.
    std::fs::create_dir_all(RELEASE_TARGET_DIR).map_err(|error| format!("failed to create {RELEASE_TARGET_DIR}: {error}"))?;
    let notices = format!("{RELEASE_TARGET_DIR}/notices.txt");
    output(
        "cargo",
        &["about", "generate", "--manifest-path", "crates/fattorini/Cargo.toml", "--config", "about.toml", "--output-file", &notices, "about.hbs"],
    )
    .map_err(|error| format!("cargo about failed to write the notices: {error}"))?;

    // 5. The build.
    let stem = format!("fattorini-{tag}");
    let exe = format!("{RELEASE_TARGET_DIR}/{stem}.exe");
    let parameters_file = snapshot::parameters_file(entry.parameters, RELEASE_TARGET_DIR, &stem)?;
    let description = snapshot::describe_release(name, &tag, &snapshot::now_text());
    let build = EmbeddedBuild {
        network: entry.net.as_deref(),
        parameters: parameters_file.as_deref(),
        description: &description,
        notices: Some(&notices),
        target_dir: RELEASE_TARGET_DIR,
        static_runtime: true,
    };
    let built = snapshot::build_embedded(&build, &exe);
    if let Some(file) = &parameters_file {
        std::fs::remove_file(file).ok();
    }
    built?;

    // 6. The checks on the `.exe` itself.
    let bytes = std::fs::read(&exe).map_err(|error| format!("{exe}: {error}"))?;
    if imports_dynamic_runtime(&bytes) {
        return Err(format!("{exe} imports VCRUNTIME140.dll: the C runtime is not static"));
    }
    let about = output(&exe, &["about"])?;
    if !about.contains(&description) {
        return Err(format!("{exe} about says {about:?}, not the description {description:?}"));
    }
    println!("{about}");
    println!("running {exe} bench");
    let bench = output(&exe, &["bench"])?;
    println!("{bench}");

    // 7. The draft.
    if dry_run {
        println!("dry run passed: {exe}, not uploaded");
        return Ok(());
    }
    let title = format!("Fattorini {tag}");
    let status = process::Command::new("gh")
        .args(["release", "create", &tag, &exe, "--draft", "--verify-tag", "--title", &title, "--notes-file", NOTES])
        .status()
        .map_err(|error| format!("failed to run gh: {error}"))?;
    if !status.success() {
        return Err(format!("gh release create failed ({status}): no draft made"));
    }
    println!("drafted {tag}: read it on GitHub, then publish it there");
    Ok(())
}

/// The stdout of `program` with `args`, trimmed, or why it failed, with its stderr.
fn output(program: &str, args: &[&str]) -> Result<String, String> {
    let output = process::Command::new(program).args(args).output().map_err(|error| format!("failed to run {program}: {error}"))?;
    if !output.status.success() {
        return Err(format!("{program} {} failed ({}): {}", args.join(" "), output.status, String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Whether `version` is CalVer `YYYY.M.N` of the given year and month: `2026.11.0` in November 2026,
/// never `2026.10.3` nor `2026.011.0`.
fn check_month(version: &str, year: i32, month: u8) -> Result<(), String> {
    let prefix = format!("{year}.{month}.");
    let release = version.strip_prefix(&prefix);
    if release.is_some_and(|n| !n.is_empty() && n.bytes().all(|byte| byte.is_ascii_digit())) {
        return Ok(());
    }
    Err(format!(
        "the workspace's version is {version}, not this month's {prefix}<n>: set Cargo.toml's [workspace.package] version, N counting this month's releases from 0"
    ))
}

/// Whether `tag` is `v<version>` or a release candidate `v<version>-rc.<n>`: candidates share the
/// version they lead to, so they need no version change of their own.
fn check_tag(tag: &str, version: &str) -> Result<(), String> {
    let release = format!("v{version}");
    let candidate = tag.strip_prefix(&format!("{release}-rc.")).is_some_and(|n| !n.is_empty() && n.bytes().all(|byte| byte.is_ascii_digit()));
    if tag == release || candidate {
        return Ok(());
    }
    Err(format!("the tag {tag} is neither {release} nor {release}-rc.<n>: the workspace's version is {version}"))
}

/// Whether `text` names `version` as a whole: `2026.11.0` in `v2026.11.0` or `2026.11.0.` at a
/// sentence's end, never inside `12026.11.0` or `2026.11.01`.
fn names_version(text: &str, version: &str) -> bool {
    let bytes = text.as_bytes();
    text.match_indices(version).any(|(at, _)| {
        let before = at.checked_sub(1).map(|i| bytes[i]);
        let after = bytes.get(at + version.len()).copied();
        let after_next = bytes.get(at + version.len() + 1).copied();
        let joined_before = before.is_some_and(|byte| byte.is_ascii_digit() || byte == b'.');
        let joined_after = after.is_some_and(|byte| byte.is_ascii_digit())
            || (after == Some(b'.') && after_next.is_some_and(|byte| byte.is_ascii_digit()));
        !joined_before && !joined_after
    })
}

/// Whether an `.exe`'s bytes name the dynamic C runtime among its imports. Import names are ASCII
/// in the file, so a search does what `dumpbin /imports` would, with no Visual Studio shell.
fn imports_dynamic_runtime(bytes: &[u8]) -> bool {
    const NAME: &[u8] = b"vcruntime140";
    bytes.windows(NAME.len()).any(|window| window.eq_ignore_ascii_case(NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_the_current_months() {
        for version in ["2026.11.0", "2026.11.1", "2026.11.12"] {
            assert!(check_month(version, 2026, 11).is_ok(), "{version}");
        }
        assert!(check_month("2026.1.0", 2026, 1).is_ok());
        for version in ["2026.10.3", "2025.11.0", "2026.12.0", "2026.011.0", "2026.11", "2026.11.", "2026.11.x", "2026.110.0", "0.1.0"] {
            assert!(check_month(version, 2026, 11).is_err(), "{version}");
        }
    }

    #[test]
    fn a_tag_is_the_version_or_one_of_its_candidates() {
        for tag in ["v2026.11.0", "v2026.11.0-rc.1", "v2026.11.0-rc.12"] {
            assert!(check_tag(tag, "2026.11.0").is_ok(), "{tag}");
        }
        for tag in ["2026.11.0", "v2026.11.1", "v2026.11.0-rc", "v2026.11.0-rc.", "v2026.11.0-rc.x", "v2026.11.0-beta.1", "v2026.11.00"] {
            assert!(check_tag(tag, "2026.11.0").is_err(), "{tag}");
        }
    }

    #[test]
    fn the_notes_must_name_the_version_whole() {
        assert!(names_version("# Fattorini 2026.11.0\n", "2026.11.0"));
        assert!(names_version("its tag, `v2026.11.0`.", "2026.11.0"));
        assert!(names_version("released as 2026.11.0.", "2026.11.0"));
        assert!(!names_version("# Fattorini 2026.10.0\n", "2026.11.0"));
        assert!(!names_version("version 2026.11.01", "2026.11.0"));
        assert!(!names_version("version 12026.11.0", "2026.11.0"));
        assert!(!names_version("version 2026.11.0.1", "2026.11.0"));
    }

    #[test]
    fn the_dynamic_runtime_is_found_in_any_case() {
        assert!(imports_dynamic_runtime(b"\0KERNEL32.dll\0VCRUNTIME140.dll\0"));
        assert!(imports_dynamic_runtime(b"\0vcruntime140_1.dll\0"));
        assert!(!imports_dynamic_runtime(b"\0KERNEL32.dll\0ntdll.dll\0"));
    }
}
