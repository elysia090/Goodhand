// AppKit shell for a single native Goodhand window. Artwork and its clock
// stay in the caller; Cocoa receives a borrowed opaque sRGB frame.
#import <AppKit/AppKit.h>
#import <CoreGraphics/CoreGraphics.h>

#include <limits.h>
#include <stdint.h>

typedef int32_t (*GoodhandUpdate)(void *context, const uint32_t **pixels);

@class GoodhandController;

@interface GoodhandView : NSView
@property(nonatomic, weak) GoodhandController *controller;
@property(nonatomic, strong) NSColor *backdrop;
- (BOOL)replaceFrame:(const uint32_t *)pixels
               width:(uint32_t)width
              height:(uint32_t)height;
@end

@interface GoodhandController : NSObject <NSWindowDelegate, NSApplicationDelegate>
@property(nonatomic) void *context;
@property(nonatomic) GoodhandUpdate update;
@property(nonatomic) uint32_t width;
@property(nonatomic) uint32_t height;
@property(nonatomic) BOOL done;
@property(nonatomic) BOOL hostError;
@property(nonatomic, strong) NSTimer *timer;
@property(nonatomic, strong) GoodhandView *view;
- (void)poll:(NSTimer *)timer;
- (void)requestStop:(id)sender;
@end

@implementation GoodhandView {
    CGImageRef _image;
}

- (void)dealloc {
    if (_image != NULL) {
        CGImageRelease(_image);
    }
}

- (BOOL)isOpaque { return YES; }
- (BOOL)acceptsFirstResponder { return YES; }

- (BOOL)replaceFrame:(const uint32_t *)pixels
               width:(uint32_t)width
              height:(uint32_t)height {
    const size_t stride = (size_t)width * sizeof(*pixels);
    // Core Graphics can keep this data until a deferred expose or resize;
    // an immutable copy is necessary before the next Rust callback.
    CFDataRef data = CFDataCreate(kCFAllocatorDefault, (const UInt8 *)pixels,
                                  (CFIndex)(stride * height));
    if (data == NULL) {
        return NO;
    }
    CGDataProviderRef provider = CGDataProviderCreateWithCFData(data);
    CFRelease(data);
    if (provider == NULL) {
        return NO;
    }
    CGColorSpaceRef color = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
    CGImageRef image = NULL;
    if (color != NULL) {
        // Little endian 0xFFRRGGBB is B,G,R,unused in memory.
        CGBitmapInfo format = kCGBitmapByteOrder32Little | (CGBitmapInfo)kCGImageAlphaNoneSkipFirst;
        image = CGImageCreate(width, height, 8, 32, stride, color,
                              format, provider, NULL, false,
                              kCGRenderingIntentDefault);
        CGColorSpaceRelease(color);
    }
    CGDataProviderRelease(provider);
    if (image == NULL) {
        return NO;
    }
    if (_image != NULL) {
        CGImageRelease(_image);
    }
    _image = image;
    [self setNeedsDisplay:YES];
    return YES;
}

- (void)drawRect:(NSRect)dirty {
    const NSRect bounds = self.bounds;
    [self.backdrop setFill];
    NSRectFill(dirty);
    if (_image == NULL || NSWidth(bounds) <= 0 || NSHeight(bounds) <= 0) {
        return;
    }

    // The default AppKit view is y-up, as is CGContextDrawImage; the
    // top row of the supplied image appears at the top of the aperture.
    const CGFloat width = (CGFloat)CGImageGetWidth(_image);
    const CGFloat height = (CGFloat)CGImageGetHeight(_image);
    const CGFloat sx = NSWidth(bounds) / width;
    const CGFloat sy = NSHeight(bounds) / height;
    const CGFloat scale = sx < sy ? sx : sy;
    const CGFloat fitWidth = width * scale;
    const CGFloat fitHeight = height * scale;
    const NSRect fit = NSMakeRect(NSMidX(bounds) - fitWidth / 2,
                                  NSMidY(bounds) - fitHeight / 2,
                                  fitWidth, fitHeight);
    const NSRect aligned = [self backingAlignedRect:fit
                                           options:NSAlignAllEdgesNearest];
    CGContextRef context = [NSGraphicsContext currentContext].CGContext;
    CGContextSaveGState(context);
    CGContextSetInterpolationQuality(context, kCGInterpolationNone);
    CGContextSetShouldAntialias(context, false);
    CGContextDrawImage(context, NSRectToCGRect(aligned), _image);
    CGContextRestoreGState(context);
}

- (void)keyDown:(NSEvent *)event {
    const NSEventModifierFlags reserved =
        NSEventModifierFlagCommand | NSEventModifierFlagControl | NSEventModifierFlagOption;
    NSString *key = event.charactersIgnoringModifiers.lowercaseString;
    if ((event.modifierFlags & reserved) == 0 &&
        ([key isEqualToString:@"q"] || [key isEqualToString:@"\x1b"])) {
        [self.controller requestStop:event];
        return;
    }
    [super keyDown:event];
}
@end

@implementation GoodhandController
- (void)poll:(NSTimer *)timer {
    (void)timer;
    if (self.done) {
        return;
    }
    const uint32_t *pixels = NULL;
    const int32_t decision = self.update(self.context, &pixels);
    switch (decision) {
        case -1:
            [self requestStop:nil];
            break;
        case 0:
            break;
        case 1:
            if (pixels == NULL ||
                ![self.view replaceFrame:pixels width:self.width height:self.height]) {
                self.hostError = YES;
                [self requestStop:nil];
            }
            break;
        default:
            self.hostError = YES;
            [self requestStop:nil];
            break;
    }
}

