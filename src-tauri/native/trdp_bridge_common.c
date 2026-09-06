#include "trdp_bridge.h"

#include <ctype.h>
#include <errno.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#ifdef _WIN32
#include <process.h>
#else
#include <unistd.h>
#endif

static bridge_mutex_t g_output_mutex;
static int g_common_ready;

#ifdef _WIN32
#define BRIDGE_THREAD_LOCAL __declspec(thread)
#else
#define BRIDGE_THREAD_LOCAL _Thread_local
#endif

/*
 * Request correlation is thread-local by design. stdin commands run on the
 * bridge control thread while queued TCNOpen commands run on the Node runtime
 * thread. Asynchronous protocol callbacks therefore never inherit an unrelated
 * request id from another thread.
 */
static BRIDGE_THREAD_LOCAL char g_request_id[64];

void bridge_mutex_init(bridge_mutex_t *mutex) {
#ifdef _WIN32
    InitializeCriticalSection(mutex);
#else
    (void)pthread_mutex_init(mutex, NULL);
#endif
}

void bridge_mutex_destroy(bridge_mutex_t *mutex) {
#ifdef _WIN32
    DeleteCriticalSection(mutex);
#else
    (void)pthread_mutex_destroy(mutex);
#endif
}

void bridge_mutex_lock(bridge_mutex_t *mutex) {
#ifdef _WIN32
    EnterCriticalSection(mutex);
#else
    (void)pthread_mutex_lock(mutex);
#endif
}

void bridge_mutex_unlock(bridge_mutex_t *mutex) {
#ifdef _WIN32
    LeaveCriticalSection(mutex);
#else
    (void)pthread_mutex_unlock(mutex);
#endif
}

void bridge_thread_join(bridge_thread_t thread) {
#ifdef _WIN32
    (void)WaitForSingleObject(thread, INFINITE);
    (void)CloseHandle(thread);
#else
    (void)pthread_join(thread, NULL);
#endif
}

void bridge_sleep_ms(unsigned int milliseconds) {
#ifdef _WIN32
    Sleep((DWORD)milliseconds);
#else
    struct timespec delay;
    delay.tv_sec = (time_t)(milliseconds / 1000u);
    delay.tv_nsec = (long)(milliseconds % 1000u) * 1000000L;
    (void)nanosleep(&delay, NULL);
#endif
}

uint64_t bridge_now_us(void) {
#ifdef _WIN32
    FILETIME file_time;
    ULARGE_INTEGER value;
    GetSystemTimeAsFileTime(&file_time);
    value.LowPart = file_time.dwLowDateTime;
    value.HighPart = file_time.dwHighDateTime;
    if (value.QuadPart < 116444736000000000ULL) {
        return 0;
    }
    return (uint64_t)((value.QuadPart - 116444736000000000ULL) / 10ULL);
#else
    struct timeval now;
    if (gettimeofday(&now, NULL) != 0) {
        return 0;
    }
    return (uint64_t)now.tv_sec * 1000000ULL + (uint64_t)now.tv_usec;
#endif
}

void bridge_common_init(void) {
    if (!g_common_ready) {
        bridge_mutex_init(&g_output_mutex);
        g_common_ready = 1;
    }
}

void bridge_common_shutdown(void) {
    if (g_common_ready) {
        bridge_mutex_destroy(&g_output_mutex);
        g_common_ready = 0;
    }
}

void bridge_output_lock(void) {
    bridge_mutex_lock(&g_output_mutex);
}

void bridge_output_unlock(void) {
    bridge_mutex_unlock(&g_output_mutex);
}

void bridge_request_begin(const char *line) {
    g_request_id[0] = '\0';
    if (line != NULL) {
        (void)bridge_json_string(line, "request_id", g_request_id, sizeof(g_request_id), "");
    }
}

void bridge_request_end(void) {
    g_request_id[0] = '\0';
}

static void bridge_emit_request_id(FILE *file) {
    if (g_request_id[0] != '\0') {
        fputs(",\"request_id\":\"", file);
        bridge_json_escape(file, g_request_id);
        fputc('"', file);
    }
}

