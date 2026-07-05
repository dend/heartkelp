use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum CaptureMode {
    FullScreen,
    Region { x: i32, y: i32, w: u32, h: u32 },
}

pub enum Command {
    TakeScreenshot,
    StartRecording {
        mode: CaptureMode,
        fps: u8,
    },
    StopRecording,
    PauseRecording,
    ResumeRecording,
    EncodeFrames {
        frames: Vec<Frame>,
        fps: u8,
        start: usize,
        end: usize,
        width: Option<u32>,
        height: Option<u32>,
        output_path: PathBuf,
    },
}

pub enum Event {
    ScreenshotReady(egui::ColorImage),
    RecordingStarted,
    FrameCaptured(usize),
    RecordingReady { frames: Vec<Frame>, fps: u8 },
    EncodingProgress(usize),
    RecordingFinished(PathBuf),
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum RecordingState {
    Idle,
    SelectingRegion,
    Recording,
    Reviewing,
    Encoding,
}

#[derive(Clone)]
pub struct Frame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Seconds since capture start. Capture is damage-driven and lossy, so
    /// frames are not uniformly spaced — the encoder must use these
    /// timestamps, not an assumed fixed frame rate.
    pub pts: f64,
}
