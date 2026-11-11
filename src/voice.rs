//! Native dictation, started only by an explicit user action.
/// Events may arrive on an audio or recognition thread. Forward them through `Link`.
#[derive(Clone,Debug,PartialEq)]
pub enum VoiceEvent {Ready,Level(f32),Transcribing,Text(String),Error(String)}

/// A microphone/speech session owned by the UI thread. Dropping it cancels capture and
/// discards pending recognition. No permissions are requested until `start` is called.
pub struct VoiceSession {
    #[cfg(target_os="macos")]
    native:crate::macos::voice::Session,
}
impl VoiceSession {
    pub fn start(locale:&str,callback:impl Fn(VoiceEvent)+Send+Sync+'static)->Result<Self,String> {
        #[cfg(target_os="macos")]
        {crate::macos::voice::Session::start(locale,callback).map(|native|Self{native})}
        #[cfg(not(target_os="macos"))]
        {let _=(locale,callback);Err("Voice input is not available on this platform".into())}
    }
    /// Stop the microphone and finish recognition. A terminal Text or Error follows.
    pub fn finish(&self) {#[cfg(target_os="macos")] self.native.finish();}
    /// Stop capture and discard both partial and final text. Safe to repeat.
    pub fn cancel(&self) {#[cfg(target_os="macos")] self.native.cancel();}
}