void bridge_json_escape(FILE *file, const char *text) {
    const unsigned char *cursor = (const unsigned char *)(text != NULL ? text : "");
    while (*cursor != 0u) {
        switch (*cursor) {
            case '\\': fputs("\\\\", file); break;
            case '"': fputs("\\\"", file); break;
            case '\n': fputs("\\n", file); break;
            case '\r': fputs("\\r", file); break;
            case '\t': fputs("\\t", file); break;
            default:
                if (*cursor < 0x20u) {
                    fprintf(file, "\\u%04x", (unsigned int)*cursor);
                } else {
                    fputc((int)*cursor, file);
                }
                break;
        }
        ++cursor;
    }
}

void bridge_print_hex(FILE *file, const UINT8 *data, UINT32 size) {
    static const char digits[] = "0123456789ABCDEF";
    UINT32 index;
    for (index = 0u; data != NULL && index < size; ++index) {
        fputc(digits[(data[index] >> 4) & 0x0fu], file);
        fputc(digits[data[index] & 0x0fu], file);
    }
}

void bridge_print_ip(FILE *file, UINT32 ip) {
    const CHAR8 *text = vos_ipDotted(ip);
    fputs(text != NULL ? text : "0.0.0.0", file);
}

void bridge_emit_ack(const char *command, const char *id) {
    bridge_output_lock();
    fputs("{\"event\":\"ack\",\"command\":\"", stdout);
    bridge_json_escape(stdout, command);
    fputs("\"", stdout);
    bridge_emit_request_id(stdout);
    if (id != NULL && *id != '\0') {
        fputs(",\"id\":\"", stdout);
        bridge_json_escape(stdout, id);
        fputc('"', stdout);
    }
    fputs("}\n", stdout);
    fflush(stdout);
    bridge_output_unlock();
}

void bridge_emit_error(const char *message) {
    bridge_output_lock();
    fputs("{\"event\":\"error\",\"error\":\"", stdout);
    bridge_json_escape(stdout, message != NULL ? message : "unknown error");
    fputc('"', stdout);
    bridge_emit_request_id(stdout);
    fputs("}\n", stdout);
    fflush(stdout);
    bridge_output_unlock();
}

void bridge_emit_trdp_error(const char *operation, TRDP_ERR_T error) {
    char message[256];
    (void)snprintf(
        message,
        sizeof(message),
        "%s failed with TCNOpen error %d",
        operation != NULL ? operation : "TRDP operation",
        (int)error
    );
    bridge_emit_error(message);
}

static const char *find_key(const char *line, const char *key) {
    const char *cursor = line;
    size_t key_length;
    if (line == NULL || key == NULL) {
        return NULL;
    }
    key_length = strlen(key);

    /*
     * Scan JSON string tokens structurally instead of using strstr(). This
     * deliberately supports the small JSON envelope emitted by Rust without
     * mistaking an escaped key-looking substring inside a user string value
     * for an object member name. Nested objects are allowed: the first real
     * member whose unescaped ASCII key matches is returned.
     */
    while (*cursor != '\0') {
        const char *start;
        const char *end;
        const char *after;
        int escaped = 0;
        if (*cursor != '"') {
            ++cursor;
            continue;
        }
        start = ++cursor;
        while (*cursor != '\0') {
            if (escaped) {
                escaped = 0;
                ++cursor;
                continue;
            }
            if (*cursor == '\\') {
                escaped = 1;
                ++cursor;
                continue;
            }
            if (*cursor == '"') {
                break;
            }
            ++cursor;
        }
        if (*cursor != '"') {
            return NULL;
        }
        end = cursor;
        after = cursor + 1;
        while (*after != '\0' && isspace((unsigned char)*after)) {
            ++after;
        }
        if (
            *after == ':'
            && (size_t)(end - start) == key_length
            && memchr(start, '\\', key_length) == NULL
            && memcmp(start, key, key_length) == 0
        ) {
            ++after;
            while (*after != '\0' && isspace((unsigned char)*after)) {
                ++after;
            }
            return after;
        }
        ++cursor;
    }
    return NULL;
}

