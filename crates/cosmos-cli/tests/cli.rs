// SPDX-License-Identifier: GPL-3.0-or-later
//! Runs the real `cosmos` binary against a fake HOME and a stand-in `hypora-theme`.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, MutexGuard};

use image::{Rgb, RgbImage};

/// Linux refuses to exec a script another thread still has open for writing, so tests take turns.
fn turn() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    fn new() -> Env {
        let env = Env {
            dir: tempfile::tempdir().unwrap(),
        };
        fs::create_dir_all(env.home().join(".config")).unwrap();
        fs::create_dir_all(env.bin()).unwrap();
        let script = env.bin().join("hypora-theme");
        fs::write(
            &script,
            "#!/bin/sh\necho \"$1\" >> \"$CALLS\"\n[ \"$1\" = \"$FAIL\" ] && exit 1\n\
             mkdir -p \"$HOME/.config/hypora/current\" && echo \"$1\" > \"$HOME/.config/hypora/current/name\"\n",
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        env
    }

    fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }
    fn bin(&self) -> PathBuf {
        self.dir.path().join("bin")
    }
    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.dir.path().join("calls"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }
    fn hypora(&self) -> PathBuf {
        let p = self.home().join(".config/hypora");
        fs::create_dir_all(&p).unwrap();
        p
    }
    fn set_current(&self, name: &str) {
        let c = self.hypora().join("current");
        fs::create_dir_all(&c).unwrap();
        fs::write(c.join("name"), format!("{name}\n")).unwrap();
    }
    fn wallpaper(&self) -> PathBuf {
        let path = self.dir.path().join("wall.png");
        let img = RgbImage::from_fn(96, 64, |x, y| match (x / 32, y / 32) {
            (0, 0) => Rgb([18, 20, 28]),
            (1, 0) => Rgb([220, 60, 50]),
            (2, 0) => Rgb([60, 110, 220]),
            (0, 1) => Rgb([60, 190, 80]),
            (1, 1) => Rgb([230, 190, 50]),
            _ => Rgb([20, 22, 30]),
        });
        img.save(&path).unwrap();
        path
    }
    fn config(&self, text: &str) {
        let d = self.home().join(".config/cosmos");
        fs::create_dir_all(d.join("templates")).unwrap();
        fs::write(d.join("config.toml"), text).unwrap();
    }
    fn template(&self, name: &str, text: &str) {
        let d = self.home().join(".config/cosmos/templates");
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join(name), text).unwrap();
    }
    fn run(&self, args: &[&str]) -> Output {
        self.run_with(args, &[])
    }
    fn command(&self, args: &[&str], extra: &[(&str, &str)]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_cosmos"));
        cmd.args(args)
            .env_clear()
            .env("HOME", self.home())
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin().display()))
            .env("CALLS", self.dir.path().join("calls"));
        for (k, v) in extra {
            cmd.env(k, v);
        }
        cmd
    }
    fn run_with(&self, args: &[&str], extra: &[(&str, &str)]) -> Output {
        self.command(args, extra).output().unwrap()
    }
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}
fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}
fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

#[test]
fn bare_run_shows_the_palette_and_writes_nothing() {
    let _t = turn();
    let e = Env::new();
    let o = e.run(&[&s(&e.wallpaper())]);
    assert!(o.status.success(), "{}", err(&o));
    assert!(out(&o).contains("background") && out(&o).contains("bright_white"));
    assert!(!e.home().join(".config/hypora").exists());
    assert!(!e.home().join(".config/cosmos").exists());
}

#[test]
fn saving_needs_hypora_to_exist() {
    let _t = turn();
    let e = Env::new();
    let o = e.run(&[&s(&e.wallpaper()), "--name", "Dusk"]);
    assert!(!o.status.success());
    assert!(err(&o).contains("Hypora was not found"), "{}", err(&o));
    assert!(!e.home().join(".config/hypora").exists());
}

#[test]
fn writes_a_theme_and_refuses_to_replace_it_without_force() {
    let _t = turn();
    let e = Env::new();
    e.hypora();
    let w = s(&e.wallpaper());
    let o = e.run(&[&w, "--name", "Dusk"]);
    assert!(o.status.success(), "{}", err(&o));
    let dir = e.hypora().join("themes/Dusk");
    assert!(dir.join("colors.toml").is_file() && dir.join("backgrounds/wall.png").is_file());
    assert!(
        e.calls().is_empty(),
        "hypora-theme must not run without --apply"
    );

    let again = e.run(&[&w, "--name", "Dusk"]);
    assert!(
        !again.status.success() && err(&again).contains("--force"),
        "{}",
        err(&again)
    );
    assert!(e.run(&[&w, "--name", "Dusk", "--force"]).status.success());
    assert!(dir.join("colors.toml.bak").is_file());
}

