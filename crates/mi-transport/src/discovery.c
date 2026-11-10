// R-D02: OS notifications only; no device open, report access, or vendor code.
#include <IOKit/hid/IOHIDManager.h>
#include <CoreGraphics/CoreGraphics.h>
#include <stdatomic.h>
#include <stdint.h>
typedef void (*change_callback)(void *);
typedef struct { change_callback notify; void *context; } watch_context;
static void changed(void *ctx, IOReturn result, void *sender, IOHIDDeviceRef device) {
    (void)result; (void)sender; (void)device;
    watch_context *watch = ctx;
    watch->notify(watch->context);
}
int mi_permission(void) { return CGPreflightListenEventAccess(); }
void mi_watch(const atomic_bool *stop, change_callback notify, void *context) {
    watch_context watch = {notify, context};
    IOHIDManagerRef manager = IOHIDManagerCreate(kCFAllocatorDefault, kIOHIDOptionsTypeNone);
    if (!manager) return;
    int vendor = 0x303A;
    CFNumberRef number = CFNumberCreate(NULL, kCFNumberIntType, &vendor);
    const void *keys[] = { CFSTR(kIOHIDVendorIDKey) };
    const void *values[] = { number };
    CFDictionaryRef filter = CFDictionaryCreate(NULL, keys, values, 1, &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks);
    IOHIDManagerSetDeviceMatching(manager, filter);
    CFRelease(filter); CFRelease(number);
    IOHIDManagerRegisterDeviceMatchingCallback(manager, changed, &watch);
    IOHIDManagerRegisterDeviceRemovalCallback(manager, changed, &watch);
    IOHIDManagerScheduleWithRunLoop(manager, CFRunLoopGetCurrent(), kCFRunLoopDefaultMode);
    while (!atomic_load(stop)) CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.1, true);
    IOHIDManagerUnscheduleFromRunLoop(manager, CFRunLoopGetCurrent(), kCFRunLoopDefaultMode);
    CFRelease(manager);
}

// Descriptor-only enumeration: never IOHIDManagerOpen/IOHIDDeviceOpen or report I/O.
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
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

static uint64_t keyboard_usb_parent(IOHIDDeviceRef device) {
    io_registry_entry_t current = IOHIDDeviceGetService(device);
    IOObjectRetain(current);
    for (unsigned i = 0; current && i < 24; ++i) {
        if (IOObjectConformsTo(current, "IOUSBHostDevice")) {
            uint64_t ident = 0;
            IORegistryEntryGetRegistryEntryID(current, &ident);
            IOObjectRelease(current);
            return ident;
        }
        io_registry_entry_t parent = 0;
        kern_return_t result = IORegistryEntryGetParentEntry(current, kIOServicePlane, &parent);
        IOObjectRelease(current);
        current = result == KERN_SUCCESS ? parent : 0;
    }
    if (current) IOObjectRelease(current);
    return 0;
}

int mi_keyboard_identity(void (*callback)(void *, uint32_t, uint64_t, uint64_t), void *context) {
    static const uint8_t descriptor[] = {
        0x05,0x01,0x09,0x06,0xa1,0x01,0x05,0x07,0x19,0xe0,0x29,0xe7,
        0x15,0x00,0x25,0x01,0x95,0x08,0x75,0x01,0x81,0x02,0x95,0x01,
        0x75,0x08,0x81,0x01,0x05,0x08,0x19,0x01,0x29,0x05,0x95,0x05,
        0x75,0x01,0x91,0x02,0x95,0x01,0x75,0x03,0x91,0x01,0x05,0x07,
        0x19,0x00,0x2a,0xff,0x00,0x15,0x00,0x26,0xff,0x00,0x95,0x06,
        0x75,0x08,0x81,0x00,0xc0
    };
    IOHIDManagerRef manager = IOHIDManagerCreate(NULL, kIOHIDOptionsTypeNone);
    if (!manager) return -1;
    int vid = 0x303a;
    CFNumberRef vendor_id = CFNumberCreate(NULL, kCFNumberIntType, &vid);
    const void *keys[] = {CFSTR(kIOHIDVendorIDKey)};
    const void *values[] = {vendor_id};
    CFDictionaryRef filter = CFDictionaryCreate(NULL, keys, values, 1, &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks);
    IOHIDManagerSetDeviceMatching(manager, filter);
    CFRelease(filter); CFRelease(vendor_id);
    CFSetRef devices = IOHIDManagerCopyDevices(manager);
    CFIndex count = devices ? CFSetGetCount(devices) : 0;
    const void **items = calloc((size_t)count + 1, sizeof(*items));
    if (!items) { if (devices) CFRelease(devices); CFRelease(manager); return -1; }
    if (devices) CFSetGetValues(devices, items);
    IOHIDDeviceRef keyboard = NULL, vendor = NULL;
    unsigned keyboards = 0, vendors = 0;
    for (CFIndex i = 0; i < count; ++i) {
        IOHIDDeviceRef d = (IOHIDDeviceRef)items[i];
        CFTypeRef transport = IOHIDDeviceGetProperty(d, CFSTR(kIOHIDTransportKey));
        if (number(IOHIDDeviceGetProperty(d, CFSTR(kIOHIDProductIDKey))) != 0x8298 || !transport || !CFEqual(transport, CFSTR("USB"))) continue;
        int page = number(IOHIDDeviceGetProperty(d, CFSTR(kIOHIDPrimaryUsagePageKey)));
        int usage = number(IOHIDDeviceGetProperty(d, CFSTR(kIOHIDPrimaryUsageKey)));
        if (page == 1 && usage == 6) { keyboard = d; ++keyboards; }
        if (page == 0xff00 && usage == 1) { vendor = d; ++vendors; }
    }
    int status = -1;
    if (keyboards == 1 && vendors == 1) {
        uint64_t parent = keyboard_usb_parent(keyboard), vendor_id_value = 0;
        uint32_t location = (uint32_t)number(IOHIDDeviceGetProperty(keyboard, CFSTR(kIOHIDLocationIDKey)));
        CFTypeRef actual = IOHIDDeviceGetProperty(keyboard, CFSTR(kIOHIDReportDescriptorKey));
        if (parent && parent == keyboard_usb_parent(vendor) && location && actual && CFGetTypeID(actual) == CFDataGetTypeID() && CFDataGetLength(actual) == sizeof(descriptor) && !memcmp(CFDataGetBytePtr(actual), descriptor, sizeof(descriptor)) && IORegistryEntryGetRegistryEntryID(IOHIDDeviceGetService(vendor), &vendor_id_value) == KERN_SUCCESS) {
            callback(context, location, parent, vendor_id_value);
            status = 0;
        }
    }
    free(items);
    if (devices) CFRelease(devices);
    CFRelease(manager);
    return status;
}
