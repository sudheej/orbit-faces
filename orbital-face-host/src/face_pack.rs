use anyhow::{bail, Context};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Component, PathBuf};

pub const FACE_PACK_KIND: &str = "orbital.face";
pub const FACE_PACK_VERSION: &str = "0.1";

#[derive(Debug, Clone, Deserialize)]
pub struct FaceManifest {
    pub kind: String,
    pub version: String,
    pub name: String,
    pub entry: PathBuf,
    pub window: FaceWindow,
    pub states: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FaceWindow {
    pub width: u32,
    pub height: u32,
    pub transparent: bool,
    pub borderless: bool,
    pub always_on_top: bool,
}

#[derive(Debug, Clone)]
pub struct FacePack {
    pub directory: PathBuf,
    pub manifest: FaceManifest,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LaunchOptions {
    pub face_dir: PathBuf,
    pub bridge_url: Option<String>,
}

impl FacePack {
    pub fn load(directory: PathBuf) -> anyhow::Result<Self> {
        let manifest_path = directory.join("manifest.json");
        let text = fs::read_to_string(&manifest_path)
            .with_context(|| format!("failed to read face manifest {}", manifest_path.display()))?;
        let manifest: FaceManifest = serde_json::from_str(&text)
            .with_context(|| format!("invalid face manifest {}", manifest_path.display()))?;
        manifest.validate()?;

        let entry_path = directory.join(&manifest.entry);
        if !entry_path.is_file() {
            bail!(
                "face pack entry script does not exist: {}",
                entry_path.display()
            );
        }

        Ok(Self {
            directory,
            manifest,
        })
    }

    pub fn entry_path(&self) -> PathBuf {
        self.directory.join(&self.manifest.entry)
    }

    pub fn supports_state(&self, state: &str) -> bool {
        self.manifest
            .states
            .iter()
            .any(|candidate| candidate == state)
    }

    pub fn resolve_switch_path(&self, requested: &str) -> PathBuf {
        let requested = PathBuf::from(requested);
        if requested.join("manifest.json").is_file() {
            return requested;
        }
        if requested.components().count() == 1 {
            if let Some(parent) = self.directory.parent() {
                let sibling = parent.join(&requested);
                if sibling.join("manifest.json").is_file() {
                    return sibling;
                }
            }
        }
        requested
    }
}

impl FaceManifest {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.kind != FACE_PACK_KIND {
            bail!(
                "unsupported face manifest kind {:?}; expected {:?}",
                self.kind,
                FACE_PACK_KIND
            );
        }
        if self.version != FACE_PACK_VERSION {
            bail!(
                "unsupported face manifest version {:?}; expected {:?}",
                self.version,
                FACE_PACK_VERSION
            );
        }
        if self.name.trim().is_empty() {
            bail!("face manifest name must not be empty");
        }
        if self.entry.as_os_str().is_empty()
            || self.entry.is_absolute()
            || self
                .entry
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            bail!("face manifest entry must stay inside the face pack directory");
        }
        if self.window.width == 0 || self.window.height == 0 {
            bail!("face window width and height must be greater than zero");
        }
        if !self.states.iter().any(|state| state == "idle") {
            bail!("face manifest states must include \"idle\"");
        }

        let mut seen = HashSet::new();
        for state in &self.states {
            if state.trim().is_empty() {
                bail!("face manifest states must not contain empty values");
            }
            if !seen.insert(state) {
                bail!("face manifest contains duplicate state {:?}", state);
            }
        }
        Ok(())
    }
}

pub fn launch_options_from_args() -> anyhow::Result<LaunchOptions> {
    let mut args = std::env::args().skip(1);
    let mut face = None;
    let mut bridge_url = None;

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--face" => {
                let value = args
                    .next()
                    .context("--face requires a directory path argument")?;
                face = Some(PathBuf::from(value));
            }
            "--bridge" => {
                bridge_url = Some(
                    args.next()
                        .context("--bridge requires a ws:// URL argument")?,
                );
            }
            "-h" | "--help" => {
                println!(
                    "Usage: orbital-face-host [--face <face-pack-directory>] [--bridge <ws://url>]"
                );
                std::process::exit(0);
            }
            _ => bail!("unknown argument {argument:?}; use --help for usage"),
        }
    }

    Ok(LaunchOptions {
        face_dir: face.unwrap_or_else(|| PathBuf::from("examples/basic_orb")),
        bridge_url,
    })
}

#[cfg(test)]
mod tests {
    use super::{FaceManifest, FacePack, FaceWindow, FACE_PACK_KIND, FACE_PACK_VERSION};
    use std::fs;
    use std::path::PathBuf;

    fn valid_manifest() -> FaceManifest {
        FaceManifest {
            kind: FACE_PACK_KIND.into(),
            version: FACE_PACK_VERSION.into(),
            name: "Test Face".into(),
            entry: PathBuf::from("main.lua"),
            window: FaceWindow {
                width: 260,
                height: 260,
                transparent: true,
                borderless: true,
                always_on_top: false,
            },
            states: vec!["idle".into(), "thinking".into()],
        }
    }

    #[test]
    fn valid_manifest_passes_validation() {
        valid_manifest().validate().unwrap();
    }

    #[test]
    fn parses_v0_manifest_json() {
        let manifest: FaceManifest = serde_json::from_str(
            r#"{
                "kind":"orbital.face",
                "version":"0.1",
                "name":"Test",
                "entry":"main.lua",
                "window":{
                    "width":260,
                    "height":260,
                    "transparent":true,
                    "borderless":true,
                    "always_on_top":false
                },
                "states":["idle","thinking"]
            }"#,
        )
        .unwrap();

        manifest.validate().unwrap();
        assert_eq!(manifest.entry, PathBuf::from("main.lua"));
    }

    #[test]
    fn unknown_kind_is_rejected() {
        let mut manifest = valid_manifest();
        manifest.kind = "other.face".into();
        assert!(manifest
            .validate()
            .unwrap_err()
            .to_string()
            .contains("kind"));
    }

    #[test]
    fn unknown_version_is_rejected() {
        let mut manifest = valid_manifest();
        manifest.version = "99".into();
        assert!(manifest
            .validate()
            .unwrap_err()
            .to_string()
            .contains("version"));
    }

    #[test]
    fn idle_state_is_required() {
        let mut manifest = valid_manifest();
        manifest.states = vec!["thinking".into()];
        assert!(manifest
            .validate()
            .unwrap_err()
            .to_string()
            .contains("idle"));
    }

    #[test]
    fn missing_entry_script_is_rejected_clearly() {
        let directory =
            std::env::temp_dir().join(format!("orbital-face-pack-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("manifest.json"),
            r#"{
                "kind":"orbital.face",
                "version":"0.1",
                "name":"Missing Script",
                "entry":"main.lua",
                "window":{
                    "width":260,
                    "height":260,
                    "transparent":true,
                    "borderless":true,
                    "always_on_top":false
                },
                "states":["idle"]
            }"#,
        )
        .unwrap();

        let error = FacePack::load(directory.clone()).unwrap_err().to_string();
        let _ = fs::remove_dir_all(directory);
        assert!(error.contains("entry script does not exist"));
    }
}
