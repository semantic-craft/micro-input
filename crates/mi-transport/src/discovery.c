// R-D02: OS notifications only; no device open, report access, or vendor code.
#include <IOKit/hid/IOHIDManager.h>
#include <CoreGraphics/CoreGraphics.h>
#include <stdatomic.h>
#include <stdint.h>
static atomic_uint_fast64_t changes = 0;
static void changed(void *ctx, IOReturn result, void *sender, IOHIDDeviceRef device) {
    (void)ctx; (void)result; (void)sender; (void)device;
    atomic_fetch_add(&changes, 1);
}
uint64_t mi_changes(void) { return atomic_load(&changes); }
int mi_permission(void) { return CGPreflightListenEventAccess(); }
void mi_watch(const atomic_bool *stop) {
    IOHIDManagerRef manager = IOHIDManagerCreate(kCFAllocatorDefault, kIOHIDOptionsTypeNone);
    if (!manager) return;
    int vendor = 0x303A;
    CFNumberRef number = CFNumberCreate(NULL, kCFNumberIntType, &vendor);
    const void *keys[] = { CFSTR(kIOHIDVendorIDKey) };
    const void *values[] = { number };
    CFDictionaryRef filter = CFDictionaryCreate(NULL, keys, values, 1, &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks);
    IOHIDManagerSetDeviceMatching(manager, filter);
    CFRelease(filter); CFRelease(number);
    IOHIDManagerRegisterDeviceMatchingCallback(manager, changed, NULL);
    IOHIDManagerRegisterDeviceRemovalCallback(manager, changed, NULL);
    IOHIDManagerScheduleWithRunLoop(manager, CFRunLoopGetCurrent(), kCFRunLoopDefaultMode);
    while (!atomic_load(stop)) CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.1, true);
    IOHIDManagerUnscheduleFromRunLoop(manager, CFRunLoopGetCurrent(), kCFRunLoopDefaultMode);
    CFRelease(manager);
}
