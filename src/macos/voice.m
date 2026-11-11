// Native Speech/AVAudioEngine ownership. No audio file is written to disk.
#import <Foundation/Foundation.h>
#import <AVFoundation/AVFoundation.h>
#import <Speech/Speech.h>
#import <stdatomic.h>
#import <math.h>

typedef void (*RuRuVoiceCallback)(void *, int, const char *, float);
typedef void (*RuRuVoiceDestroy)(void *);
@interface RuRuVoiceSession : NSObject {
@public
    void *context;
    RuRuVoiceCallback callback;
    RuRuVoiceDestroy destroy;
    atomic_bool cancelled;
    BOOL terminal;
    BOOL finishing;
    BOOL tapped;
    AVAudioEngine *engine;
    SFSpeechRecognizer *recognizer;
    SFSpeechAudioBufferRecognitionRequest *request;
    SFSpeechRecognitionTask *task;
    NSString *latest;
    NSString *locale;
}
- (void)authorize;
- (void)finish;
- (void)cancel;
@end
@implementation RuRuVoiceSession
- (void)dealloc {if (destroy) destroy(context);}
- (void)emit:(int)kind text:(NSString *)text level:(float)level {
    if (!atomic_load(&cancelled) && callback) callback(context,kind,text ? text.UTF8String : "",level);
}
- (void)stopCapture {
    [engine stop];
    if (tapped) {[engine.inputNode removeTapOnBus:0];tapped=NO;}
}
- (void)complete:(NSString *)text error:(NSString *)error {
    if (terminal || atomic_load(&cancelled)) return;
    terminal=YES;[self stopCapture];[request endAudio];
    if (text.length>0) [self emit:3 text:text level:0];
    else [self emit:4 text:error ?: @"No speech was recognized. Try again." level:0];
    atomic_store(&cancelled,true);
    [task cancel];task=nil;request=nil;recognizer=nil;engine=nil;
}
- (void)authorize {
    if (atomic_load(&cancelled)) return;
    [AVCaptureDevice requestAccessForMediaType:AVMediaTypeAudio completionHandler:^(BOOL granted) {
        dispatch_async(dispatch_get_main_queue(), ^{
            if (atomic_load(&self->cancelled)) return;
            if (!granted) {[self complete:nil error:@"Microphone access was denied. Enable it in System Settings → Privacy & Security → Microphone."];return;}
            [SFSpeechRecognizer requestAuthorization:^(SFSpeechRecognizerAuthorizationStatus status) {
                dispatch_async(dispatch_get_main_queue(), ^{
                    if (atomic_load(&self->cancelled)) return;
                    if (status!=SFSpeechRecognizerAuthorizationStatusAuthorized) {
                        [self complete:nil error:@"Speech recognition access was denied or restricted. Check System Settings → Privacy & Security → Speech Recognition."];return;
                    }
                    [self startCapture];
                });
            }];
        });
    }];
}
- (void)startCapture {
    if (atomic_load(&cancelled) || terminal) return;
    @try {
    recognizer=[[SFSpeechRecognizer alloc] initWithLocale:[[NSLocale alloc] initWithLocaleIdentifier:locale]];
    if (!recognizer || !recognizer.available) {[self complete:nil error:@"Speech recognition is currently unavailable for this language."];return;}
    request=[SFSpeechAudioBufferRecognitionRequest new];request.shouldReportPartialResults=YES;
    if (recognizer.supportsOnDeviceRecognition) request.requiresOnDeviceRecognition=YES;
    engine=[AVAudioEngine new];AVAudioInputNode *input=engine.inputNode;
    AVAudioFormat *format=[input outputFormatForBus:0];
    if (!input || format.sampleRate<=0 || format.channelCount==0) {[self complete:nil error:@"No microphone input is available."];return;}
    __weak RuRuVoiceSession *weak=self;
    task=[recognizer recognitionTaskWithRequest:request resultHandler:^(SFSpeechRecognitionResult *result,NSError *error) {
        dispatch_async(dispatch_get_main_queue(), ^{
            RuRuVoiceSession *session=weak;if (!session || atomic_load(&session->cancelled) || session->terminal) return;
            if (result) session->latest=result.bestTranscription.formattedString;
            if (result.final) {[session complete:session->latest error:nil];}
            else if (error) {[session complete:session->finishing ? session->latest : nil error:error.localizedDescription];}
        });
    }];
    __block CFAbsoluteTime lastLevel=0;
    SFSpeechAudioBufferRecognitionRequest *audioRequest=request;
    [input installTapOnBus:0 bufferSize:1024 format:format block:^(AVAudioPCMBuffer *buffer,AVAudioTime *when) {
        (void)when;
        RuRuVoiceSession *session=weak;if (!session || atomic_load(&session->cancelled)) return;
        [audioRequest appendAudioPCMBuffer:buffer];
        CFAbsoluteTime now=CFAbsoluteTimeGetCurrent();
        if (now-lastLevel<0.05) return;lastLevel=now;
        float level=0;float * const *channels=buffer.floatChannelData;AVAudioFrameCount count=buffer.frameLength;
        if (channels && count>0) {
            double energy=0;for (AVAudioFrameCount i=0;i<count;i++) {double value=channels[0][i];energy+=value*value;}
            double rms=sqrt(energy/count);level=(float)fmin(1.0,fmax(0.0,(20.0*log10(fmax(rms,0.000001))+60.0)/60.0));
        }
        [session emit:1 text:nil level:level];
    }];tapped=YES;
    [engine prepare];NSError *error=nil;
    if (![engine startAndReturnError:&error]) {[self complete:nil error:error.localizedDescription ?: @"Could not start the microphone."];return;}
    [self emit:0 text:nil level:0];
    } @catch (NSException *exception) {
        [self complete:nil error:exception.reason ?: @"Could not start voice input."];
    }
}
- (void)finish {
    if (atomic_load(&cancelled) || terminal || finishing) return;
    if (!request) {[self complete:nil error:@"Recording has not started yet."];return;}
    finishing=YES;[self stopCapture];[self emit:2 text:nil level:0];[request endAudio];
    __weak RuRuVoiceSession *weak=self;
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW,15*NSEC_PER_SEC),dispatch_get_main_queue(),^{
        RuRuVoiceSession *session=weak;if (!session || atomic_load(&session->cancelled) || session->terminal) return;
        [session complete:session->latest error:@"Speech recognition timed out. Try again."];
    });
}
- (void)cancel {
    if (atomic_exchange(&cancelled,true)) return;
    [self stopCapture];[request endAudio];[task cancel];task=nil;request=nil;recognizer=nil;engine=nil;
}
@end

