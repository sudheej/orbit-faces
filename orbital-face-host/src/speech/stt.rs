use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq)]
pub struct Transcription {
    pub text: String,
    pub provider: String,
    pub elapsed_ms: u64,
    pub confidence: Option<f32>,
}

pub trait SpeechToTextProvider {
    fn name(&self) -> &'static str;
    fn transcribe_wav(&self, path: &Path) -> anyhow::Result<Transcription>;
    fn requires_audio_capture(&self) -> bool {
        true
    }
    fn model_path(&self) -> Option<&Path> {
        None
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MockSttProvider;

impl SpeechToTextProvider for MockSttProvider {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn transcribe_wav(&self, _path: &Path) -> anyhow::Result<Transcription> {
        Ok(Transcription {
            text: "mock voice prompt".into(),
            provider: self.name().into(),
            elapsed_ms: 0,
            confidence: Some(1.0),
        })
    }

    fn requires_audio_capture(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone)]
pub struct WhisperSttProvider {
    binary: PathBuf,
    model: PathBuf,
}

impl WhisperSttProvider {
    pub fn new(binary: PathBuf, model: Option<PathBuf>) -> anyhow::Result<Self> {
        let model = model.ok_or_else(|| {
            anyhow::anyhow!(
                "Whisper model path is required. Download ggml-tiny.en.bin and pass --whisper-model-path."
            )
        })?;
        anyhow::ensure!(
            model.is_file(),
            "Whisper model does not exist: {}",
            model.display()
        );
        Ok(Self { binary, model })
    }
}

impl SpeechToTextProvider for WhisperSttProvider {
    fn name(&self) -> &'static str {
        "whisper"
    }

    fn transcribe_wav(&self, path: &Path) -> anyhow::Result<Transcription> {
        anyhow::ensure!(
            path.is_file(),
            "audio file does not exist: {}",
            path.display()
        );
        let started = Instant::now();
        let output = Command::new(&self.binary)
            .arg("-m")
            .arg(&self.model)
            .arg("-f")
            .arg(path)
            .args(["-np", "-nt", "-l", "en"])
            .output()
            .map_err(|error| {
                anyhow::anyhow!(
                    "failed to start whisper.cpp CLI {}: {error}",
                    self.binary.display()
                )
            })?;
        anyhow::ensure!(
            output.status.success(),
            "whisper.cpp failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        anyhow::ensure!(
            !text.is_empty(),
            "whisper.cpp returned an empty transcription"
        );
        Ok(Transcription {
            text,
            provider: self.name().into(),
            elapsed_ms: started.elapsed().as_millis() as u64,
            confidence: None,
        })
    }

    fn model_path(&self) -> Option<&Path> {
        Some(&self.model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_stt_is_deterministic_and_skips_audio() {
        let transcription = MockSttProvider
            .transcribe_wav(Path::new("unused.wav"))
            .unwrap();
        assert_eq!(transcription.text, "mock voice prompt");
        assert!(!MockSttProvider.requires_audio_capture());
    }

    #[test]
    fn whisper_requires_model_path() {
        let error = WhisperSttProvider::new(PathBuf::from("whisper-cli"), None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("Whisper model path is required"));
    }
}
