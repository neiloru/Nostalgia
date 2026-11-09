//! Starting games with their emulator, play time and save sync around a session (port of launcher.ts).

use crate::client::{ApiError, Client};
use crate::model::{LaunchEmulator, LaunchOptions, RunningGame};
use crate::util::{iso_now, resolve};
use nostalgia_shared::{EmulatorSummary, ExpandContext, ItemKind, OsKey, build_arguments, disc_label, expand_string, launch_candidates, platform_info, split_args};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct LauncherRuntime {
    pid: Option<u32>,
}

impl Client {
    pub fn launcher_running(&self) -> bool {
        self.launcher_rt.lock().unwrap().pid.is_some()
    }

    fn emulators_for(&self, platform: &str) -> Vec<EmulatorSummary> {
        self.lock().cache().emulators.into_iter().filter(|e| e.platform == platform).collect()
    }

    fn game_platform(&self, game_id: i64) -> Option<String> {
        self.game(game_id).map(|g| g.platform).or_else(|| self.installed(ItemKind::Game, game_id).map(|i| i.platform))
    }

    /// Emulator used for a game: per-game choice, else an installed one, else the first available.
    pub fn pick_emulator(&self, game_id: i64) -> Option<EmulatorSummary> {
        let platform = self.game_platform(game_id)?;
        let list: Vec<EmulatorSummary> =
            self.emulators_for(&platform).into_iter().filter(|e| e.config.as_ref().is_some_and(|c| c.systems.get(self.os).is_some())).collect();
        let chosen = self.lock().game_settings.data().get(&game_id).and_then(|s| s.emulator_id);
        list.iter()
            .find(|e| Some(e.id) == chosen)
            .or_else(|| list.iter().find(|e| self.installed(ItemKind::Emulator, e.id).is_some()))
            .or_else(|| list.first())
            .cloned()
    }

    pub fn launch_options(&self, game_id: i64) -> LaunchOptions {
        let platform = self.game_platform(game_id);
        let settings = self.lock().game_settings.data().get(&game_id).cloned().unwrap_or_default();
        let emus = platform.as_deref().map(|p| self.emulators_for(p)).unwrap_or_default();
        let emu = self.pick_emulator(game_id);
        let inst = self.installed(ItemKind::Game, game_id);
        let exts: Vec<String> = emu
            .as_ref()
            .and_then(|e| e.config.as_ref())
            .and_then(|c| c.extensions.clone())
            .unwrap_or_else(|| platform_info(platform.as_deref().unwrap_or("")).ext);
        let discs = inst.map(|i| launch_candidates(&i.files.iter().map(|f| f.path.clone()).collect::<Vec<_>>(), &exts)).unwrap_or_default();
        let multi = discs.len() > 1;
        LaunchOptions {
            emulators: emus
                .iter()
                .map(|e| LaunchEmulator {
                    id: e.id,
                    name: e.name.clone(),
                    installed: self.installed(ItemKind::Emulator, e.id).is_some(),
                    supported: e.config.as_ref().is_some_and(|c| c.systems.get(self.os).is_some()),
                })
                .collect(),
            emulator_id: emu.map(|e| e.id),
            disc: settings.disc.clone().filter(|d| discs.contains(d)).or_else(|| discs.first().cloned()),
            discs: discs.into_iter().map(|f| (f.clone(), if multi { disc_label(&f) } else { f })).collect(),
            extra_args: settings.extra_args,
        }
    }

