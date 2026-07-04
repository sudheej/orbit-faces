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
            piper_bin: Some(PathBuf::from("./piper/piper")),
            piper_model: Some(PathBuf::from("./voices/en_US-amy-medium.onnx")),
            piper_config: Some(PathBuf::from("./voices/en_US-amy-medium.onnx.json")),
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
    let without_emoji: String = text
        .chars()
        .filter(|character| {
            !is_emoji_character(*character) && !matches!(character, '*' | '_' | '`' | '~')
        })
        .collect();
    without_emoji
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect()
}

fn is_emoji_character(character: char) -> bool {
    matches!(
        character as u32,
        0x00A9 | 0x00AE | 0x200D | 0x203C | 0x2049 | 0x20E3 | 0x2122 | 0x2139
            | 0x2194..=0x21FF | 0x2300..=0x23FF | 0x2460..=0x24FF
            | 0x25A0..=0x27BF | 0x2934..=0x2935 | 0x2B00..=0x2BFF
            | 0x3030 | 0x303D | 0x3297 | 0x3299 | 0xFE00..=0xFE0F
            | 0x1F000..=0x1FAFF | 0xE0020..=0xE007F
    )
}

#[cfg(test)]
mod tests {
    use super::{spoken_response, SpeechOptions};

    #[test]
    fn amy_is_the_default_piper_voice() {
        let options = SpeechOptions::default();
        assert_eq!(
            options.piper_model.as_deref(),
            Some(std::path::Path::new("./voices/en_US-amy-medium.onnx"))
        );
        assert_eq!(
            options.piper_config.as_deref(),
            Some(std::path::Path::new("./voices/en_US-amy-medium.onnx.json"))
        );
    }

    #[test]
    fn spoken_response_is_bounded_by_characters() {
        assert_eq!(spoken_response("abcdef", 4), "abcd");
        assert_eq!(spoken_response("héllo", 3), "hél");
    }

    #[test]
    fn spoken_response_removes_emoji_and_repairs_spacing() {
        assert_eq!(spoken_response("Hello 👋 world! 🚀", 100), "Hello world!");
        assert_eq!(spoken_response("Good ✅\njob", 100), "Good job");
    }

    #[test]
    fn spoken_response_removes_markdown_emphasis_markers() {
        assert_eq!(
            spoken_response("**Whole foods:** eat `lean protein` and _vegetables_.", 100),
            "Whole foods: eat lean protein and vegetables."
        );
    }
}
