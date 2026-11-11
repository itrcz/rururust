//! FFI boundary to the ARC-owned native speech session. All handle calls are UI-thread-only.
use crate::voice::VoiceEvent;
use std::{ffi::{c_char,c_void,CStr,CString},marker::PhantomData,ptr::NonNull,rc::Rc};
type Callback=Box<dyn Fn(VoiceEvent)+Send+Sync>;
extern "C" {
    fn ruru_voice_validate()->*const c_char;
    fn ruru_voice_start(locale:*const c_char,context:*mut c_void,callback:extern "C" fn(*mut c_void,i32,*const c_char,f32),destroy:extern "C" fn(*mut c_void))->*mut c_void;
    fn ruru_voice_finish(session:*mut c_void);
    fn ruru_voice_cancel(session:*mut c_void);
    fn ruru_voice_release(session:*mut c_void);
}
extern "C" fn event(context:*mut c_void,kind:i32,text:*const c_char,level:f32) {
    let _=std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: the native session owns this Box until its final deallocation. Every
        // callback retains that session, and UTF-8 text lives until this call returns.
        let callback=unsafe {&*(context.cast::<Callback>())};
        let value=||if text.is_null(){String::new()}else{unsafe{CStr::from_ptr(text)}.to_string_lossy().into_owned()};
        let e=match kind {0=>VoiceEvent::Ready,1=>VoiceEvent::Level(level.clamp(0.0,1.0)),2=>VoiceEvent::Transcribing,3=>VoiceEvent::Text(value()),_=>VoiceEvent::Error(value())};
        callback(e);
    }));
}
extern "C" fn destroy(context:*mut c_void) {
    // SAFETY: native dealloc calls this exactly once for its exclusively owned context.
    unsafe {drop(Box::from_raw(context.cast::<Callback>()));}
}
pub(crate) struct Session {pointer:NonNull<c_void>,_main:PhantomData<Rc<()>>}
impl Session {
    pub(crate) fn start(locale:&str,callback:impl Fn(VoiceEvent)+Send+Sync+'static)->Result<Self,String> {
        // SAFETY: validation touches only NSBundle/NSThread and returns a static error string.
        let error=unsafe {ruru_voice_validate()};
        if !error.is_null() {return Err(unsafe {CStr::from_ptr(error)}.to_string_lossy().into_owned());}
        let locale=CString::new(locale).map_err(|_|"Invalid speech language")?;
        let context=Box::into_raw(Box::new(Box::new(callback) as Callback)).cast::<c_void>();
        // SAFETY: validation ensures main thread and usage descriptions. Native copies
        // locale immediately and consumes context only if a non-null handle is returned.
        let pointer=unsafe {ruru_voice_start(locale.as_ptr(),context,event,destroy)};
        match NonNull::new(pointer) {Some(pointer)=>Ok(Self{pointer,_main:PhantomData}),None=>{destroy(context);Err("Cannot create a speech session".into())}}
    }
    pub(crate) fn finish(&self) {
        // SAFETY: Session is !Send/!Sync and owns a retained, live native object.
        unsafe {ruru_voice_finish(self.pointer.as_ptr());}
    }
    pub(crate) fn cancel(&self) {
        // SAFETY: same main-thread ownership; native cancellation is idempotent.
        unsafe {ruru_voice_cancel(self.pointer.as_ptr());}
    }
}
impl Drop for Session {fn drop(&mut self) {
    // SAFETY: transfer the one owned retain back to ARC; release cancels before destruction.
    unsafe {ruru_voice_release(self.pointer.as_ptr());}
}}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc,Mutex};
    #[test]
    fn voice_bridge_copies_text_bounds_levels_and_owns_callback_without_audio() {
        let values=Arc::new(Mutex::new(Vec::new()));let out=values.clone();
        let callback:Callback=Box::new(move |value|out.lock().unwrap().push(value));
        let context=Box::into_raw(Box::new(callback)).cast::<c_void>();
        let text=CString::new("Dictated text").unwrap();
        event(context,0,std::ptr::null(),0.0);event(context,1,std::ptr::null(),2.0);
        event(context,2,std::ptr::null(),0.0);event(context,3,text.as_ptr(),0.0);event(context,4,text.as_ptr(),0.0);
        destroy(context);drop(text);
        assert_eq!(*values.lock().unwrap(),vec![VoiceEvent::Ready,VoiceEvent::Level(1.0),VoiceEvent::Transcribing,
            VoiceEvent::Text("Dictated text".into()),VoiceEvent::Error("Dictated text".into())]);
    }
    #[test]
    fn voice_start_off_ui_thread_fails_before_permission_or_microphone_access() {
        let error=std::thread::spawn(||crate::VoiceSession::start("en-US",|_|panic!("must not start capture"))
            .err().expect("must reject worker thread")).join().unwrap();
        assert_eq!(error,"Voice input must start on the UI thread");
    }
}