    pub async fn launch(self: &Arc<Self>, game_id: i64) -> Result<(), ApiError> {
        if self.launcher_running() {
            let name = self.lock().running.as_ref().map(|r| r.name.clone()).unwrap_or_else(|| "A game".into());
            return Err(ApiError::new(0, format!("{name} is already running")));
        }
        let inst = self.installed(ItemKind::Game, game_id).ok_or_else(|| ApiError::new(0, "The game is not installed"))?;
        let emu = self
            .pick_emulator(game_id)
            .ok_or_else(|| ApiError::new(0, format!("No emulator for {} is available on the server", platform_info(&inst.platform).name)))?;
        let sys = emu
            .config
            .as_ref()
            .and_then(|c| c.systems.get(self.os).cloned())
            .ok_or_else(|| ApiError::new(0, format!("{} has no {} configuration in its emu.yaml", emu.name, self.os.as_str())))?;
        let Some(emu_inst) = self.installed(ItemKind::Emulator, emu.id) else {
            let _ = self.enqueue(ItemKind::Emulator, emu.id, None).await;
            return Err(ApiError::new(0, format!("{} is not installed yet - it was added to the downloads", emu.name)));
        };

        let opts = self.launch_options(game_id);
        let disc = opts.disc.clone().ok_or_else(|| ApiError::new(0, "No launchable file found in the game folder"))?;
        let rom = disc.split('/').fold(PathBuf::from(&inst.path), |p, s| p.join(s));

        // Make sure we play with the newest cloud save.
        if self.settings().auto_sync_saves && self.is_online()
            && let Err(e) = self.sync_down(emu.id, "launch").await
        {
            self.toast(format!("Could not check cloud saves: {e}"), true);
        }
        for p in self.save_paths(emu.id) {
            let _ = std::fs::create_dir_all(p);
        }

        let mut vars = self.path_vars(&emu_inst.path);
        let rom_s = rom.to_string_lossy().into_owned();
        vars.insert("rom".into(), rom_s.clone());
        vars.insert("romDir".into(), rom.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default());
        vars.insert("gameDir".into(), inst.path.clone());
        vars.insert("disc".into(), disc.clone());
        let env: HashMap<String, String> = std::env::vars().collect();
        let home = self.paths.home.to_string_lossy().into_owned();
        let ctx = ExpandContext { vars: &vars, env: &env, home: &home, os: self.os };
        let exe = resolve(Path::new(&emu_inst.path), &expand_string(&sys.executable, &ctx));
        if !exe.exists() {
            return Err(ApiError::new(0, format!("Emulator executable not found: {}", exe.display())));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755));
        }
        let mut args = build_arguments(&sys, &ctx);
        if let Some(extra) = opts.extra_args.as_deref().filter(|s| !s.is_empty()) {
            args.extend(split_args(extra));
        }
        let cwd = match &sys.working_directory {
            Some(w) => resolve(Path::new(&emu_inst.path), &expand_string(w, &ctx)),
            None => PathBuf::from(&emu_inst.path),
        };
        let mut cmd = tokio::process::Command::new(&exe);
        cmd.args(&args).current_dir(&cwd).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        for (k, v) in sys.env.iter().flatten() {
            cmd.env(k, expand_string(v, &ctx));
        }
        // AppImages launched from inside an AppImage must not inherit our mount variables.
        cmd.env_remove("APPIMAGE").env_remove("APPDIR");
        // Started from Steam, the Linux launcher keeps Steam's overlay out of the launcher; emulators get it back.
        if let Ok(p) = std::env::var("NOSTALGIA_LD_PRELOAD") {
            cmd.env("LD_PRELOAD", p);
        }
        cmd.env_remove("NOSTALGIA_LD_PRELOAD");

        let started_at = iso_now();
        let started = Instant::now();
        let name = self.game(game_id).map(|g| g.name).unwrap_or(inst.name.clone());
        eprintln!("[launch] {} {}", exe.display(), args.iter().map(|a| format!("{a:?}")).collect::<Vec<_>>().join(" "));
        let mut child = cmd.spawn().map_err(|e| ApiError::new(0, format!("Could not start {}: {e}", emu.name)))?;
        let pid = child.id();
        self.launcher_rt.lock().unwrap().pid = pid.or(Some(0));
        let running = RunningGame { game_id, emulator_id: emu.id, name: name.clone(), started_at: started_at.clone(), pid };
        self.lock().running = Some(running.clone());
        self.heartbeat(Some(running));
        self.emit_state();
        let fullscreen = self.lock().fullscreen;
        if self.settings().minimize_on_launch && !fullscreen {
            self.minimize();
        }

        // Do not block the caller until the emulator exits.
        let me = self.clone();
        self.rt.spawn(async move {
            let mut beat = tokio::time::interval(Duration::from_secs(30));
            beat.tick().await;
            let status = loop {
                tokio::select! {
                    s = child.wait() => break s,
                    _ = beat.tick() => {
                        let r = me.lock().running.clone();
                        me.heartbeat(r);
                    }
                }
            };
            let seconds = started.elapsed().as_secs_f64();
            me.launcher_rt.lock().unwrap().pid = None;
            me.lock().running = None;
            me.heartbeat(None);
            me.emit_state();
            me.restore();
            match status {
                Err(e) => me.toast(format!("Could not start {}: {e}", emu.name), true),
                Ok(s) => {
                    if seconds < 3.0 && s.code().is_some_and(|c| c != 0) {
                        me.toast(format!("{} exited immediately (code {})", emu.name, s.code().unwrap_or(0)), true);
                    }
                    me.record_session(game_id, &started_at, seconds);
                    me.sync_up(emu.id, &format!("{name} ({} min)", (seconds / 60.0).round() as i64)).await;
                }
            }
        });
        Ok(())
    }

    pub fn stop_game(&self) {
        let Some(pid) = self.launcher_rt.lock().unwrap().pid.filter(|p| *p != 0) else { return };
        if self.os == OsKey::Windows {
            // Kill the whole process tree on Windows.
            let mut c = std::process::Command::new("taskkill");
            c.args(["/pid", &pid.to_string(), "/T", "/F"]).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
            }
            let _ = c.spawn();
        } else {
            let _ = std::process::Command::new("kill").args(["-TERM", &pid.to_string()]).spawn();
        }
    }
}
