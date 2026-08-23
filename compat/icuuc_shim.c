#include <stdint.h>

// Windows 10 1703+ ships system ICU as icuuc.dll. Windows Server 2016 / Windows 10 1607 do not.
// GPUI 0.2.2 imports only u_strlen from that DLL, so the legacy package supplies this tiny ABI
// compatible implementation instead of redistributing a full third-party ICU build.
__declspec(dllexport) int32_t __cdecl u_strlen(const uint16_t *text) {
    const uint16_t *cursor;

    if (text == 0) {
        return 0;
    }

    cursor = text;
    while (*cursor != 0) {
        ++cursor;
    }
    return (int32_t)(cursor - text);
}
