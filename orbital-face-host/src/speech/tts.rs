use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub trait TextToSpeechProvider {
    fn name(&self) -> &'static str;
    fn enabled(&self) -> bool;
    fn speak(&self, text: &str) -> anyhow::Result<()>;
    fn synthesize_to_wav(&self, _text: &str, _output_path: &Path) -> anyhow::Result<()> {
        anyhow::bail!("{} does not support WAV synthesis", self.name())
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopTtsProvider;

impl TextToSpeechProvider for NoopTtsProvider {
    fn name(&self) -> &'static str {
        "none"
    }

    fn enabled(&self) -> bool {
        false
    }

    fn speak(&self, _text: &str) -> anyhow::Result<()> {
        anyhow::bail!("TTS is disabled. Start runtime with --tts piper or --tts windows-sapi.")
    }
}

#[derive(Debug, Clone)]
pub struct PiperTtsProvider {
    binary: PathBuf,
    model: PathBuf,
    config: Option<PathBuf>,
}

impl PiperTtsProvider {
    pub fn new(
        binary: Option<PathBuf>,
        model: Option<PathBuf>,
        config: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        let binary = binary.ok_or_else(|| anyhow::anyhow!("--piper-bin is required"))?;
        let model = model.ok_or_else(|| anyhow::anyhow!("--piper-model is required"))?;
        Ok(Self {
            binary,
            model,
            config,
        })
    }
}

impl TextToSpeechProvider for PiperTtsProvider {
    fn name(&self) -> &'static str {
        "piper"
    }

    fn enabled(&self) -> bool {
        true
    }

    fn speak(&self, text: &str) -> anyhow::Result<()> {
        let path = temporary_wav_path("piper");
        let result = self
            .synthesize_to_wav(text, &path)
            .and_then(|_| play_wav(&path));
        let _ = std::fs::remove_file(path);
        result
    }

    fn synthesize_to_wav(&self, text: &str, output_path: &Path) -> anyhow::Result<()> {
        let mut command = Command::new(&self.binary);
        command
            .arg("--model")
            .arg(&self.model)
            .arg("--output_file")
            .arg(output_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        if let Some(config) = &self.config {
            command.arg("--config").arg(config);
        }
        let mut child = command.spawn().map_err(|error| {
            anyhow::anyhow!("failed to start Piper {}: {error}", self.binary.display())
        })?;
        child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Piper stdin was unavailable"))?
            .write_all(text.as_bytes())?;
        let output = child.wait_with_output()?;
        anyhow::ensure!(
            output.status.success(),
            "Piper failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        anyhow::ensure!(output_path.is_file(), "Piper did not create a WAV file");
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsSapiTtsProvider;

impl TextToSpeechProvider for WindowsSapiTtsProvider {
    fn name(&self) -> &'static str {
        "windows-sapi"
    }

    fn enabled(&self) -> bool {
        cfg!(windows)
    }

    fn speak(&self, text: &str) -> anyhow::Result<()> {
        speak_windows_sapi(text)
    }
}

#[cfg(windows)]
fn speak_windows_sapi(text: &str) -> anyhow::Result<()> {
    let script = "Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; $s.Speak([Console]::In.ReadToEnd())";
    let mut child = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| anyhow::anyhow!("failed to start Windows SAPI: {error}"))?;
    child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("Windows SAPI stdin was unavailable"))?
        .write_all(text.as_bytes())?;
    let output = child.wait_with_output()?;
    anyhow::ensure!(
        output.status.success(),
        "Windows SAPI failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

#[cfg(not(windows))]
fn speak_windows_sapi(_text: &str) -> anyhow::Result<()> {
    anyhow::bail!("Windows SAPI TTS is unsupported on this platform")
}

fn play_wav(path: &Path) -> anyhow::Result<()> {
    #[cfg(windows)]
    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(New-Object Media.SoundPlayer $args[0]).PlaySync()",
        ])
        .arg(path)
        .status();
    #[cfg(target_os = "macos")]
    let status = Command::new("afplay").arg(path).status();
    #[cfg(all(unix, not(target_os = "macos")))]
    let status = Command::new("aplay").arg(path).status();

    let status =
        status.map_err(|error| anyhow::anyhow!("failed to start WAV playback: {error}"))?;
    anyhow::ensure!(status.success(), "WAV playback command failed");
    Ok(())
}

fn temporary_wav_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "orbital-{prefix}-{}-{}.wav",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_tts_reports_disabled() {
        assert!(!NoopTtsProvider.enabled());
        assert!(NoopTtsProvider
            .speak("hello")
            .unwrap_err()
            .to_string()
            .contains("disabled"));
    }

    #[cfg(not(windows))]
    #[test]
    fn windows_sapi_is_unsupported_off_windows() {
        assert!(!WindowsSapiTtsProvider.enabled());
        assert!(WindowsSapiTtsProvider.speak("hello").is_err());
    }
}
