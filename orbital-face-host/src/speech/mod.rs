pub mod audio_capture;
pub mod stt;
pub mod tts;

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeechOptions {
    pub stt: String,
    pub whisper_model_path: Option<PathBuf>,
    pub whisper_bin: PathBuf,
    pub tts: String,
    pub piper_bin: Option<PathBuf>,
    pub piper_model: Option<PathBuf>,
    pub piper_config: Option<PathBuf>,
    pub speak_responses: bool,
}

impl Default for SpeechOptions {
    fn default() -> Self {
        Self {
            stt: "mock".into(),
            whisper_model_path: None,
            whisper_bin: PathBuf::from("whisper-cli"),
            tts: "none".into(),
            piper_bin: None,
            piper_model: None,
            piper_config: None,
            speak_responses: false,
        }
    }
}

#[derive(Debug, Default)]
pub struct SpeechRuntimeStatus {
    pub last_transcription: Option<String>,
    pub last_error: Option<String>,
}

pub fn spoken_response(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::spoken_response;

    #[test]
    fn spoken_response_is_bounded_by_characters() {
        assert_eq!(spoken_response("abcdef", 4), "abcd");
        assert_eq!(spoken_response("héllo", 3), "hél");
    }
}