- (void)requestStop:(id)sender {
    (void)sender;
    if (self.done) {
        return;
    }
    self.done = YES;
    [self.timer invalidate];
    self.timer = nil;
    if (NSApp.running) {
        [NSApp stop:nil];
        // stop: from a timer needs an event for AppKit's run method to
        // notice the stop flag even when no person is interacting.
        NSEvent *wake = [NSEvent otherEventWithType:NSEventTypeApplicationDefined
                                          location:NSZeroPoint
                                     modifierFlags:0
                                         timestamp:0
                                      windowNumber:0
                                           context:nil
                                           subtype:0
                                             data1:0
                                             data2:0];
        [NSApp postEvent:wake atStart:YES];
    }
}

- (void)windowWillClose:(NSNotification *)notification {
    [self requestStop:notification];
}

- (NSApplicationTerminateReply)applicationShouldTerminate:(NSApplication *)sender {
    // Leave the event loop so Rust can release its state and report errors.
    [self requestStop:sender];
    return NSTerminateCancel;
}
@end

static NSMenu *goodhand_menu(void) {
    NSMenu *menu = [[NSMenu alloc] initWithTitle:@""];
    NSMenuItem *appItem = [[NSMenuItem alloc] initWithTitle:@"" action:NULL keyEquivalent:@""];
    NSMenu *appMenu = [[NSMenu alloc] initWithTitle:@"oops"];
    [appMenu addItemWithTitle:@"Quit oops" action:@selector(terminate:) keyEquivalent:@"q"];
    appItem.submenu = appMenu;
    [menu addItem:appItem];
    return menu;
}

// main-thread only. Returns zero for a normal stop, nonzero if AppKit or
// the framebuffer could not be initialized. Graphics consumes no Rust
// state after this returns.
int32_t goodhand_run_platform(void *context, uint32_t width, uint32_t height,
                             uint32_t client_width, uint32_t client_height,
                             uint32_t min_width, uint32_t min_height,
                             uint32_t background, GoodhandUpdate update) {
    if (![NSThread isMainThread] || update == NULL || width == 0 || height == 0 ||
        min_width == 0 || min_height == 0 || min_width > client_width || min_height > client_height ||
        (size_t)width > (size_t)LONG_MAX / sizeof(uint32_t) / height) {
        return 1;
    }
    @autoreleasepool {
        NSApplication *app = [NSApplication sharedApplication];
        if (app == nil || ![app setActivationPolicy:NSApplicationActivationPolicyRegular]) {
            return 1;
        }
        app.mainMenu = goodhand_menu();

        const NSRect rect = NSMakeRect(0, 0, client_width, client_height);
        const NSWindowStyleMask style = NSWindowStyleMaskTitled | NSWindowStyleMaskClosable |
                                        NSWindowStyleMaskMiniaturizable | NSWindowStyleMaskResizable;
        NSWindow *window = [[NSWindow alloc] initWithContentRect:rect
                                                       styleMask:style
                                                         backing:NSBackingStoreBuffered
                                                           defer:NO];
        GoodhandView *view = [[GoodhandView alloc] initWithFrame:rect];
        GoodhandController *controller = [[GoodhandController alloc] init];
        if (window == nil || view == nil || controller == nil) {
            return 1;
        }
        controller.context = context;
        controller.update = update;
        controller.width = width;
        controller.height = height;
        controller.view = view;
        view.controller = controller;
        view.backdrop = [NSColor colorWithSRGBRed:((background >> 16) & 0xff) / 255.0
                                           green:((background >> 8) & 0xff) / 255.0
                                            blue:(background & 0xff) / 255.0
                                           alpha:1.0];
        view.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
        window.releasedWhenClosed = NO;
        window.delegate = controller;
        app.delegate = controller;
        window.title = @"oops";
        window.appearance = [NSAppearance appearanceNamed:NSAppearanceNameDarkAqua];
        window.backgroundColor = view.backdrop;
        window.contentView = view;
        window.contentMinSize = NSMakeSize(min_width, min_height);
        [window setContentSize:rect.size];
        [window center];

        [app finishLaunching];
        [window makeFirstResponder:view];
        [window makeKeyAndOrderFront:nil];
        // Resolve the Sonoma activation method through AppKit. The host
        // is linked by rustc, so do not introduce Clang's separate
        // deployment-availability runtime for this optional selector.
        if ([app respondsToSelector:@selector(activate)]) {
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wunguarded-availability-new"
            [app activate];
#pragma clang diagnostic pop
        } else {
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
            [app activateIgnoringOtherApps:YES];
#pragma clang diagnostic pop
        }

        // An absolute phase clock lives in the caller. Polling at 8 ms
        // doesn't set the artwork frame rate or keep a backlog.
        [controller poll:nil];
        if (!controller.done) {
            controller.timer = [NSTimer timerWithTimeInterval:0.008
                                                        target:controller
                                                      selector:@selector(poll:)
                                                      userInfo:nil
                                                       repeats:YES];
            [[NSRunLoop mainRunLoop] addTimer:controller.timer forMode:NSRunLoopCommonModes];
            [app run];
        }

        [controller.timer invalidate];
        controller.timer = nil;
        window.delegate = nil;
        app.delegate = nil;
        [window orderOut:nil];
        return controller.hostError ? 1 : 0;
    }
}