const char *ruru_voice_validate(void) {
    if (![NSThread isMainThread]) return "Voice input must start on the UI thread";
    if (![[NSBundle mainBundle] objectForInfoDictionaryKey:@"NSMicrophoneUsageDescription"]) return "Microphone usage description is missing from the application";
    if (![[NSBundle mainBundle] objectForInfoDictionaryKey:@"NSSpeechRecognitionUsageDescription"]) return "Speech usage description is missing from the application";
    return NULL;
}
void *ruru_voice_start(const char *language,void *context,RuRuVoiceCallback callback,RuRuVoiceDestroy destroy) {
    RuRuVoiceSession *session=[RuRuVoiceSession new];if (!session) return NULL;
    session->context=context;session->callback=callback;session->destroy=destroy;atomic_init(&session->cancelled,false);
    session->locale=[NSString stringWithUTF8String:language];
    dispatch_async(dispatch_get_main_queue(),^{[session authorize];});
    return (__bridge_retained void *)session;
}
void ruru_voice_finish(void *session) {[(__bridge RuRuVoiceSession *)session finish];}
void ruru_voice_cancel(void *session) {[(__bridge RuRuVoiceSession *)session cancel];}
void ruru_voice_release(void *session) {RuRuVoiceSession *owned=CFBridgingRelease(session);[owned cancel];}
