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

// Descriptor-only enumeration: never IOHIDManagerOpen/IOHIDDeviceOpen or report I/O.
#include <stdlib.h>
#include <stdio.h>
#include <IOKit/IOKitLib.h>
static int number(CFTypeRef value) {
    int result = 0;
    if (value && CFGetTypeID(value) == CFNumberGetTypeID())
        CFNumberGetValue(value, kCFNumberIntType, &result);
    return result;
}
static void text(CFTypeRef value, char *buffer, size_t size, const char *fallback) {
    snprintf(buffer, size, "%s", fallback);
    if (value && CFGetTypeID(value) == CFStringGetTypeID())
        if (!CFStringGetCString(value, buffer, size, kCFStringEncodingUTF8))
            snprintf(buffer, size, "%s", fallback);
}
typedef void (*candidate_callback)(void *, uint16_t, uint16_t, uint16_t, uint16_t, const uint16_t *, size_t, const char *, const char *, const char *);
int mi_enumerate(candidate_callback callback, void *context) {
    IOHIDManagerRef manager = IOHIDManagerCreate(kCFAllocatorDefault, kIOHIDOptionsTypeNone);
    if (!manager) return -1;
    int vendor = 0x303A;
    CFNumberRef n = CFNumberCreate(NULL, kCFNumberIntType, &vendor);
    const void *keys[] = {CFSTR(kIOHIDVendorIDKey)};
    const void *values[] = {n};
    CFDictionaryRef filter = CFDictionaryCreate(NULL, keys, values, 1, &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks);
    IOHIDManagerSetDeviceMatching(manager, filter);
    CFRelease(filter); CFRelease(n);
    CFSetRef devices = IOHIDManagerCopyDevices(manager);
    if (!devices) { CFRelease(manager); return 0; }
    CFIndex count = CFSetGetCount(devices);
    IOHIDDeviceRef *entries = calloc((size_t)count + 1, sizeof(IOHIDDeviceRef));
    if (!entries) { CFRelease(devices); CFRelease(manager); return -1; }
    CFSetGetValues(devices, (const void **)entries);
    for (CFIndex i = 0; i < count; ++i) {
        IOHIDDeviceRef d = entries[i];
        CFTypeRef pairs = IOHIDDeviceGetProperty(d, CFSTR(kIOHIDDeviceUsagePairsKey));
        CFIndex pair_count = pairs && CFGetTypeID(pairs) == CFArrayGetTypeID() ? CFArrayGetCount(pairs) : 0;
        uint16_t *usages = calloc((size_t)pair_count * 2 + 1, sizeof(uint16_t));
        if (!usages) { free(entries); CFRelease(devices); CFRelease(manager); return -1; }
        size_t found = 0;
        for (CFIndex j = 0; j < pair_count; ++j) {
            CFTypeRef pair = CFArrayGetValueAtIndex(pairs, j);
            if (CFGetTypeID(pair) != CFDictionaryGetTypeID()) continue;
            usages[found * 2] = number(CFDictionaryGetValue(pair, CFSTR(kIOHIDDeviceUsagePageKey)));
            usages[found * 2 + 1] = number(CFDictionaryGetValue(pair, CFSTR(kIOHIDDeviceUsageKey)));
            ++found;
        }
        char product[1024], transport[128], path[32];
        text(IOHIDDeviceGetProperty(d, CFSTR(kIOHIDProductKey)), product, sizeof(product), "未命名 HID");
        text(IOHIDDeviceGetProperty(d, CFSTR(kIOHIDTransportKey)), transport, sizeof(transport), "Unknown");
        uint64_t registry_id = 0;
        if (IORegistryEntryGetRegistryEntryID(IOHIDDeviceGetService(d), &registry_id) == KERN_SUCCESS) {
            snprintf(path, sizeof(path), "DevSrvsID:%llu", (unsigned long long)registry_id);
            callback(context, number(IOHIDDeviceGetProperty(d, CFSTR(kIOHIDVendorIDKey))), number(IOHIDDeviceGetProperty(d, CFSTR(kIOHIDProductIDKey))), number(IOHIDDeviceGetProperty(d, CFSTR(kIOHIDPrimaryUsagePageKey))), number(IOHIDDeviceGetProperty(d, CFSTR(kIOHIDPrimaryUsageKey))), usages, found, transport, product, path);
        }
        free(usages);
    }
    free(entries); CFRelease(devices); CFRelease(manager);
    return 0;
}
