/* A stand-in for the user32.dll (and uxtheme.dll, kernel32.dll) functions classic plugins' dialogs call off Windows.
   GetSystemMetrics (Ed Harvey's controls size themselves with it) answers Windows 10/11's defaults at 96 DPI, 0 for
   anything else. The rest are calls on window handles (toe_head2001's OptionBasedLibrary tunes its controls with
   window messages): headless no control has a window, and Windows answers a call on a handle that is no window as
   these do (0, no focus, no icon, no key down). OutputDebugString hands a message to a debugger, and none listens.
   publish.sh builds it beside PdnBridge.dll; the plugin load context binds "user32.dll", "uxtheme.dll" and
   "kernel32.dll" to it. The browser host gives the same answers in managed code (paintnet-web/app/Rewrite.cs,
   PluginWindows): keep the two alike. */
#if defined(_WIN32)
#error "Windows has the real user32.dll"
#endif
#include <stdint.h>
#include <stddef.h>
#include <sys/mman.h>
#include <unistd.h>

intptr_t SendMessageW(void *hwnd, unsigned msg, uintptr_t wparam, intptr_t lparam) { return 0; }
intptr_t SendMessageA(void *hwnd, unsigned msg, uintptr_t wparam, intptr_t lparam) { return 0; }
intptr_t SendMessage(void *hwnd, unsigned msg, uintptr_t wparam, intptr_t lparam) { return 0; }
int LockWindowUpdate(void *hwnd) { return 1; }
void *GetFocus(void) { return 0; }
short GetKeyState(int key) { return 0; }
short VkKeyScanW(uint16_t ch) { return -1; }
unsigned GetGuiResources(void *process, unsigned flags) { return 0; }
int SetProcessDPIAware(void) { return 1; }
void *CreateIconFromResource(void *bits, unsigned size, int icon, unsigned version) { return 0; }
void *CreateIconIndirect(void *info) { return 0; }
int SetWindowTheme(void *hwnd, const uint16_t *app, const uint16_t *idlist) { return 0; }
void OutputDebugStringW(const uint16_t *message) { }
void OutputDebugStringA(const char *message) { }
/* kernel32's page allocator, which a plug-in may use for its own buffers (Midora's animated WebP FileType): zeroed
   pages, as Windows gives. Committing pages already reserved returns them. Windows releases a whole reservation
   with MEM_RELEASE (0x8000) and size 0, so each mapping keeps its length in a page in front of what it hands out. */
void *VirtualAlloc(void *address, size_t size, unsigned type, unsigned protect) {
    if (address) return address;
    size_t page = (size_t)sysconf(_SC_PAGESIZE), total = size + page;
    char *p = mmap(0, total, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (p == MAP_FAILED) return 0;
    *(size_t *)p = total;
    return p + page;
}
int VirtualFree(void *address, size_t size, unsigned type) {
    if (!(type & 0x8000) || !address) return 1;
    char *p = (char *)address - (size_t)sysconf(_SC_PAGESIZE);
    return munmap(p, *(size_t *)p) == 0;
}
int GetSystemMetrics(int index) {
    switch (index) {
    case 0: return 1920;              /* SM_CXSCREEN */
    case 1: return 1080;              /* SM_CYSCREEN */
    case 2: case 3: return 17;        /* SM_CXVSCROLL, SM_CYHSCROLL */
    case 4: return 23;                /* SM_CYCAPTION */
    case 5: case 6: return 1;         /* SM_CXBORDER, SM_CYBORDER */
    case 7: case 8: return 3;         /* SM_CXDLGFRAME, SM_CYDLGFRAME */
    case 9: case 10: return 17;       /* SM_CYVTHUMB, SM_CXHTHUMB */
    case 11: case 12: return 32;      /* SM_CXICON, SM_CYICON */
    case 13: case 14: return 32;      /* SM_CXCURSOR, SM_CYCURSOR */
    case 15: return 20;               /* SM_CYMENU */
    case 16: return 1920;             /* SM_CXFULLSCREEN */
    case 17: return 1017;             /* SM_CYFULLSCREEN */
    case 19: return 1;                /* SM_MOUSEPRESENT */
    case 20: case 21: return 17;      /* SM_CYVSCROLL, SM_CXHSCROLL */
    case 28: case 29: return 136;     /* SM_CXMIN, SM_CYMIN (≈) */
    case 30: case 31: return 22;      /* SM_CXSIZE, SM_CYSIZE */
    case 32: case 33: return 4;       /* SM_CXFRAME, SM_CYFRAME */
    case 36: case 37: return 4;       /* SM_CXDOUBLECLK, SM_CYDOUBLECLK */
    case 43: return 3;                /* SM_CMOUSEBUTTONS */
    case 45: case 46: return 2;       /* SM_CXEDGE, SM_CYEDGE */
    case 49: case 50: return 16;      /* SM_CXSMICON, SM_CYSMICON */
    case 51: return 17;               /* SM_CYSMCAPTION */
    case 68: case 69: return 4;       /* SM_CXDRAG, SM_CYDRAG */
    case 71: case 72: return 13;      /* SM_CXMENUCHECK, SM_CYMENUCHECK */
    case 75: return 1;                /* SM_MOUSEWHEELPRESENT */
    case 80: return 1;                /* SM_CMONITORS */
    case 83: case 84: return 1;       /* SM_CXFOCUSBORDER, SM_CYFOCUSBORDER */
    default: return 0;
    }
}