#[test]
fn dry_run_writes_nothing() {
    let _t = turn();
    let e = Env::new();
    e.hypora();
    let o = e.run(&[&s(&e.wallpaper()), "--name", "Dusk", "--dry-run", "--apply"]);
    assert!(o.status.success(), "{}", err(&o));
    assert!(out(&o).contains("would write"));
    assert!(!e.hypora().join("themes").exists());
    assert!(e.calls().is_empty());
}

#[test]
fn hostile_theme_names_are_refused_before_anything_happens() {
    let _t = turn();
    let e = Env::new();
    e.hypora();
    let w = s(&e.wallpaper());
    for bad in [
        "../x",
        "a/b",
        "a;b",
        "$(touch pwned)",
        "a\nb",
        "",
        ".hidden",
        "x|e",
    ] {
        let o = e.run(&[&w, "--name", bad, "--apply"]);
        assert!(!o.status.success(), "{bad:?} was accepted");
    }
    assert!(!e.hypora().join("themes").exists());
    assert!(e.calls().is_empty());
    assert!(!e.home().join("pwned").exists());
}

#[test]
fn apply_runs_hypora_theme_and_says_how_to_go_back() {
    let _t = turn();
    let e = Env::new();
    e.set_current("Old");
    let o = e.run(&[&s(&e.wallpaper()), "--name", "New", "--apply"]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(e.calls(), ["New"]);
    assert!(
        out(&o).contains("to go back: hypora-theme Old"),
        "{}",
        out(&o)
    );
}

#[test]
fn failed_apply_switches_back_to_the_previous_theme() {
    let _t = turn();
    let e = Env::new();
    e.set_current("Old");
    let o = e.run_with(
        &[&s(&e.wallpaper()), "--name", "New", "--apply"],
        &[("FAIL", "New")],
    );
    assert!(!o.status.success());
    assert!(err(&o).contains("switched back to Old"), "{}", err(&o));
    assert_eq!(e.calls(), ["New", "Old"]);
    assert_eq!(
        fs::read_to_string(e.hypora().join("current/name"))
            .unwrap()
            .trim(),
        "Old"
    );
}

#[test]
fn flags_that_need_a_name_are_rejected_without_one() {
    let _t = turn();
    let e = Env::new();
    let w = s(&e.wallpaper());
    for flag in ["--apply", "--force"] {
        let o = e.run(&[&w, flag]);
        assert!(!o.status.success(), "{flag}");
    }
}

#[test]
fn templates_are_filled_and_hooks_get_arguments_not_a_shell() {
    let _t = turn();
    let e = Env::new();
    let target = e.dir.path().join("a b;c");
    e.template(
        "k.conf",
        "bg={background.strip}\nfg={foreground}\n# {mode}\n",
    );
    e.config(&format!(
        "[[template]]\nsrc = \"k.conf\"\ndest = \"~/out/k.conf\"\npost = [\"touch\", \"{}\"]\n",
        target.display()
    ));
    let o = e.run(&[&s(&e.wallpaper()), "--templates", "--mode", "dark"]);
    assert!(o.status.success(), "{}", err(&o));
    let text = fs::read_to_string(e.home().join("out/k.conf")).unwrap();
    assert!(
        text.starts_with("bg=") && text.contains("fg=#") && text.contains("# dark"),
        "{text}"
    );
    assert!(
        target.exists(),
        "the hook's argument must arrive as one literal string"
    );
}

#[test]
fn a_typo_in_any_template_writes_nothing() {
    let _t = turn();
    let e = Env::new();
    e.template("good.conf", "{background}\n");
    e.template("bad.conf", "{backgroud}\n");
    e.config(
        "[[template]]\nsrc = \"good.conf\"\ndest = \"~/out/good\"\n[[template]]\nsrc = \"bad.conf\"\ndest = \"~/out/bad\"\n",
    );
    let o = e.run(&[&s(&e.wallpaper()), "--templates"]);
    assert!(!o.status.success());
    assert!(err(&o).contains("backgroud"), "{}", err(&o));
    assert!(!e.home().join("out").exists());
}

#[test]
fn a_template_cannot_reach_outside_the_templates_folder() {
    let _t = turn();
    let e = Env::new();
    e.config("[[template]]\nsrc = \"../config.toml\"\ndest = \"~/out/x\"\n");
    let o = e.run(&[&s(&e.wallpaper()), "--templates"]);
    assert!(!o.status.success() && err(&o).contains(".."), "{}", err(&o));
    assert!(!e.home().join("out").exists());
}

#[test]
fn json_goes_to_stdout_alone_and_mode_can_be_forced() {
    let _t = turn();
    let e = Env::new();
    e.hypora();
    let o = e.run(&[
        &s(&e.wallpaper()),
        "--json",
        "--mode",
        "light",
        "--name",
        "J",
    ]);
    assert!(o.status.success(), "{}", err(&o));
    let text = out(&o);
    assert!(
        text.trim_start().starts_with('{') && text.trim_end().ends_with('}'),
        "{text}"
    );
    assert!(text.contains("\"mode\": \"light\""));
    assert!(
        err(&o).contains("wrote"),
        "messages belong on stderr with --json"
    );
}

#[test]
fn bad_inputs_give_a_clear_error_not_a_crash() {
    let _t = turn();
    let e = Env::new();
    let junk = e.dir.path().join("junk.png");
    fs::write(&junk, b"not an image").unwrap();
    let o = e.run(&[&s(&junk)]);
    assert!(
        !o.status.success() && err(&o).starts_with("cosmos: "),
        "{}",
        err(&o)
    );
    assert!(!err(&o).contains("panicked"));
    let o = e.run(&["/nonexistent/wall.png"]);
    assert!(!o.status.success() && !err(&o).contains("panicked"));
    e.config("mode = [");
    let o = e.run(&[&s(&e.wallpaper())]);
    assert!(
        !o.status.success() && err(&o).contains("config.toml"),
        "{}",
        err(&o)
    );
}

#[test]
fn a_template_cannot_write_outside_the_home_directory() {
    let _t = turn();
    let e = Env::new();
    let outside = e.dir.path().join("outside.conf");
    e.template("k.conf", "{background}\n");
    for dest in [
        outside.to_string_lossy().into_owned(),
        "~/../escape.conf".to_string(),
        "/etc/cosmos-test".to_string(),
    ] {
        e.config(&format!(
            "[[template]]\nsrc = \"k.conf\"\ndest = {dest:?}\n"
        ));
        let o = e.run(&[&s(&e.wallpaper()), "--templates"]);
        assert!(
            !o.status.success() && err(&o).contains("inside your home"),
            "{dest}: {}",
            err(&o)
        );
    }
    assert!(!outside.exists());
    assert!(!e.home().join("escape.conf").exists() && !e.dir.path().join("escape.conf").exists());
    assert!(!Path::new("/etc/cosmos-test").exists());
}

#[test]
fn a_fifo_wallpaper_or_template_is_refused_instead_of_hanging() {
    let _t = turn();
    let e = Env::new();
    let fifo = e.dir.path().join("pipe.png");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    for args in [
        vec![s(&fifo)],
        vec![s(&e.wallpaper()), "--templates".into()],
    ] {
        if args.len() == 2 {
            e.template("k.conf", "x");
            fs::remove_file(e.home().join(".config/cosmos/templates/k.conf")).unwrap();
            let tpl = e.home().join(".config/cosmos/templates/k.conf");
            assert!(Command::new("mkfifo").arg(&tpl).status().unwrap().success());
            e.config("[[template]]\nsrc = \"k.conf\"\ndest = \"~/out/k\"\n");
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let mut child = e
            .command(&refs, &[])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let status = loop {
            if let Some(st) = child.try_wait().unwrap() {
                break st;
            }
            if std::time::Instant::now() > deadline {
                let _ = child.kill();
                panic!("cosmos hung on a FIFO: {args:?}");
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        assert!(!status.success(), "{args:?}");
    }
}

#[test]
fn an_oversized_template_is_refused() {
    let _t = turn();
    let e = Env::new();
    e.template("big.conf", &"x".repeat(2 * 1024 * 1024));
    e.config("[[template]]\nsrc = \"big.conf\"\ndest = \"~/out/big\"\n");
    let o = e.run(&[&s(&e.wallpaper()), "--templates"]);
    assert!(
        !o.status.success() && !e.home().join("out").exists(),
        "{}",
        err(&o)
    );
}

#[test]
fn hooks_are_shown_before_they_run_and_in_a_dry_run() {
    let _t = turn();
    let e = Env::new();
    let marker = e.dir.path().join("ran");
    e.template("k.conf", "{background}\n");
    e.config(&format!(
        "[[template]]\nsrc = \"k.conf\"\ndest = \"~/out/k\"\npost = [\"touch\", \"{}\"]\n",
        marker.display()
    ));
    let w = s(&e.wallpaper());
    let dry = e.run(&[&w, "--templates", "--dry-run"]);
    assert!(out(&dry).contains("would run: touch"), "{}", out(&dry));
    assert!(!marker.exists() && !e.home().join("out").exists());
    let real = e.run(&[&w, "--templates"]);
    assert!(
        out(&real).contains("running: touch") && marker.exists(),
        "{}",
        out(&real)
    );
}

#[test]
fn terminal_escape_codes_in_file_names_are_masked_in_messages() {
    let _t = turn();
    let e = Env::new();
    let evil = e.dir.path().join("a\x1b]0;pwned\x07b.png");
    fs::write(&evil, b"junk").unwrap();
    let o = e.run(&[&s(&evil)]);
    assert!(!o.status.success());
    assert!(
        !err(&o).contains('\x1b') && !err(&o).contains('\x07'),
        "{:?}",
        err(&o)
    );
}