int bridge_json_string(
    const char *line,
    const char *key,
    char *output,
    size_t capacity,
    const char *fallback
) {
    const char *cursor = find_key(line, key);
    size_t length = 0u;
    if (capacity == 0u) {
        return 0;
    }
    if (cursor == NULL || *cursor != '"') {
        if (fallback != NULL) {
            (void)snprintf(output, capacity, "%s", fallback);
            return 1;
        }
        output[0] = '\0';
        return 0;
    }
    ++cursor;
    while (*cursor != '\0' && *cursor != '"' && length + 1u < capacity) {
        if (*cursor == '\\' && cursor[1] != '\0') {
            ++cursor;
            switch (*cursor) {
                case 'n': output[length++] = '\n'; break;
                case 'r': output[length++] = '\r'; break;
                case 't': output[length++] = '\t'; break;
                case 'b': output[length++] = '\b'; break;
                case 'f': output[length++] = '\f'; break;
                default: output[length++] = *cursor; break;
            }
        } else {
            output[length++] = *cursor;
        }
        ++cursor;
    }
    output[length] = '\0';
    return *cursor == '"';
}

uint32_t bridge_json_u32(const char *line, const char *key, uint32_t fallback) {
    const char *cursor = find_key(line, key);
    char *end = NULL;
    unsigned long value;
    if (cursor == NULL || *cursor == '-' || !isdigit((unsigned char)*cursor)) {
        return fallback;
    }
    errno = 0;
    value = strtoul(cursor, &end, 10);
    if (errno == ERANGE || end == cursor || value > 0xffffffffUL) {
        return fallback;
    }
    while (*end != '\0' && isspace((unsigned char)*end)) {
        ++end;
    }
    if (*end != '\0' && *end != ',' && *end != '}') {
        return fallback;
    }
    return (uint32_t)value;
}

int bridge_json_bool(const char *line, const char *key, int fallback) {
    const char *cursor = find_key(line, key);
    if (cursor == NULL) {
        return fallback;
    }
    if (strncmp(cursor, "true", 4u) == 0) {
        return 1;
    }
    if (strncmp(cursor, "false", 5u) == 0) {
        return 0;
    }
    return fallback;
}

static int hex_value(char character) {
    if (character >= '0' && character <= '9') {
        return character - '0';
    }
    if (character >= 'a' && character <= 'f') {
        return character - 'a' + 10;
    }
    if (character >= 'A' && character <= 'F') {
        return character - 'A' + 10;
    }
    return -1;
}

UINT8 *bridge_hex_decode(const char *text, UINT32 *size) {
    size_t length;
    size_t index;
    UINT8 *output;
    if (size == NULL) {
        return NULL;
    }
    *size = 0u;
    if (text == NULL || *text == '\0') {
        return NULL;
    }
    length = strlen(text);
    if ((length & 1u) != 0u || length / 2u > BRIDGE_MAX_PAYLOAD) {
        return NULL;
    }
    output = (UINT8 *)malloc(length / 2u);
    if (output == NULL) {
        return NULL;
    }
    for (index = 0u; index < length; index += 2u) {
        int high = hex_value(text[index]);
        int low = hex_value(text[index + 1u]);
        if (high < 0 || low < 0) {
            free(output);
            return NULL;
        }
        output[index / 2u] = (UINT8)((high << 4) | low);
    }
    *size = (UINT32)(length / 2u);
    return output;
}

void bridge_uuid_to_text(const TRDP_UUID_T uuid, char output[37]) {
    const UINT8 *bytes = (const UINT8 *)uuid;
    (void)snprintf(
        output,
        37u,
        "%02x%02x%02x%02x-%02x%02x-%02x%02x-%02x%02x-%02x%02x%02x%02x%02x%02x",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    );
}

int bridge_uuid_parse(const char *text, TRDP_UUID_T uuid) {
    UINT8 *bytes = (UINT8 *)uuid;
    int high = -1;
    size_t count = 0u;
    const char *cursor = text;
    if (text == NULL) {
        return 0;
    }
    while (*cursor != '\0') {
        int value;
        if (*cursor == '-' || *cursor == '{' || *cursor == '}') {
            ++cursor;
            continue;
        }
        value = hex_value(*cursor++);
        if (value < 0) {
            return 0;
        }
        if (high < 0) {
            high = value;
        } else {
            if (count >= sizeof(TRDP_UUID_T)) {
                return 0;
            }
            bytes[count++] = (UINT8)((high << 4) | value);
            high = -1;
        }
    }
    return high < 0 && count == sizeof(TRDP_UUID_T);
}
